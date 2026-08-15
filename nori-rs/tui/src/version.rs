/// The current Codex CLI version as embedded at compile time.
pub const CODEX_CLI_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Whether this build is on an unstable channel: a local debug build, or an
/// `X.Y.Z-next*` prerelease cut by `scripts/create_nori_release --publish-next`.
///
/// Diagnostics that document what a harness supports — the full ACP
/// session-info metadata dump, for example — are valuable while developing
/// against an agent and pure noise for everyone else, so they render here and
/// stay out of stable releases.
pub(crate) fn is_unstable_build() -> bool {
    cfg!(debug_assertions) || is_next_prerelease(CODEX_CLI_VERSION)
}

/// Whether a semver string carries a `next` prerelease suffix (`1.4.0-next.2`).
/// Matches the first prerelease identifier exactly, so an unrelated channel
/// like `1.4.0-nextgen.1` is not mistaken for one of ours.
fn is_next_prerelease(version: &str) -> bool {
    version
        .split_once('+')
        .map_or(version, |(without_build_metadata, _)| {
            without_build_metadata
        })
        .split_once('-')
        .is_some_and(|(_, prerelease)| {
            prerelease
                .split('.')
                .next()
                .is_some_and(|identifier| identifier == "next")
        })
}

#[cfg(test)]
mod tests {
    use super::is_next_prerelease;
    use pretty_assertions::assert_eq;

    #[test]
    fn only_next_prereleases_are_unstable_versions() {
        assert_eq!(
            [
                "1.4.0-next.2",
                "1.4.0-next",
                "1.4.0",
                "1.4.0-alpha.1",
                "1.4.0-nextgen.1",
                "1.4.0+build-next",
            ]
            .map(is_next_prerelease),
            [true, true, false, false, false, false]
        );
    }
}
