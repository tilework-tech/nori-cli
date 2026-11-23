#![allow(clippy::unwrap_used)]

mod common;

use std::path::PathBuf;

use common::Fixture;
use nori_acp_host::registry::AgentKind;
use nori_auth_providers::AuthStatus;
use nori_auth_providers::CredentialKind;
use nori_auth_providers::CredentialSource;
use nori_auth_providers::DetectContext;
use nori_auth_providers::DetectedCredential;
use nori_auth_providers::Notice;
use nori_auth_providers::Platform;
use nori_auth_providers::Provider;
use nori_auth_providers::StoreBackend;
use nori_auth_providers::detect_all;
use pretty_assertions::assert_eq;

const CLAUDE_OAUTH: &str = r#"{"claudeAiOauth":{"accessToken":"sk-ant-oat01-SECRET","refreshToken":"sk-ant-ort01-SECRET","expiresAt":1999999999000,"scopes":["user:inference"],"subscriptionType":"max"}}"#;

/// The kind, source, and detail of a detected credential.
type Cred = (CredentialKind, CredentialSource, Option<String>);

fn cred(kind: CredentialKind, source: CredentialSource, detail: Option<&str>) -> Cred {
    (kind, source, detail.map(str::to_string))
}

fn saved(provider: &str, env: &str) -> CredentialSource {
    CredentialSource::NoriSaved {
        provider: provider.to_string(),
        env: env.to_string(),
        store: StoreBackend::File,
    }
}

fn env(name: &str) -> CredentialSource {
    CredentialSource::Env {
        name: name.to_string(),
    }
}

fn file(path: PathBuf) -> CredentialSource {
    CredentialSource::File { path }
}

fn keychain(service: &str) -> CredentialSource {
    CredentialSource::Keychain {
        service: service.to_string(),
    }
}

fn outcome(f: &Fixture, agent: AgentKind) -> (AuthStatus, Vec<Cred>) {
    let status = f.detect(agent);
    let credentials = status
        .credentials
        .into_iter()
        .map(|c: DetectedCredential| (c.kind, c.source, c.detail))
        .collect();
    (status.status, credentials)
}

#[test]
fn claude_subscription_file_reports_plan_without_tokens() {
    let f = Fixture::new().file(".claude/.credentials.json", CLAUDE_OAUTH);
    assert_eq!(
        outcome(&f, AgentKind::ClaudeCode),
        (
            AuthStatus::SignedIn,
            vec![cred(
                CredentialKind::Subscription,
                file(f.home().join(".claude/.credentials.json")),
                Some("max"),
            )]
        )
    );
}

#[test]
fn claude_lists_sources_in_the_order_claude_code_prefers_them() {
    let f = Fixture::new()
        .file(".claude/.credentials.json", CLAUDE_OAUTH)
        .env("CLAUDE_CODE_OAUTH_TOKEN", "SECRET-value")
        .env("ANTHROPIC_API_KEY", "SECRET-value")
        .env("ANTHROPIC_AUTH_TOKEN", "SECRET-value")
        .env("CLAUDE_CODE_USE_BEDROCK", "1");
    assert_eq!(
        outcome(&f, AgentKind::ClaudeCode),
        (
            AuthStatus::SignedIn,
            vec![
                cred(CredentialKind::Cloud, env("CLAUDE_CODE_USE_BEDROCK"), None),
                cred(CredentialKind::Token, env("ANTHROPIC_AUTH_TOKEN"), None),
                cred(CredentialKind::ApiKey, env("ANTHROPIC_API_KEY"), None),
                cred(
                    CredentialKind::Subscription,
                    env("CLAUDE_CODE_OAUTH_TOKEN"),
                    None
                ),
                cred(
                    CredentialKind::Subscription,
                    file(f.home().join(".claude/.credentials.json")),
                    Some("max"),
                ),
            ]
        )
    );
}

