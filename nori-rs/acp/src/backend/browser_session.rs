use std::path::PathBuf;
use std::sync::Mutex;

use anyhow::Context as _;
use anyhow::Result;
use anyhow::bail;
use tokio::io::AsyncBufReadExt as _;
use tokio::io::BufReader;

const CHROME_CANDIDATES: &[&str] = &[
    "google-chrome-stable",
    "google-chrome",
    "chromium-browser",
    "chromium",
];

static ACTIVE_SESSION: Mutex<Option<BrowserSession>> = Mutex::new(None);

/// Whether a browser session is currently active.
pub fn is_browser_active() -> bool {
    ACTIVE_SESSION.lock().map(|s| s.is_some()).unwrap_or(false)
}

/// Returns the (ws_url, cdp_port) of the active session, if any.
pub fn active_session_info() -> Option<(String, i32)> {
    ACTIVE_SESSION
        .lock()
        .ok()
        .and_then(|s| s.as_ref().map(|b| (b.ws_url.clone(), b.cdp_port)))
}

/// Store a browser session as the active session, closing any previous one.
fn store_session(session: BrowserSession) {
    if let Ok(mut guard) = ACTIVE_SESSION.lock() {
        *guard = Some(session);
    }
}

/// Terminate the active browser session, if any, and remove its profile dir.
///
/// The session is owned by a process-lifetime `static`, which Rust never drops
/// at exit, so the nori shutdown path must call this explicitly. Dropping the
/// taken session kills Chrome (`kill_on_drop`) and removes the temp profile
/// (`TempDir`). A no-op when no session is active; safe to call more than once.
pub fn shutdown_active_session() {
    if let Ok(mut guard) = ACTIVE_SESSION.lock() {
        guard.take();
    }
}

/// Parses the CDP WebSocket URL from a line of Chrome's stderr output.
///
/// Chrome prints a line like:
///   `DevTools listening on ws://127.0.0.1:9222/devtools/browser/<uuid>`
/// when launched with `--remote-debugging-port`.
pub fn parse_cdp_ws_url(line: &str) -> Option<String> {
    let marker = "DevTools listening on ";
    let idx = line.find(marker)?;
    let url = line[idx + marker.len()..].trim();
    if url.starts_with("ws://") {
        Some(url.to_string())
    } else {
        None
    }
}

/// Extracts the port number from a CDP WebSocket URL.
///
/// Given `ws://127.0.0.1:9222/devtools/browser/...`, returns `9222`.
pub fn extract_cdp_port(ws_url: &str) -> Option<i32> {
    let after_scheme = ws_url.strip_prefix("ws://")?;
    let colon = after_scheme.find(':')?;
    let after_colon = &after_scheme[colon + 1..];
    let slash = after_colon.find('/').unwrap_or(after_colon.len());
    after_colon[..slash].parse().ok()
}

/// Searches for a Chrome or Chromium binary on the system.
pub fn find_chrome_binary() -> Result<PathBuf> {
    for candidate in CHROME_CANDIDATES {
        if let Ok(path) = which::which(candidate) {
            return Ok(path);
        }
    }
    bail!(
        "Could not find Chrome or Chromium. Searched for: {}. \
         Install Chrome or set one of these in your PATH.",
        CHROME_CANDIDATES.join(", ")
    )
}

/// A running Chrome browser session with CDP enabled.
pub struct BrowserSession {
    /// Held only for its `kill_on_drop` side effect: dropping it kills Chrome.
    _child: tokio::process::Child,
    ws_url: String,
    cdp_port: i32,
    /// Throwaway Chrome profile. Declared last so it is removed only after the
    /// child has been dropped (and thus killed) above it.
    _user_data_dir: tempfile::TempDir,
}

