//! Expands the model list the Claude ACP adapter offers in `/model`.
//!
//! The adapter advertises a short curated set that omits models the account can
//! actually run. Anthropic publishes the full id list in the generated Python
//! SDK, so nori fetches it at agent spawn and injects it through
//! `CLAUDE_CODE_EXECUTABLE` (see [`shim`]). Every failure path resolves to
//! `None`, which leaves the adapter's own list in place.

mod catalog;
mod shim;

use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;

use tracing::debug;

/// Anthropic's generated model id union. Public and unauthenticated.
const CATALOG_URL: &str = "https://raw.githubusercontent.com/anthropics/anthropic-sdk-python/main/src/anthropic/types/model.py";

/// Model lifecycle table, used to drop models that are no longer active.
const DEPRECATIONS_URL: &str =
    "https://platform.claude.com/docs/en/about-claude/model-deprecations.md";

/// Keeps a slow network from delaying agent startup.
const FETCH_TIMEOUT: Duration = Duration::from_secs(3);

/// Resolves the `CLAUDE_CODE_EXECUTABLE` wrapper that widens the model picker,
/// or `None` when the catalog is unavailable and nothing is cached.
pub(crate) async fn claude_executable_override(
    cache_dir: &Path,
    claude_path: &Path,
) -> Option<PathBuf> {
    // Proxy settings are honoured here: a user behind a corporate proxy still
    // needs to reach Anthropic.
    let client = reqwest::Client::builder()
        .timeout(FETCH_TIMEOUT)
        .build()
        .ok()?;
    resolve(
        &client,
        cache_dir,
        claude_path,
        CATALOG_URL,
        DEPRECATIONS_URL,
    )
    .await
}

async fn resolve(
    client: &reqwest::Client,
    cache_dir: &Path,
    claude_path: &Path,
    catalog_url: &str,
    deprecations_url: &str,
) -> Option<PathBuf> {
    // Neither fetch depends on the other, so pay for one round trip, not two.
    let (catalog_source, deprecations) = tokio::join!(
        fetch_text(client, catalog_url),
        fetch_text(client, deprecations_url),
    );

    let cache_path = cache_dir.join("anthropic-models.json");
    // A freshly published catalog always wins; the cache only covers the case
    // where Anthropic is unreachable or answering with something unusable.
    let ids = match catalog_source.map(|source| catalog::parse_model_ids(&source)) {
        Some(fetched) if !fetched.is_empty() => {
            cache_ids(cache_dir, &cache_path, &fetched);
            fetched
        }
        Some(_) | None => read_cached_ids(&cache_path),
    };
    if ids.is_empty() {
        debug!("No Claude model catalog available; leaving the agent's own list in place");
        return None;
    }

    // Lifecycle data is a refinement, not a requirement.
    let models = catalog::usable_models(ids, &deprecations.unwrap_or_default());
    if models.is_empty() {
        debug!("Every published Claude model was filtered out; leaving the agent's list in place");
        return None;
    }

    match shim::write_shim(cache_dir, claude_path, &models) {
        Ok(shim_path) => Some(shim_path),
        Err(error) => {
            debug!("Could not write the Claude model wrapper: {error}");
            None
        }
    }
}

async fn fetch_text(client: &reqwest::Client, url: &str) -> Option<String> {
    let response = client.get(url).send().await.ok()?;
    if !response.status().is_success() {
        return None;
    }
    response.text().await.ok()
}

fn cache_ids(cache_dir: &Path, cache_path: &Path, ids: &[String]) {
    let cached = std::fs::create_dir_all(cache_dir)
        .map_err(anyhow::Error::from)
        .and_then(|()| Ok(std::fs::write(cache_path, serde_json::to_vec(ids)?)?));
    if let Err(error) = cached {
        debug!("Could not cache the Claude model catalog: {error}");
    }
}

fn read_cached_ids(cache_path: &Path) -> Vec<String> {
    std::fs::read(cache_path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Vec<String>>(&bytes).ok())
        .unwrap_or_default()
        .into_iter()
        .filter(|id| catalog::is_model_id(id))
        .collect()
}

#[cfg(test)]
#[cfg(unix)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use std::fs;
    use tokio::io::AsyncReadExt;
    use tokio::io::AsyncWriteExt;
    use tokio::net::TcpListener;

    const SDK_SOURCE: &str = r#"