#[test]
fn claude_on_macos_finds_the_keychain_login() {
    let f = Fixture::new()
        .on(Platform::MacOs)
        .keychain_item("Claude Code-credentials");
    assert_eq!(
        outcome(&f, AgentKind::ClaudeCode),
        (
            AuthStatus::SignedIn,
            vec![cred(
                CredentialKind::Subscription,
                keychain("Claude Code-credentials"),
                None
            )]
        )
    );
}

#[test]
fn claude_keychain_is_not_consulted_on_linux() {
    let f = Fixture::new().keychain_item("Claude Code-credentials");
    assert_eq!(
        outcome(&f, AgentKind::ClaudeCode),
        (AuthStatus::SignedOut, vec![])
    );
}

#[test]
fn claude_config_dir_moves_the_credentials_file_and_keychain_item() {
    let f = Fixture::new()
        .on(Platform::MacOs)
        .env("CLAUDE_CONFIG_DIR", "/opt/claude-cfg")
        .keychain_item("Claude Code-credentials")
        // Claude Code suffixes the item with the first 8 hex digits of sha256("/opt/claude-cfg").
        .keychain_item("Claude Code-credentials-9751abf6")
        .file(".claude/.credentials.json", CLAUDE_OAUTH);
    assert_eq!(
        outcome(&f, AgentKind::ClaudeCode),
        (
            AuthStatus::SignedIn,
            vec![cred(
                CredentialKind::Subscription,
                keychain("Claude Code-credentials-9751abf6"),
                None
            )]
        )
    );
}

#[test]
fn claude_config_dir_credentials_file_is_found() {
    let f = Fixture::new().file("cfg/.credentials.json", CLAUDE_OAUTH);
    let dir = f.home().join("cfg");
    let f = f.env("CLAUDE_CONFIG_DIR", dir.to_str().unwrap());
    assert_eq!(
        outcome(&f, AgentKind::ClaudeCode),
        (
            AuthStatus::SignedIn,
            vec![cred(
                CredentialKind::Subscription,
                file(dir.join(".credentials.json")),
                Some("max")
            )]
        )
    );
}

#[test]
fn claude_console_api_key_in_global_config_counts() {
    let f = Fixture::new().file(
        ".claude.json",
        r#"{"primaryApiKey":"sk-ant-api03-SECRET","oauthAccount":null}"#,
    );
    assert_eq!(
        outcome(&f, AgentKind::ClaudeCode),
        (
            AuthStatus::SignedIn,
            vec![cred(
                CredentialKind::ApiKey,
                file(f.home().join(".claude.json")),
                None
            )]
        )
    );
}

#[test]
fn codex_chatgpt_login_is_a_subscription() {
    let f = Fixture::new().file(
        ".codex/auth.json",
        r#"{"auth_mode":"chatgpt","OPENAI_API_KEY":null,"tokens":{"id_token":"SECRET","access_token":"SECRET","refresh_token":"SECRET"}}"#,
    );
    assert_eq!(
        outcome(&f, AgentKind::Codex),
        (
            AuthStatus::SignedIn,
            vec![cred(
                CredentialKind::Subscription,
                file(f.home().join(".codex/auth.json")),
                Some("chatgpt")
            )]
        )
    );
}

#[test]
fn codex_auth_file_with_an_api_key_is_an_api_key() {
    let f = Fixture::new().file(
        ".codex/auth.json",
        r#"{"auth_mode":"apikey","OPENAI_API_KEY":"sk-SECRET"}"#,
    );
    assert_eq!(
        outcome(&f, AgentKind::Codex),
        (
            AuthStatus::SignedIn,
            vec![cred(
                CredentialKind::ApiKey,
                file(f.home().join(".codex/auth.json")),
                None
            )]
        )
    );
}

