//! Launch control for built-in ACP adapters: per-agent command overrides,
//! package version pins, and the first-download notice.
#![allow(clippy::expect_used)]
use std::collections::HashMap;
use std::path::Path;

use nori_acp_host::registry::get_agent_config;
use nori_acp_host::registry::initialize_registry;
use nori_acp_host::registry::set_adapter_launch_overrides;
use nori_config::AcpAdapterToml;
use nori_config::NoriConfig;
use nori_config::NoriConfigOverrides;
use pretty_assertions::assert_eq;
use serial_test::serial;
use tempfile::TempDir;

const LAUNCH_ENV: &[&str] = &[
    "NORI_ACP_COMMAND_CLAUDE_CODE",
    "NORI_ACP_COMMAND_CODEX",
    "NORI_ACP_COMMAND_GROK_BUILD",
    "NORI_ACP_COMMAND_LOCAL_HELPER",
    "NORI_MANAGED_BY_BUN",
    "NORI_MANAGED_BY_NPM",
    "npm_config_cache",
    "TMPDIR",
];

/// Run `body` with a clean registry and exactly `vars` set among the launch
/// variables, restoring the previous environment afterwards.
fn with_launch_env(vars: &[(&str, &str)], body: impl FnOnce()) {
    let saved: Vec<(&str, Option<String>)> = LAUNCH_ENV
        .iter()
        .map(|name| (*name, std::env::var(name).ok()))
        .collect();
    // SAFETY: every test in this binary is #[serial], so no other thread reads
    // the environment while it changes.
    unsafe {
        for name in LAUNCH_ENV {
            std::env::remove_var(name);
        }
        for (name, value) in vars {
            std::env::set_var(name, value);
        }
    }
    initialize_registry(Vec::new()).expect("reset registry");
    set_adapter_launch_overrides(HashMap::new()).expect("clear overrides");
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(body));
    unsafe {
        for (name, value) in saved {
            match value {
                Some(value) => std::env::set_var(name, value),
                None => std::env::remove_var(name),
            }
        }
    }
    initialize_registry(Vec::new()).expect("reset registry");
    set_adapter_launch_overrides(HashMap::new()).expect("clear overrides");
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
}

fn adapters(entries: &[(&str, Option<&str>, Option<&str>)]) -> HashMap<String, AcpAdapterToml> {
    entries
        .iter()
        .map(|(slug, command, version)| {
            (
                slug.to_string(),
                AcpAdapterToml {
                    command: command.map(str::to_owned),
                    version: version.map(str::to_owned),
                },
            )
        })
        .collect()
}

fn command_line(agent: &str) -> Vec<String> {
    let config = get_agent_config(agent).expect("agent resolves");
    std::iter::once(config.command).chain(config.args).collect()
}

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(ToString::to_string).collect()
}

/// Seed the npx install directory `<cache>/_npx/<hash>` as `npx --yes`
/// leaves it. Hashes were taken from npm 10.9.3 (libnpmexec) for the spec set.
fn seed_npx_cache(cache: &Path, hash: &str, package: &str) {
    let dir = cache
        .join("_npx")
        .join(hash)
        .join("node_modules")
        .join(package);
    std::fs::create_dir_all(&dir).expect("create cache dir");
    std::fs::write(dir.join("package.json"), "{}").expect("write package.json");
}

#[test]
#[serial]
fn env_command_replaces_the_resolved_adapter_command() {
    with_launch_env(
        &[(
            "NORI_ACP_COMMAND_CLAUDE_CODE",
            "/opt/nori/claude-agent-acp --flag 'two words'",
        )],
        || {
            assert_eq!(
                command_line("claude-code"),
                strings(&["/opt/nori/claude-agent-acp", "--flag", "two words"])
            );
            assert_eq!(
                command_line("claude"),
                strings(&["/opt/nori/claude-agent-acp", "--flag", "two words"]),
                "aliases resolve to the same override"
            );
        },
    );
}