Model: TypeAlias = Union[
    Literal[
        "claude-opus-4-8",
        "claude-sonnet-5",
    ],
    str,
]
"#;

    /// A status table retiring one of the ids `SDK_SOURCE` publishes.
    const DEPRECATIONS: &str = "\
| API model name  | Current state | Deprecated   | Tentative retirement date |
| --------------- | ------------- | ------------ | ------------------------- |
| claude-opus-4-8 | Active        | N/A          | Not sooner than May 2027  |
| claude-sonnet-5 | Retired       | June 5, 2026 | August 5, 2026            |
";

    /// A URL that always refuses connections, standing in for "Anthropic is
    /// unreachable" without mocking the HTTP client.
    const UNREACHABLE_URL: &str = "http://127.0.0.1:1/unreachable";

    /// Bypasses any ambient proxy so the tests talk to their own loopback
    /// server rather than a corporate proxy that would fail them spuriously.
    fn client() -> reqwest::Client {
        reqwest::Client::builder()
            .no_proxy()
            .timeout(FETCH_TIMEOUT)
            .build()
            .expect("build client")
    }

    fn write_stub_claude(path: &Path) {
        use std::os::unix::fs::PermissionsExt;
        fs::write(path, "#!/bin/sh\nprintf '%s\\n' \"$@\"\n").expect("write stub");
        let mut perms = fs::metadata(path).expect("stat stub").permissions();
        perms.set_mode(0o755);
        fs::set_permissions(path, perms).expect("chmod stub");
    }

    /// A stand-in for Anthropic's docs host whose response the test can change,
    /// so cache behaviour is observable without knowing the cache format.
    struct Upstream {
        url: String,
        response: std::sync::Arc<std::sync::Mutex<(u16, String)>>,
    }

    impl Upstream {
        async fn serving(body: &str) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
            let addr = listener.local_addr().expect("addr");
            let response = std::sync::Arc::new(std::sync::Mutex::new((200, body.to_string())));
            let served = std::sync::Arc::clone(&response);
            tokio::spawn(async move {
                while let Ok((mut socket, _)) = listener.accept().await {
                    let mut buf = [0_u8; 2048];
                    let _ = socket.read(&mut buf).await;
                    let (status, payload) = served.lock().expect("response lock").clone();
                    let reason = if status == 200 { "OK" } else { "Not Found" };
                    let head = format!(
                        "HTTP/1.1 {status} {reason}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        payload.len()
                    );
                    let _ = socket.write_all(head.as_bytes()).await;
                    let _ = socket.write_all(payload.as_bytes()).await;
                    let _ = socket.shutdown().await;
                }
            });
            Self {
                url: format!("http://{addr}/"),
                response,
            }
        }

        fn now_serves(&self, body: &str) {
            *self.response.lock().expect("response lock") = (200, body.to_string());
        }

        fn now_fails(&self) {
            *self.response.lock().expect("response lock") = (404, "Not Found".to_string());
        }
    }

    fn models_offered_by(shim: &Path) -> Vec<String> {
        let output = std::process::Command::new(shim).output().expect("run shim");
        assert!(
            output.status.success(),
            "shim failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let argv = String::from_utf8(output.stdout)
            .expect("utf8 argv")
            .lines()
            .map(str::to_string)
            .collect::<Vec<_>>();
        let flag = argv
            .iter()
            .position(|arg| arg == "--settings")
            .expect("shim passes --settings");
        let path = argv.get(flag + 1).expect("--settings has a value");
        let settings: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(path).expect("read settings"))
                .expect("valid settings json");
        settings["availableModels"]
            .as_array()
            .expect("availableModels array")
            .iter()
            .map(|value| value.as_str().expect("model id is a string").to_string())
            .collect()
    }

    /// Separate directories so the generated wrapper can never collide with the
    /// stub binary it wraps.
    fn workspace() -> (tempfile::TempDir, tempfile::TempDir, PathBuf) {
        let cache = tempfile::tempdir().expect("cache dir");
        let bin = tempfile::tempdir().expect("bin dir");
        let claude = bin.path().join("claude");
        write_stub_claude(&claude);
        (cache, bin, claude)
    }

    #[tokio::test]
    async fn offers_the_published_catalog_minus_models_anthropic_retired() {
        let (cache, _bin, claude) = workspace();
        let catalog = Upstream::serving(SDK_SOURCE).await;
        let deprecations = Upstream::serving(DEPRECATIONS).await;

        let shim = resolve(
            &client(),
            cache.path(),
            &claude,
            &catalog.url,
            &deprecations.url,
        )
        .await
        .expect("a wrapper is produced when the catalog is reachable");

        assert_eq!(
            models_offered_by(&shim),
            vec!["claude-opus-4-8".to_string()],
            "claude-sonnet-5 is retired upstream and must not be offered"
        );
    }

    #[tokio::test]
    async fn offers_the_whole_catalog_when_only_the_deprecations_page_is_unavailable() {
        let (cache, _bin, claude) = workspace();
        let catalog = Upstream::serving(SDK_SOURCE).await;

        let shim = resolve(
            &client(),
            cache.path(),
            &claude,
            &catalog.url,
            UNREACHABLE_URL,
        )
        .await
        .expect("a missing deprecations page must not suppress the catalog");

        assert_eq!(
            models_offered_by(&shim),
            vec!["claude-opus-4-8".to_string(), "claude-sonnet-5".to_string()]
        );
    }

    #[tokio::test]
    async fn keeps_offering_the_catalog_after_anthropic_becomes_unreachable() {
        let (cache, _bin, claude) = workspace();
        let catalog = Upstream::serving(SDK_SOURCE).await;

        resolve(
            &client(),
            cache.path(),
            &claude,
            &catalog.url,
            UNREACHABLE_URL,
        )
        .await
        .expect("first resolve succeeds");
        let offline = resolve(
            &client(),
            cache.path(),
            &claude,
            UNREACHABLE_URL,
            UNREACHABLE_URL,
        )
        .await
        .expect("a previously fetched catalog survives losing the network");

        assert_eq!(
            models_offered_by(&offline),
            vec!["claude-opus-4-8".to_string(), "claude-sonnet-5".to_string()]
        );
    }

    #[tokio::test]
    async fn keeps_offering_the_catalog_when_anthropic_answers_with_an_error() {
        let (cache, _bin, claude) = workspace();
        let catalog = Upstream::serving(SDK_SOURCE).await;

        resolve(
            &client(),
            cache.path(),
            &claude,
            &catalog.url,
            UNREACHABLE_URL,
        )
        .await
        .expect("first resolve succeeds");
        catalog.now_fails();
        let degraded = resolve(
            &client(),
            cache.path(),
            &claude,
            &catalog.url,
            UNREACHABLE_URL,
        )
        .await
        .expect("a 404 must not discard a known-good catalog");

        assert_eq!(
            models_offered_by(&degraded),
            vec!["claude-opus-4-8".to_string(), "claude-sonnet-5".to_string()]
        );
    }

    #[tokio::test]
    async fn picks_up_models_anthropic_publishes_later() {
        let (cache, _bin, claude) = workspace();
        let catalog = Upstream::serving(SDK_SOURCE).await;

        resolve(
            &client(),
            cache.path(),
            &claude,
            &catalog.url,
            UNREACHABLE_URL,
        )
        .await
        .expect("first resolve succeeds");
        catalog.now_serves(
            r#"
    Literal[
        "claude-opus-4-8",
        "claude-sonnet-5",
        "claude-opus-9-9",
    ],
"#,
        );
        let refreshed = resolve(
            &client(),
            cache.path(),
            &claude,
            &catalog.url,
            UNREACHABLE_URL,
        )
        .await
        .expect("second resolve succeeds");

        assert_eq!(
            models_offered_by(&refreshed),
            vec![
                "claude-opus-4-8".to_string(),
                "claude-sonnet-5".to_string(),
                "claude-opus-9-9".to_string(),
            ],
            "a cached catalog must not pin the picker to yesterday's models"
        );
    }

    #[tokio::test]
    async fn leaves_the_agent_untouched_when_every_published_model_is_retired() {
        let (cache, _bin, claude) = workspace();
        let catalog = Upstream::serving(SDK_SOURCE).await;
        let deprecations = Upstream::serving(
            "\
| API model name  | Current state | Deprecated   | Tentative retirement date |
| --------------- | ------------- | ------------ | ------------------------- |
| claude-opus-4-8 | Retired       | June 5, 2026 | August 5, 2026            |
| claude-sonnet-5 | Retired       | June 5, 2026 | August 5, 2026            |
",
        )
        .await;

        let resolved = resolve(
            &client(),
            cache.path(),
            &claude,
            &catalog.url,
            &deprecations.url,
        )
        .await;

        assert_eq!(
            resolved, None,
            "filtering everything away must fall back to the adapter's list, never offer an empty picker"
        );
    }

    #[tokio::test]
    async fn leaves_the_agent_untouched_when_the_catalog_is_not_a_catalog() {
        let (cache, _bin, claude) = workspace();
        let catalog = Upstream::serving("<html><title>404 Not Found</title></html>").await;

        let resolved = resolve(
            &client(),
            cache.path(),
            &claude,
            &catalog.url,
            UNREACHABLE_URL,
        )
        .await;

        assert_eq!(
            resolved, None,
            "an unparseable catalog must fall back to the adapter's list, never offer an empty picker"
        );
    }

    #[tokio::test]
    async fn leaves_the_agent_untouched_when_the_cache_is_unusable() {
        let (cache, _bin, claude) = workspace();
        fs::write(cache.path().join("anthropic-models.json"), "{ truncated")
            .expect("write corrupt cache");

        let resolved = resolve(
            &client(),
            cache.path(),
            &claude,
            UNREACHABLE_URL,
            UNREACHABLE_URL,
        )
        .await;

        assert_eq!(
            resolved, None,
            "a corrupt cache must not be trusted into the picker"
        );
    }

    #[tokio::test]
    async fn leaves_the_agent_untouched_when_the_catalog_was_never_available() {
        let (cache, _bin, claude) = workspace();

        let resolved = resolve(
            &client(),
            cache.path(),
            &claude,
            UNREACHABLE_URL,
            UNREACHABLE_URL,
        )
        .await;

        assert_eq!(
            resolved, None,
            "with no catalog and no cache the adapter's own model list must stand"
        );
    }
}