#[test]
fn codex_home_is_ignored_because_nori_launches_codex_without_it() {
    let f = Fixture::new().file("alt-codex/auth.json", r#"{"auth_mode":"chatgpt"}"#);
    let dir = f.home().join("alt-codex");
    let f = f.env("CODEX_HOME", dir.to_str().unwrap());
    assert_eq!(
        outcome(&f, AgentKind::Codex),
        (AuthStatus::SignedOut, vec![])
    );
}

#[test]
fn codex_env_keys_come_before_its_auth_file() {
    let f = Fixture::new()
        .env("CODEX_API_KEY", "SECRET-value")
        .env("OPENAI_API_KEY", "SECRET-value")
        .file(
            ".codex/auth.json",
            r#"{"auth_mode":"chatgpt","tokens":{"access_token":"SECRET"}}"#,
        );
    assert_eq!(
        outcome(&f, AgentKind::Codex),
        (
            AuthStatus::SignedIn,
            vec![
                cred(CredentialKind::ApiKey, env("CODEX_API_KEY"), None),
                cred(CredentialKind::ApiKey, env("OPENAI_API_KEY"), None),
                cred(
                    CredentialKind::Subscription,
                    file(f.home().join(".codex/auth.json")),
                    Some("chatgpt")
                ),
            ]
        )
    );
}

#[test]
fn codex_keyring_login_is_unknown_on_linux() {
    let f = Fixture::new().file(
        ".codex/config.toml",
        "cli_auth_credentials_store = \"keyring\"\n",
    );
    assert_eq!(outcome(&f, AgentKind::Codex), (AuthStatus::Unknown, vec![]));
}

#[test]
fn codex_keyring_login_is_found_on_macos() {
    let f = Fixture::new()
        .on(Platform::MacOs)
        .keychain_item("Codex Auth")
        .file(
            ".codex/config.toml",
            "cli_auth_credentials_store = \"auto\"\n",
        );
    assert_eq!(
        outcome(&f, AgentKind::Codex),
        (
            AuthStatus::SignedIn,
            vec![cred(CredentialKind::Login, keychain("Codex Auth"), None)]
        )
    );
}

#[test]
fn codex_default_store_ignores_a_keychain_item_on_macos() {
    let f = Fixture::new()
        .on(Platform::MacOs)
        .keychain_item("Codex Auth");
    assert_eq!(
        outcome(&f, AgentKind::Codex),
        (AuthStatus::SignedOut, vec![])
    );
}

#[test]
fn key_saved_through_nori_is_reported_after_ambient_env() {
    let f = Fixture::new()
        .prefer_api_key(&[AgentKind::ClaudeCode])
        .save_key(Provider::Anthropic, "sk-ant-api03-SECRET")
        .env("ANTHROPIC_API_KEY", "SECRET-value");
    assert_eq!(
        outcome(&f, AgentKind::ClaudeCode),
        (
            AuthStatus::SignedIn,
            vec![
                cred(CredentialKind::ApiKey, env("ANTHROPIC_API_KEY"), None),
                cred(
                    CredentialKind::ApiKey,
                    saved("anthropic", "ANTHROPIC_API_KEY"),
                    None
                ),
            ]
        )
    );
}

#[test]
fn saved_claude_key_outranks_the_subscription_login() {
    let f = Fixture::new()
        .file(".claude/.credentials.json", CLAUDE_OAUTH)
        .prefer_api_key(&[AgentKind::ClaudeCode])
        .save_key(Provider::Anthropic, "sk-ant-api03-SECRET");
    assert_eq!(
        outcome(&f, AgentKind::ClaudeCode),
        (
            AuthStatus::SignedIn,
            vec![
                cred(
                    CredentialKind::ApiKey,
                    saved("anthropic", "ANTHROPIC_API_KEY"),
                    None
                ),
                cred(
                    CredentialKind::Subscription,
                    file(f.home().join(".claude/.credentials.json")),
                    Some("max")
                ),
            ]
        )
    );
}

#[test]
fn saved_key_alone_signs_an_opaque_agent_in() {
    let f = Fixture::new()
        .prefer_api_key(&[AgentKind::Cursor])
        .save_key(Provider::Cursor, "SECRET-cursor");
    assert_eq!(
        outcome(&f, AgentKind::Cursor),
        (
            AuthStatus::SignedIn,
            vec![cred(
                CredentialKind::ApiKey,
                saved("cursor", "CURSOR_API_KEY"),
                None
            )]
        )
    );
}

#[test]
fn values_that_do_not_count_as_credentials() {
    let f = Fixture::new()
        .env("ANTHROPIC_API_KEY", "")
        .env("CLAUDE_CODE_USE_VERTEX", "0")
        .env("OPENAI_API_KEY", " ")
        .env("GOOGLE_GENAI_USE_VERTEXAI", "false")
        .env("XAI_API_KEY", "")
        .file(
            ".claude/.credentials.json",
            r#"{"note":"no claudeAiOauth entry"}"#,
        );
    assert_eq!(
        [
            AgentKind::ClaudeCode,
            AgentKind::Codex,
            AgentKind::Gemini,
            AgentKind::GrokBuild
        ]
        .map(|agent| (agent.slug(), outcome(&f, agent))),
        [
            ("claude-code", (AuthStatus::SignedOut, vec![])),
            ("codex", (AuthStatus::SignedOut, vec![])),
            ("gemini", (AuthStatus::Unknown, vec![])),
            ("grok-build", (AuthStatus::Unknown, vec![])),
        ]
    );
}

#[test]
fn malformed_credential_files_are_ignored() {
    let f = Fixture::new()
        .file(".claude/.credentials.json", "{not json SECRET")
        .file(".claude.json", "[]")
        .file(".codex/auth.json", "SECRET")
        .file(".local/share/opencode/auth.json", "\"SECRET\"")
        .file(".pi/agent/auth.json", "[\"SECRET\"]");
    assert_eq!(
        [AgentKind::ClaudeCode, AgentKind::Codex, AgentKind::OpenCode]
            .map(|agent| outcome(&f, agent)),
        [
            (AuthStatus::SignedOut, vec![]),
            (AuthStatus::SignedOut, vec![]),
            (AuthStatus::Unknown, vec![]),
        ]
    );
}

#[test]
fn opencode_reports_each_signed_in_provider() {
    let f = Fixture::new().file(
        ".local/share/opencode/auth.json",
        r#"{"openai":{"type":"api","key":"sk-SECRET"},"anthropic":{"type":"oauth","refresh":"SECRET","access":"SECRET","expires":1}}"#,
    );
    let path = f.home().join(".local/share/opencode/auth.json");
    assert_eq!(
        outcome(&f, AgentKind::OpenCode),
        (
            AuthStatus::SignedIn,
            vec![
                cred(CredentialKind::Subscription, file(path.clone()), None),
                cred(CredentialKind::ApiKey, file(path), None),
            ]
        )
    );
}

#[test]
fn opencode_follows_xdg_data_home() {
    let f = Fixture::new().file(
        "data/opencode/auth.json",
        r#"{"openai":{"type":"api","key":"SECRET"}}"#,
    );
    let data = f.home().join("data");
    let f = f.env("XDG_DATA_HOME", data.to_str().unwrap());
    assert_eq!(
        outcome(&f, AgentKind::OpenCode),
        (
            AuthStatus::SignedIn,
            vec![cred(
                CredentialKind::ApiKey,
                file(data.join("opencode/auth.json")),
                None
            )]
        )
    );
}

#[test]
fn pi_reports_each_signed_in_provider() {
    let f = Fixture::new().file(
        ".pi/agent/auth.json",
        r#"{"anthropic":{"type":"oauth","access":"SECRET"},"openai":{"type":"api_key","key":"SECRET"}}"#,
    );
    let path = f.home().join(".pi/agent/auth.json");
    assert_eq!(
        outcome(&f, AgentKind::Pi),
        (
            AuthStatus::SignedIn,
            vec![
                cred(CredentialKind::Subscription, file(path.clone()), None),
                cred(CredentialKind::ApiKey, file(path), None),
            ]
        )
    );
}

#[test]
fn agents_with_their_own_opaque_login_are_unknown_without_env() {
    let f = Fixture::new();
    let opaque: Vec<_> = AgentKind::all()
        .iter()
        .filter(|agent| !matches!(agent, AgentKind::ClaudeCode | AgentKind::Codex))
        .map(|agent| (agent.slug(), outcome(&f, *agent)))
        .collect();
    let expected: Vec<_> = opaque
        .iter()
        .map(|(slug, _)| (*slug, (AuthStatus::Unknown, vec![])))
        .collect();
    assert_eq!(opaque, expected);
}

#[test]
fn saved_key_is_listed_after_every_environment_credential() {
    let f = Fixture::new()
        .prefer_api_key(&[AgentKind::Gemini])
        .save_key(Provider::Google, "SECRET-saved")
        .env("GOOGLE_API_KEY", "SECRET-value");
    assert_eq!(
        outcome(&f, AgentKind::Gemini),
        (
            AuthStatus::SignedIn,
            vec![
                cred(CredentialKind::ApiKey, env("GOOGLE_API_KEY"), None),
                cred(
                    CredentialKind::ApiKey,
                    saved("google", "GEMINI_API_KEY"),
                    None
                ),
            ]
        )
    );
}

#[test]
fn env_keys_sign_in_the_other_agents() {
    let f = Fixture::new()
        .env("GEMINI_API_KEY", "SECRET-value")
        .env("XAI_API_KEY", "SECRET-value")
        .env("CURSOR_API_KEY", "SECRET-value");
    assert_eq!(
        [AgentKind::Gemini, AgentKind::GrokBuild, AgentKind::Cursor]
            .map(|agent| outcome(&f, agent)),
        [
            (
                AuthStatus::SignedIn,
                vec![cred(CredentialKind::ApiKey, env("GEMINI_API_KEY"), None)]
            ),
            (
                AuthStatus::SignedIn,
                vec![cred(CredentialKind::ApiKey, env("XAI_API_KEY"), None)]
            ),
            (
                AuthStatus::SignedIn,
                vec![cred(CredentialKind::ApiKey, env("CURSOR_API_KEY"), None)]
            ),
        ]
    );
}

#[test]
fn antigravity_cloud_routing_flags_sign_in() {
    let f = Fixture::new()
        .env("GOOGLE_GENAI_USE_VERTEXAI", "true")
        .env("AGY_ADC_AUTH", "true");
    assert_eq!(
        outcome(&f, AgentKind::Gemini),
        (
            AuthStatus::SignedIn,
            vec![
                cred(
                    CredentialKind::Cloud,
                    env("GOOGLE_GENAI_USE_VERTEXAI"),
                    None
                ),
                cred(CredentialKind::Cloud, env("AGY_ADC_AUTH"), None),
            ]
        )
    );
}

#[test]
fn report_covers_every_built_in_agent() {
    let f = Fixture::new();
    let home = f.home();
    let env = |_: &str| None;
    let report = detect_all(&DetectContext {
        home: &home,
        env: &env,
        platform: f.platform,
        keychain: &f.keychain,
        saved: &f.saved,
        prefer_api_key: &f.prefers(),
    });
    let slugs: Vec<_> = report.agents.iter().map(|a| a.agent.as_str()).collect();
    let expected: Vec<_> = AgentKind::all().iter().map(AgentKind::slug).collect();
    assert_eq!(slugs, expected);
}

#[test]
fn pi_credentials_name_their_provider() {
    let f = Fixture::new()
        .file(
            ".pi/agent/auth.json",
            r#"{"anthropic":{"type":"oauth","access":"SECRET"}}"#,
        )
        .env("XAI_API_KEY", "SECRET-value")
        .prefer_api_key(&[AgentKind::Pi])
        .save_key(Provider::OpenAi, "SECRET-saved");
    let providers: Vec<_> = f
        .detect(AgentKind::Pi)
        .credentials
        .into_iter()
        .map(|c| (c.provider, c.source))
        .collect();
    assert_eq!(
        providers,
        vec![
            (
                Some("anthropic".to_string()),
                file(f.home().join(".pi/agent/auth.json"))
            ),
            (Some("xai".to_string()), env("XAI_API_KEY")),
            (
                Some("openai".to_string()),
                saved("openai", "OPENAI_API_KEY")
            ),
        ]
    );
}

#[test]
fn one_saved_provider_key_signs_in_every_agent_that_prefers_api_keys() {
    let f = Fixture::new()
        .prefer_api_key(&[AgentKind::Codex, AgentKind::Pi, AgentKind::ClaudeCode])
        .save_key(Provider::OpenAi, "SECRET-saved");
    let signed_in: Vec<_> = AgentKind::all()
        .iter()
        .filter(|agent| f.detect(**agent).status == AuthStatus::SignedIn)
        .map(AgentKind::slug)
        .collect();
    assert_eq!(signed_in, vec!["codex", "pi"]);
}

#[test]
fn agents_keep_their_own_login_and_are_told_about_an_unused_saved_key() {
    let f = Fixture::new()
        .file(".claude/.credentials.json", CLAUDE_OAUTH)
        .save_key(Provider::Anthropic, "SECRET-saved")
        .save_key(Provider::OpenAi, "SECRET-saved");
    let claude = f.detect(AgentKind::ClaudeCode);
    let codex = f.detect(AgentKind::Codex);
    assert_eq!(
        [&claude, &codex].map(|agent| (
            agent.status,
            agent.prefer_api_key,
            agent.credentials.iter().map(|c| c.kind).collect::<Vec<_>>(),
            agent.notices.clone()
        )),
        [
            (
                AuthStatus::SignedIn,
                false,
                vec![CredentialKind::Subscription],
                vec![Notice {
                    code: "saved_key_not_used",
                    message: "Claude Code uses its own login, not the saved anthropic API key; \
                              run `nori auth prefer-api-key claude-code` to use the key"
                        .to_string()
                }]
            ),
            (
                AuthStatus::SignedOut,
                false,
                vec![],
                vec![Notice {
                    code: "saved_key_not_used",
                    message: "Codex uses its own login, not the saved openai API key; \
                              run `nori auth prefer-api-key codex` to use the key"
                        .to_string()
                }]
            ),
        ]
    );
}

#[test]
fn preferring_api_keys_over_a_subscription_comes_with_a_billing_warning() {
    let subscribed = Fixture::new().file(".claude/.credentials.json", CLAUDE_OAUTH);
    let codex_subscribed = Fixture::new().file(
        ".codex/auth.json",
        r#"{"auth_mode":"chatgpt","tokens":{"access_token":"SECRET"}}"#,
    );
    let pi_subscribed = Fixture::new().file(
        ".pi/agent/auth.json",
        r#"{"anthropic":{"type":"oauth","access":"SECRET"}}"#,
    );
    let unsubscribed = Fixture::new().env("ANTHROPIC_API_KEY", "SECRET-value");
    let codex_api_key_login = Fixture::new().file(
        ".codex/auth.json",
        r#"{"auth_mode":"apikey","OPENAI_API_KEY":"SECRET"}"#,
    );

    let warnings = [
        subscribed.detect(AgentKind::ClaudeCode),
        codex_subscribed.detect(AgentKind::Codex),
        pi_subscribed.detect(AgentKind::Pi),
        unsubscribed.detect(AgentKind::ClaudeCode),
        codex_api_key_login.detect(AgentKind::Codex),
    ]
    .map(|status| status.api_key_billing_warning());

    assert_eq!(
        warnings,
        [
            Some(Notice {
                code: "api_key_overrides_subscription",
                message: "Claude Code is signed in with a subscription (max). Preferring the \
                          saved API key bills usage to that API account, not the subscription."
                    .to_string()
            }),
            Some(Notice {
                code: "api_key_overrides_subscription",
                message: "Codex is signed in with a subscription (chatgpt). Preferring the \
                          saved API key bills usage to that API account, not the subscription."
                    .to_string()
            }),
            // Pi keeps using its own login for a provider even when given a key.
            None,
            None,
            None,
        ]
    );
}

#[test]
fn no_unused_key_notice_without_a_saved_key_or_once_the_agent_prefers_it() {
    let nothing_saved = Fixture::new().file(".claude/.credentials.json", CLAUDE_OAUTH);
    let preferred = Fixture::new()
        .prefer_api_key(&[AgentKind::Codex])
        .save_key(Provider::OpenAi, "SECRET-saved");
    assert_eq!(
        [
            nothing_saved.detect(AgentKind::ClaudeCode).notices,
            preferred.detect(AgentKind::Codex).notices,
        ],
        [vec![], vec![]]
    );
}

#[test]
fn only_keys_saved_through_nori_can_be_disconnected_by_nori() {
    let f = Fixture::new()
        .env("ANTHROPIC_API_KEY", "SECRET-value")
        .file(".claude/.credentials.json", CLAUDE_OAUTH)
        .prefer_api_key(&[AgentKind::ClaudeCode])
        .save_key(Provider::Anthropic, "SECRET-saved");
    let rows: Vec<_> = f
        .detect(AgentKind::ClaudeCode)
        .credentials
        .into_iter()
        .map(|c| (c.read_only, c.disconnect))
        .collect();
    assert_eq!(
        rows,
        vec![
            (true, "unset ANTHROPIC_API_KEY to disconnect".to_string()),
            (
                false,
                "run `nori auth remove-key anthropic` to disconnect".to_string()
            ),
            (true, "sign out in Claude Code to disconnect".to_string()),
        ]
    );
}

#[test]
fn api_key_ahead_of_a_subscription_raises_a_billing_notice() {
    let subscription_only = Fixture::new().file(".claude/.credentials.json", CLAUDE_OAUTH);
    let saved_key_too = Fixture::new()
        .file(".claude/.credentials.json", CLAUDE_OAUTH)
        .prefer_api_key(&[AgentKind::ClaudeCode])
        .save_key(Provider::Anthropic, "SECRET-saved");

    assert_eq!(
        subscription_only.detect(AgentKind::ClaudeCode).notices,
        vec![]
    );
    let notices = saved_key_too.detect(AgentKind::ClaudeCode).notices;
    assert_eq!(
        notices
            .iter()
            .map(|notice: &Notice| notice.code)
            .collect::<Vec<_>>(),
        vec!["api_key_overrides_subscription"]
    );
    assert!(notices[0].message.contains("max"), "{}", notices[0].message);
}

#[test]
fn report_lists_each_provider_with_its_saved_record_and_agents() {
    let f = Fixture::new()
        .prefer_api_key(&[AgentKind::Gemini])
        .save_key(Provider::Google, "SECRET-saved");
    let home = f.home();
    let env = |_: &str| None;
    let report = detect_all(&DetectContext {
        home: &home,
        env: &env,
        platform: f.platform,
        keychain: &f.keychain,
        saved: &f.saved,
        prefer_api_key: &f.prefers(),
    });
    let json = serde_json::to_value(&report).unwrap();
    assert_eq!(json["store"], "file");
    let google = json["providers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|provider| provider["id"] == "google")
        .cloned()
        .unwrap();
    assert_eq!(
        google,
        serde_json::json!({
            "id": "google",
            "name": "Google Gemini",
            "saved": {"type": "api_key"},
            "agents": [
                {"agent": "gemini", "env": "GEMINI_API_KEY"},
                {"agent": "opencode", "env": "GOOGLE_GENERATIVE_AI_API_KEY"},
                {"agent": "pi", "env": "GEMINI_API_KEY"},
            ]
        })
    );
    let ids: Vec<_> = json["providers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|provider| provider["id"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(ids, ["anthropic", "openai", "google", "xai", "cursor"]);
}
