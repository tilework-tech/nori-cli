//! `nori auth` prompts driven through a real terminal: the API key is masked
//! while it is typed or pasted, and preferring an API key over a subscription
//! asks for confirmation.

use std::path::Path;

use tui_pty_e2e::Key;
use tui_pty_e2e::SessionConfig;
use tui_pty_e2e::TIMEOUT;
use tui_pty_e2e::TuiSession;

fn auth_session(home: &Path, args: &[&str]) -> TuiSession {
    let config = SessionConfig::new()
        .with_command_args(std::iter::once("auth").chain(args.iter().copied()))
        .with_nori_home(home.join("nori"))
        .with_agent_env("HOME", home.to_string_lossy())
        .with_agent_env("NORI_NO_ANALYTICS", "1");
    TuiSession::spawn_with_config(24, 100, config).unwrap()
}

#[test]
fn typed_and_pasted_key_shows_only_its_known_prefix() {
    let home = tempfile::tempdir().unwrap();
    let mut session = auth_session(home.path(), &["set-key", "openai"]);
    session.wait_for_text("OpenAI API key", TIMEOUT).unwrap();

    session.send_str("sk-proj-SEC").unwrap();
    session.send_str("\x1b[200~RETpasted\x1b[201~").unwrap();
    session
        .wait_for_text("sk-proj-************", TIMEOUT)
        .unwrap();
    session.send_key(Key::Enter).unwrap();
    session
        .wait_for_text("Saved the OpenAI API key", TIMEOUT)
        .unwrap();
    assert!(session.wait_for_process_exit(TIMEOUT));

    let stored = std::fs::read_to_string(home.path().join("nori/credentials.json")).unwrap();
    assert!(stored.contains("sk-proj-SECRETpasted"), "{stored}");
    let shown = String::from_utf8_lossy(session.raw_output()).to_string();
    assert!(!shown.contains("SEC"), "key echoed: {shown}");
}

#[test]
fn preferring_an_api_key_over_a_subscription_asks_first() {
    let home = tempfile::tempdir().unwrap();
    let auth = home.path().join(".claude/.credentials.json");
    std::fs::create_dir_all(auth.parent().unwrap()).unwrap();
    std::fs::write(
        &auth,
        r#"{"claudeAiOauth":{"accessToken":"x","subscriptionType":"max"}}"#,
    )
    .unwrap();

    let mut declined = auth_session(home.path(), &["prefer-api-key", "claude-code"]);
    declined
        .wait_for_text("subscription (max)", TIMEOUT)
        .unwrap();
    declined.send_str("no").unwrap();
    declined.send_key(Key::Enter).unwrap();
    declined
        .wait_for_text("Kept Claude Code on its own login", TIMEOUT)
        .unwrap();
    assert!(declined.wait_for_process_exit(TIMEOUT));
    let after_decline =
        std::fs::read_to_string(home.path().join("nori/config.toml")).unwrap_or_default();

    let mut accepted = auth_session(home.path(), &["prefer-api-key", "claude-code"]);
    accepted
        .wait_for_text("subscription (max)", TIMEOUT)
        .unwrap();
    accepted.send_str("yes").unwrap();
    accepted.send_key(Key::Enter).unwrap();
    accepted
        .wait_for_text("prefers the saved API key", TIMEOUT)
        .unwrap();
    assert!(accepted.wait_for_process_exit(TIMEOUT));
    let after_accept = std::fs::read_to_string(home.path().join("nori/config.toml")).unwrap();

    assert!(
        !after_decline.contains("prefer_api_key")
            && !after_decline.contains("api_billing_confirmed"),
        "{after_decline}"
    );
    assert!(
        after_accept.contains("prefer_api_key = true")
            && after_accept.contains("api_billing_confirmed = true"),
        "{after_accept}"
    );
}