#[test]
#[serial]
fn an_overridden_builtin_keeps_its_builtin_launch_behavior() {
    let resolve_with_model = || {
        let mut config = get_agent_config("codex").expect("codex resolves");
        config.inject_model("gpt-5.3-codex").expect("inject model");
        config
    };
    let mut default_launch = None;
    with_launch_env(&[], || default_launch = Some(resolve_with_model()));
    let default_launch = default_launch.expect("default launch resolved");
    with_launch_env(&[("NORI_ACP_COMMAND_CODEX", "/opt/codex-acp")], || {
        let overridden = resolve_with_model();
        assert_eq!(overridden.command, "/opt/codex-acp");
        assert_eq!(overridden.args, Vec::<String>::new());
        assert_eq!(overridden.env, default_launch.env);
        assert_eq!(overridden.builtin_kind, default_launch.builtin_kind);
    });
}

#[test]
#[serial]
fn an_unusable_env_command_is_an_error_naming_the_variable() {
    for bad in ["", "   ", "unterminated 'quote"] {
        with_launch_env(&[("NORI_ACP_COMMAND_CLAUDE_CODE", bad)], || {
            let error = get_agent_config("claude-code")
                .expect_err("bad override must not launch")
                .to_string();
            assert!(
                error.contains("NORI_ACP_COMMAND_CLAUDE_CODE"),
                "error names the variable: {error}"
            );
        });
    }
}

#[test]
#[serial]
fn config_command_replaces_the_adapter_and_env_wins_over_config() {
    with_launch_env(&[], || {
        set_adapter_launch_overrides(adapters(&[("codex", Some("/from/config codex-acp"), None)]))
            .expect("valid overrides");
        assert_eq!(
            command_line("codex"),
            strings(&["/from/config", "codex-acp"])
        );
    });
    with_launch_env(&[("NORI_ACP_COMMAND_CODEX", "/from/env")], || {
        set_adapter_launch_overrides(adapters(&[("codex", Some("/from/config"), None)]))
            .expect("valid overrides");
        assert_eq!(command_line("codex"), strings(&["/from/env"]));
    });
}

#[test]
#[serial]
fn config_keys_accept_agent_aliases() {
    with_launch_env(&[], || {
        set_adapter_launch_overrides(adapters(&[("claude", Some("/opt/claude"), None)]))
            .expect("valid overrides");
        assert_eq!(command_line("claude-code"), strings(&["/opt/claude"]));
        set_adapter_launch_overrides(adapters(&[
            ("claude", Some("/opt/alias"), None),
            ("claude-code", Some("/opt/canonical"), None),
        ]))
        .expect("valid overrides");
        assert_eq!(
            command_line("claude"),
            strings(&["/opt/canonical"]),
            "the canonical key wins over an alias"
        );
    });
}

#[test]
#[serial]
fn the_override_applies_to_custom_agents_too() {
    with_launch_env(
        &[("NORI_ACP_COMMAND_LOCAL_HELPER", "/pinned/helper --acp")],
        || {
            initialize_registry(vec![
                serde_json::from_value(serde_json::json!({
                    "name": "Local Helper", "slug": "local-helper",
                    "distribution": {"local": {"command": "helper", "env": {"KEEP": "1"}}}
                }))
                .expect("agent toml"),
            ])
            .expect("registry");
            let config = get_agent_config("local-helper").expect("custom agent resolves");
            assert_eq!(config.command, "/pinned/helper");
            assert_eq!(config.args, strings(&["--acp"]));
            assert_eq!(
                config.env,
                HashMap::from([("KEEP".to_string(), "1".to_string())])
            );
        },
    );
}

#[test]
#[serial]
fn a_version_pin_replaces_latest_for_npx_and_prefers_the_offline_cache() {
    with_launch_env(&[("NORI_MANAGED_BY_NPM", "1")], || {
        set_adapter_launch_overrides(adapters(&[("claude-code", None, Some("0.12.3"))]))
            .expect("valid overrides");
        assert_eq!(
            command_line("claude-code"),
            strings(&[
                "npx",
                "--yes",
                "--prefer-offline",
                "@agentclientprotocol/claude-agent-acp@0.12.3"
            ])
        );
    });
}

