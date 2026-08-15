/// The current Codex CLI version as embedded at compile time.
pub const CODEX_CLI_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Whether verbose ACP session-info history cells should be shown.
///
/// Stable and latest releases keep the transcript quiet. Unstable `-next*`
/// channel builds and debug builds keep the full metadata dump for harness
/// debugging.
pub(crate) fn show_verbose_session_info_history() -> bool {
    cfg!(debug_assertions) || is_next_channel_version(CODEX_CLI_VERSION)
}

/// True for prerelease versions whose identifier starts with `next`
/// (for example `0.9.0-next.3`).
pub(crate) fn is_next_channel_version(version: &str) -> bool {
    version
        .split_once('-')
        .is_some_and(|(_, pre)| pre.starts_with("next"))
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::is_next_channel_version;

    #[test]
    fn next_channel_versions_match_x_y_z_next_prefix() {
        assert_eq!(is_next_channel_version("0.9.0-next.3"), true);
        assert_eq!(is_next_channel_version("1.2.3-next"), true);
        assert_eq!(is_next_channel_version("1.2.3-next.0"), true);
        assert_eq!(is_next_channel_version("1.2.3"), false);
        assert_eq!(is_next_channel_version("0.0.0"), false);
        assert_eq!(is_next_channel_version("1.2.3-alpha.1"), false);
        assert_eq!(is_next_channel_version("1.2.3-rc.1"), false);
    }
}
