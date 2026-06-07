//! Parsing helpers for the CDP connection details embedded in the `/browser`
//! agent prompt (see `compose_agent_prompt` in `browser_session.rs`).
//!
//! This lives outside the `#[cfg(unix)]` `browser_session` module so the
//! test-only `mock-acp-agent` and `tui-pty-e2e` crates can reuse the same
//! parser instead of re-implementing it.

/// Prefix of the line in the browser-session prompt that advertises the CDP
/// HTTP endpoint, e.g. `CDP endpoint: http://127.0.0.1:9222`.
const CDP_ENDPOINT_PREFIX: &str = "CDP endpoint: http://127.0.0.1:";

/// Extract the CDP HTTP port from text containing a browser-session prompt
/// line of the form `CDP endpoint: http://127.0.0.1:<port>`.
///
/// Each line is trimmed before matching so this works on both the raw agent
/// prompt and scraped terminal output (which may be padded with whitespace).
pub fn extract_cdp_port_from_prompt(text: &str) -> Option<i32> {
    text.lines().find_map(|line| {
        line.trim()
            .strip_prefix(CDP_ENDPOINT_PREFIX)
            .and_then(|rest| rest.trim().parse::<i32>().ok())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn extracts_port_from_endpoint_line() {
        let text =
            "[Browser Session] ...\nCDP endpoint: http://127.0.0.1:9222\nWebSocket: ws://...";
        assert_eq!(extract_cdp_port_from_prompt(text), Some(9222));
    }

    #[test]
    fn extracts_high_random_port() {
        assert_eq!(
            extract_cdp_port_from_prompt("CDP endpoint: http://127.0.0.1:41567"),
            Some(41567)
        );
    }

    #[test]
    fn tolerates_leading_whitespace_from_screen_scrape() {
        assert_eq!(
            extract_cdp_port_from_prompt("      CDP endpoint: http://127.0.0.1:8080    "),
            Some(8080)
        );
    }

    #[test]
    fn returns_none_without_endpoint_line() {
        assert_eq!(extract_cdp_port_from_prompt("no cdp endpoint here"), None);
        assert_eq!(extract_cdp_port_from_prompt(""), None);
        assert_eq!(
            extract_cdp_port_from_prompt("CDP endpoint: http://127.0.0.1:not-a-port"),
            None
        );
    }

    /// Guard the produce/parse contract: the parser must accept whatever
    /// `compose_agent_prompt` actually emits.
    #[cfg(unix)]
    #[test]
    fn round_trips_compose_agent_prompt() {
        let prompt = super::super::browser_session::compose_agent_prompt(
            "ws://127.0.0.1:9222/devtools/browser/abc-123",
            9222,
        );
        assert_eq!(extract_cdp_port_from_prompt(&prompt), Some(9222));
    }
}