#[test]
#[serial]
fn a_version_pin_applies_to_bunx_launches() {
    with_launch_env(&[("NORI_MANAGED_BY_BUN", "1")], || {
        set_adapter_launch_overrides(adapters(&[("codex", None, Some("0.4.1"))]))
            .expect("valid overrides");
        assert_eq!(
            command_line("codex"),
            strings(&["bunx", "@agentclientprotocol/codex-acp@0.4.1"])
        );
    });
}

#[test]
#[serial]
fn unpinned_adapters_keep_latest() {
    with_launch_env(&[("NORI_MANAGED_BY_NPM", "1")], || {
        set_adapter_launch_overrides(adapters(&[("codex", None, Some("0.4.1"))]))
            .expect("valid overrides");
        assert_eq!(
            command_line("claude-code"),
            strings(&[
                "npx",
                "--yes",
                "@agentclientprotocol/claude-agent-acp@latest"
            ])
        );
    });
}

#[test]
#[serial]
fn a_pi_pin_applies_to_its_acp_adapter_package() {
    with_launch_env(&[], || {
        set_adapter_launch_overrides(adapters(&[("pi", None, Some("1.2.0"))]))
            .expect("valid overrides");
        let line = command_line("pi");
        assert!(
            line.contains(&"--package=pi-acp@1.2.0".to_string()),
            "{line:?}"
        );
        assert!(
            line.contains(&"--package=@earendil-works/pi-coding-agent@latest".to_string()),
            "{line:?}"
        );
        assert!(line.contains(&"--prefer-offline".to_string()), "{line:?}");
    });
}

#[test]
#[serial]
fn a_command_override_wins_over_a_version_pin() {
    with_launch_env(&[], || {
        set_adapter_launch_overrides(adapters(&[(
            "claude-code",
            Some("/opt/claude"),
            Some("0.12.3"),
        )]))
        .expect("valid overrides");
        assert_eq!(command_line("claude-code"), strings(&["/opt/claude"]));
    });
}

#[test]
#[serial]
fn a_pin_on_an_agent_without_a_package_fails_its_launch() {
    with_launch_env(&[], || {
        set_adapter_launch_overrides(adapters(&[("opencode", None, Some("1.0.0"))]))
            .expect("valid overrides");
        let error = get_agent_config("opencode")
            .expect_err("pin cannot apply")
            .to_string();
        assert!(error.contains("acp_adapters.opencode.version"), "{error}");
        assert!(
            get_agent_config("claude-code").is_ok(),
            "other agents still launch"
        );
    });
}

#[test]
#[serial]
fn malformed_overrides_are_rejected() {
    with_launch_env(&[], || {
        for (command, version) in [
            (None, Some("")),
            (None, Some("1.0 && rm")),
            (Some(""), None),
            (Some("'unterminated"), None),
        ] {
            let error = set_adapter_launch_overrides(adapters(&[("codex", command, version)]))
                .expect_err("invalid override");
            assert!(
                error.to_string().contains("acp_adapters.codex"),
                "error names the key: {error}"
            );
        }
    });
}

#[test]
#[serial]
fn first_npx_fetch_carries_a_download_notice_until_the_package_is_cached() {
    let cache = TempDir::new().expect("cache dir");
    let cache_path = cache.path().to_str().expect("utf-8 path");
    with_launch_env(
        &[
            ("NORI_MANAGED_BY_NPM", "1"),
            ("npm_config_cache", cache_path),
        ],
        || {
            let notice = get_agent_config("claude-code")
                .expect("resolves")
                .download_notice
                .expect("uncached package announces a download");
            assert!(
                notice.contains("@agentclientprotocol/claude-agent-acp@latest"),
                "{notice}"
            );

            seed_npx_cache(
                cache.path(),
                "b555b4fead8494dc",
                "@agentclientprotocol/claude-agent-acp",
            );
            assert_eq!(
                get_agent_config("claude-code")
                    .expect("resolves")
                    .download_notice,
                None
            );
        },
    );
}

