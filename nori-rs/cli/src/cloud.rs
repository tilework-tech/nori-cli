//! `nori cloud`: pin the TUI's agent adapter to a `nori-handroll cloud-acp`
//! child process.
//!
//! The CLI knows nothing about brokers, auth, or WebSockets — all Sessions
//! concerns live in `nori-handroll`. This module only resolves the handroll
//! binary and builds the registry entry that the TUI spawns like any other
//! local ACP agent.

use std::path::Path;
use std::path::PathBuf;

use nori_acp::config::AgentConfigToml;

/// Registry slug for the pinned cloud agent.
pub const CLOUD_AGENT_SLUG: &str = "nori-cloud";

/// Resolve the `nori-handroll` binary.
///
/// `env_override` (from `NORI_HANDROLL_BIN`) wins when set, and must exist —
/// a dangling override is an error, not a fallback. Otherwise the first
/// `nori-handroll` on `path_var` (the `PATH` value) is used. Errors are
/// actionable: they tell the user to install Nori Sessions.
pub fn resolve_handroll_bin(
    env_override: Option<std::ffi::OsString>,
    path_var: Option<std::ffi::OsString>,
) -> anyhow::Result<PathBuf> {
    if let Some(override_path) = env_override {
        let override_path = PathBuf::from(override_path);
        if override_path.is_file() {
            return Ok(override_path);
        }
        anyhow::bail!(
            "NORI_HANDROLL_BIN points at {} but no such file exists",
            override_path.display()
        );
    }

    let found = path_var
        .iter()
        .flat_map(std::env::split_paths)
        .map(|dir| dir.join("nori-handroll"))
        .find(|candidate| candidate.is_file());
    found.ok_or_else(|| {
        anyhow::anyhow!(
            "nori cloud requires nori-handroll, which was not found on your PATH \u{2014} \
             install Nori Sessions to get it"
        )
    })
}

/// Build the `[[agents]]`-equivalent registry entry for the cloud agent:
/// the resolved handroll binary running `cloud-acp`, with the CLI's
/// configured broker URL (if any) translated to `NORI_BROKER_URL`.
pub fn cloud_agent_config(handroll_bin: &Path, broker_url: Option<&str>) -> AgentConfigToml {
    let mut env = std::collections::HashMap::new();
    if let Some(url) = broker_url {
        env.insert("NORI_BROKER_URL".to_string(), url.to_string());
    }
    AgentConfigToml {
        name: "Nori Cloud".to_string(),
        slug: CLOUD_AGENT_SLUG.to_string(),
        distribution: nori_acp::config::AgentDistributionToml {
            local: Some(nori_acp::config::LocalDistribution {
                command: handroll_bin.to_string_lossy().into_owned(),
                args: vec!["cloud-acp".to_string()],
                env,
            }),
            ..Default::default()
        },
        context_window_size: None,
        auth_hint: Some("run: nori-handroll login".to_string()),
        transcript_base_dir: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn executable_in(dir: &Path, name: &str) -> PathBuf {
        let bin = dir.join(name);
        std::fs::write(&bin, "#!/bin/sh\nexit 0\n").expect("write fake binary");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755))
                .expect("chmod fake binary");
        }
        bin
    }

    #[test]
    fn env_override_wins_over_path() {
        let override_dir = tempfile::tempdir().expect("tempdir");
        let path_dir = tempfile::tempdir().expect("tempdir");
        let override_bin = executable_in(override_dir.path(), "custom-handroll");
        executable_in(path_dir.path(), "nori-handroll");

        let resolved = resolve_handroll_bin(
            Some(override_bin.clone().into_os_string()),
            Some(path_dir.path().as_os_str().to_os_string()),
        )
        .expect("override should resolve");

        assert_eq!(resolved, override_bin);
    }

    #[test]
    fn dangling_env_override_is_an_error_not_a_fallback() {
        let path_dir = tempfile::tempdir().expect("tempdir");
        executable_in(path_dir.path(), "nori-handroll");

        let err = resolve_handroll_bin(
            Some("/nonexistent/nori-handroll".into()),
            Some(path_dir.path().as_os_str().to_os_string()),
        )
        .expect_err("dangling override must fail loudly");

        assert!(
            err.to_string().contains("NORI_HANDROLL_BIN"),
            "error should name the env override, got: {err}"
        );
    }

    #[test]
    fn finds_handroll_on_path() {
        let other_dir = tempfile::tempdir().expect("tempdir");
        let path_dir = tempfile::tempdir().expect("tempdir");
        let bin = executable_in(path_dir.path(), "nori-handroll");

        let path_var =
            std::env::join_paths([other_dir.path(), path_dir.path()]).expect("join paths");
        let resolved =
            resolve_handroll_bin(None, Some(path_var)).expect("PATH lookup should resolve");

        assert_eq!(resolved, bin);
    }

    #[test]
    fn missing_binary_yields_actionable_install_error() {
        let empty_dir = tempfile::tempdir().expect("tempdir");

        let err = resolve_handroll_bin(None, Some(empty_dir.path().as_os_str().to_os_string()))
            .expect_err("missing binary must fail before the TUI starts");

        let message = err.to_string();
        assert!(
            message.contains("nori-handroll") && message.contains("Nori Sessions"),
            "error must name the binary and point at Nori Sessions, got: {message}"
        );
    }

    #[test]
    fn cloud_agent_pins_handroll_cloud_acp() {
        let config = cloud_agent_config(Path::new("/opt/bin/nori-handroll"), None);

        assert_eq!(config.slug, CLOUD_AGENT_SLUG);
        let resolved = config
            .distribution
            .resolve()
            .expect("distribution must be valid");
        match resolved {
            nori_acp::config::ResolvedDistribution::Local { command, args, env } => {
                assert_eq!(command, "/opt/bin/nori-handroll");
                assert_eq!(args, vec!["cloud-acp".to_string()]);
                assert!(
                    !env.contains_key("NORI_BROKER_URL"),
                    "no broker URL configured means no NORI_BROKER_URL override"
                );
            }
            other => panic!("cloud agent must use a local distribution, got: {other:?}"),
        }
        assert!(
            config
                .auth_hint
                .as_deref()
                .is_some_and(|hint| hint.contains("nori-handroll login")),
            "auth failures must point at `nori-handroll login`, got: {:?}",
            config.auth_hint
        );
    }

    #[test]
    fn cloud_agent_translates_broker_url_to_env() {
        let config = cloud_agent_config(
            Path::new("/opt/bin/nori-handroll"),
            Some("http://broker.test:19400"),
        );

        let resolved = config
            .distribution
            .resolve()
            .expect("distribution must be valid");
        match resolved {
            nori_acp::config::ResolvedDistribution::Local { env, .. } => {
                assert_eq!(
                    env.get("NORI_BROKER_URL").map(String::as_str),
                    Some("http://broker.test:19400"),
                    "[cloud] broker_url must reach the child as NORI_BROKER_URL"
                );
            }
            other => panic!("cloud agent must use a local distribution, got: {other:?}"),
        }
    }
}