impl BrowserSession {
    /// Launch Chrome in headed mode with a random CDP port, storing
    /// it as the active session. Closes any previous session.
    pub async fn launch_and_store() -> Result<(String, i32)> {
        if is_browser_active()
            && let Some((ws_url, cdp_port)) = active_session_info()
        {
            return Ok((ws_url, cdp_port));
        }

        let chrome = find_chrome_binary()?;

        // Launch against a throwaway profile so we never attach to (or disturb)
        // the user's already-running Chrome; reusing the default profile would
        // hand off to that instance and never expose CDP on our debugging port.
        let user_data_dir =
            tempfile::tempdir().context("failed to create temporary Chrome profile dir")?;

        let mut child = tokio::process::Command::new(&chrome)
            .arg("--remote-debugging-port=0")
            .arg(format!(
                "--user-data-dir={}",
                user_data_dir.path().display()
            ))
            .arg("--no-first-run")
            .arg("--no-default-browser-check")
            .arg("--disable-gpu")
            .arg("about:blank")
            .kill_on_drop(true)
            .stderr(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stdin(std::process::Stdio::null())
            .spawn()
            .with_context(|| format!("failed to launch Chrome: {}", chrome.display()))?;

        let stderr = child
            .stderr
            .take()
            .context("failed to capture Chrome stderr")?;

        let ws_url = tokio::time::timeout(
            std::time::Duration::from_secs(15),
            Self::read_ws_url_from_stderr(stderr),
        )
        .await
        .context("timed out waiting for Chrome CDP URL")?
        .context("failed to read CDP URL from Chrome stderr")?;

        let cdp_port =
            extract_cdp_port(&ws_url).context("failed to extract port from CDP WebSocket URL")?;

        tracing::info!("Browser launched: cdp_port={cdp_port} ws_url={ws_url}");

        let session = Self {
            _child: child,
            ws_url: ws_url.clone(),
            cdp_port,
            _user_data_dir: user_data_dir,
        };

        store_session(session);

        Ok((ws_url, cdp_port))
    }

    async fn read_ws_url_from_stderr(stderr: tokio::process::ChildStderr) -> Result<String> {
        let mut reader = BufReader::new(stderr).lines();
        while let Some(line) = reader.next_line().await? {
            tracing::debug!("Chrome stderr: {line}");
            if let Some(url) = parse_cdp_ws_url(&line) {
                return Ok(url);
            }
        }
        bail!("Chrome exited before printing CDP WebSocket URL")
    }
}

/// Compose the message that tells the agent about the browser session.
pub fn compose_agent_prompt(ws_url: &str, cdp_port: i32) -> String {
    format!(
        "[Browser Session] A headed Chrome browser has been launched and is visible to the user.\n\
         \n\
         CDP endpoint: http://127.0.0.1:{cdp_port}\n\
         WebSocket: {ws_url}\n\
         \n\
         You can control this browser by writing and executing scripts via your shell tool. \
         Use Playwright's `connectOverCDP('http://127.0.0.1:{cdp_port}')`, \
         puppeteer-core's `connect({{ browserWSEndpoint: '{ws_url}' }})`, \
         or raw CDP commands via curl/websocat.\n\
         \n\
         Do NOT call `browser.close()` — the browser stays open for the user. \
         Save screenshots to temp files and report the path."
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn parse_cdp_ws_url_extracts_url_from_chrome_stderr() {
        let line = "DevTools listening on ws://127.0.0.1:9222/devtools/browser/abc-def-123";
        let url = parse_cdp_ws_url(line).expect("should parse URL");
        assert_eq!(url, "ws://127.0.0.1:9222/devtools/browser/abc-def-123");
    }

    #[test]
    fn parse_cdp_ws_url_handles_random_port() {
        let line = "DevTools listening on ws://127.0.0.1:41567/devtools/browser/fa3b2c1d-e456-7890-abcd-ef1234567890";
        let url = parse_cdp_ws_url(line).expect("should parse URL with random port");
        assert!(url.contains("41567"));
    }

    #[test]
    fn parse_cdp_ws_url_returns_none_for_unrelated_output() {
        assert!(parse_cdp_ws_url("Starting Chrome...").is_none());
        assert!(parse_cdp_ws_url("").is_none());
        assert!(parse_cdp_ws_url("[0605/123456.789:INFO] some log line").is_none());
    }

    #[test]
    fn extract_cdp_port_from_ws_url() {
        let port =
            extract_cdp_port("ws://127.0.0.1:9222/devtools/browser/abc-123").expect("should parse");
        assert_eq!(port, 9222);
    }

    #[test]
    fn extract_cdp_port_from_high_port() {
        let port = extract_cdp_port("ws://127.0.0.1:41567/devtools/browser/abc-123")
            .expect("should parse");
        assert_eq!(port, 41567);
    }

    #[test]
    fn extract_cdp_port_returns_none_for_invalid_url() {
        assert!(extract_cdp_port("not a url").is_none());
        assert!(extract_cdp_port("http://localhost/foo").is_none());
    }

    #[test]
    fn compose_agent_prompt_contains_connection_details() {
        let prompt = compose_agent_prompt("ws://127.0.0.1:9222/devtools/browser/abc-123", 9222);
        assert!(
            prompt.contains("ws://127.0.0.1:9222/devtools/browser/abc-123"),
            "prompt should contain the WebSocket URL"
        );
        assert!(
            prompt.contains("9222"),
            "prompt should contain the CDP port"
        );
    }
}