#[test]
#[serial]
fn pinned_and_multi_package_npx_launches_check_their_own_cache_entry() {
    let cache = TempDir::new().expect("cache dir");
    let cache_path = cache.path().to_str().expect("utf-8 path");
    with_launch_env(
        &[
            ("NORI_MANAGED_BY_NPM", "1"),
            ("npm_config_cache", cache_path),
        ],
        || {
            set_adapter_launch_overrides(adapters(&[("codex", None, Some("0.4.1"))]))
                .expect("valid overrides");
            seed_npx_cache(
                cache.path(),
                "a81fa54699a25c5d",
                "@agentclientprotocol/codex-acp",
            );
            assert!(
                get_agent_config("codex")
                    .expect("resolves")
                    .download_notice
                    .is_some(),
                "a cached @latest does not satisfy a pin"
            );
            seed_npx_cache(
                cache.path(),
                "02818b7f64428b82",
                "@agentclientprotocol/codex-acp",
            );
            assert_eq!(
                get_agent_config("codex").expect("resolves").download_notice,
                None
            );

            assert!(
                get_agent_config("pi")
                    .expect("resolves")
                    .download_notice
                    .is_some()
            );
            seed_npx_cache(cache.path(), "8c295f72a9e190be", "pi-acp");
            assert_eq!(
                get_agent_config("pi").expect("resolves").download_notice,
                None
            );
        },
    );
}

#[test]
#[serial]
fn first_bunx_fetch_carries_a_download_notice_until_the_package_is_cached() {
    let tmp = TempDir::new().expect("tmp dir");
    let tmp_path = tmp.path().to_str().expect("utf-8 path");
    with_launch_env(
        &[("NORI_MANAGED_BY_BUN", "1"), ("TMPDIR", tmp_path)],
        || {
            let notice = get_agent_config("codex")
                .expect("resolves")
                .download_notice
                .expect("uncached package announces a download");
            assert!(
                notice.contains("@agentclientprotocol/codex-acp@latest"),
                "{notice}"
            );

            let uid = unsafe { libc::getuid() };
            let dir = tmp
                .path()
                .join(format!("bunx-{uid}-@agentclientprotocol/codex-acp@latest"))
                .join("node_modules/@agentclientprotocol/codex-acp");
            std::fs::create_dir_all(&dir).expect("create bunx dir");
            std::fs::write(dir.join("package.json"), "{}").expect("write package.json");
            assert_eq!(
                get_agent_config("codex").expect("resolves").download_notice,
                None
            );
        },
    );
}

#[test]
#[serial]
fn overridden_and_path_launched_agents_carry_no_download_notice() {
    let cache = TempDir::new().expect("cache dir");
    let cache_path = cache.path().to_str().expect("utf-8 path");
    with_launch_env(
        &[
            ("NORI_MANAGED_BY_NPM", "1"),
            ("npm_config_cache", cache_path),
            ("NORI_ACP_COMMAND_CLAUDE_CODE", "npx --yes some-fork"),
        ],
        || {
            assert_eq!(
                get_agent_config("claude-code")
                    .expect("resolves")
                    .download_notice,
                None
            );
            assert_eq!(
                get_agent_config("opencode")
                    .expect("resolves")
                    .download_notice,
                None
            );
        },
    );
}

#[test]
#[serial]
fn config_toml_acp_adapters_table_drives_the_launch() {
    let home = TempDir::new().expect("config home");
    let config_path = home.path().join("config.toml");
    std::fs::write(
        &config_path,
        r#"
[acp_adapters.claude-code]
version = "0.12.3"

[acp_adapters.codex]
command = "/opt/nori/codex-acp --verbose"
"#,
    )
    .expect("write config");
    let config =
        NoriConfig::load_from_path_with_overrides(&config_path, NoriConfigOverrides::default())
            .expect("load config");

    with_launch_env(&[("NORI_MANAGED_BY_NPM", "1")], || {
        set_adapter_launch_overrides(config.acp_adapters.clone()).expect("valid overrides");
        assert_eq!(
            command_line("claude-code"),
            strings(&[
                "npx",
                "--yes",
                "--prefer-offline",
                "@agentclientprotocol/claude-agent-acp@0.12.3"
            ])
        );
        assert_eq!(
            command_line("codex"),
            strings(&["/opt/nori/codex-acp", "--verbose"])
        );
    });
}
