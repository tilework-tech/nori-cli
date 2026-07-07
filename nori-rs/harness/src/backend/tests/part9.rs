//! Tests for the pre-session agent probe: `probe_agent_sessions` spawns the
//! agent, reads its advertised capabilities, fetches `session/list`, and
//! tears the child down — WITHOUT ever creating a session. This powers the
//! picker-first `nori cloud` entry, which must not claim a cloud VM until
//! the user explicitly picks "new" or an existing session.

use super::*;

use crate::probe_agent_sessions;

/// Whether the mock agent binary is available; tests skip quietly otherwise
/// (same convention as the other backend test parts).
fn mock_agent_available() -> bool {
    let mock_config =
        crate::registry::get_agent_config("mock-model").expect("mock-model should be registered");
    if std::path::Path::new(&mock_config.command).exists() {
        return true;
    }
    eprintln!(
        "Skipping test: mock_acp_agent not found at {}",
        mock_config.command
    );
    false
}

/// The probe returns the agent's capability view and its `session/list` rows
/// without creating a session. The ACP wire log is the proof: exactly one
/// `session/list` request and zero `session/new` — an implementation that
/// calls `session/new` and swallows the failure cannot pass.
#[tokio::test]
#[serial]
async fn probe_returns_capabilities_and_sessions_without_creating_one() {
    if !mock_agent_available() {
        return;
    }

    let _list_guard = EnvGuard::set("MOCK_AGENT_SUPPORT_SESSION_LIST", "1");
    let _resume_guard = EnvGuard::set("MOCK_AGENT_SUPPORT_SESSION_RESUME", "1");
    let _close_guard = EnvGuard::set("MOCK_AGENT_SUPPORT_SESSION_CLOSE", "1");
    let _fail_new_guard = EnvGuard::set("MOCK_AGENT_FAIL_NEW_SESSION_FROM", "0");
    // Hermetic against ambient dev-shell exports: the deep-equal below relies
    // on these being unset.
    let _load_guard = EnvGuard::remove("MOCK_AGENT_SUPPORT_LOAD_SESSION");
    let _http_guard = EnvGuard::remove("MOCK_AGENT_MCP_HTTP");

    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
    let wire_log_dir = temp_dir.path().join("acp-wire");
    let mut config = build_test_config(temp_dir.path());
    config.acp_proxy = crate::config::AcpProxyConfig {
        enabled: true,
        log_dir: wire_log_dir.clone(),
    };

    let probe = probe_agent_sessions(&config)
        .await
        .expect("probe should succeed without creating a session");

    assert_eq!(
        probe.capabilities,
        nori_protocol::AgentCapabilitiesView {
            http_mcp: false,
            load_session: false,
            session_list: true,
            session_resume: true,
            session_close: true,
        },
        "the probe must mirror exactly what the agent advertised"
    );

    // The mock agent reports two fixed sessions; the broker-style titled row
    // must arrive with its title intact (this is what the picker renders).
    assert_eq!(probe.sessions.len(), 2, "sessions: {:?}", probe.sessions);
    assert_eq!(probe.sessions[0].session_id, "mock-session-1");
    assert_eq!(
        probe.sessions[0].title.as_deref(),
        Some("First mock session")
    );
    assert_eq!(probe.sessions[1].session_id, "mock-session-2");
    assert_eq!(probe.sessions[1].title, None);

    // Wire-log proof: the probe listed sessions exactly once and never asked
    // for a session to be created (or loaded/resumed).
    assert_eq!(count_logged_requests(&wire_log_dir, "session/list"), 1);
    assert_eq!(count_logged_requests(&wire_log_dir, "session/new"), 0);
    assert_eq!(count_logged_requests(&wire_log_dir, "session/load"), 0);
    assert_eq!(count_logged_requests(&wire_log_dir, "session/resume"), 0);
}

/// Probing an agent that does not advertise `session/list` fails with an
/// actionable error instead of calling an unadvertised method. The caller
/// (the TUI entry flow) uses this to fall back to a plain spawn.
#[tokio::test]
#[serial]
async fn probe_fails_clearly_when_agent_lacks_session_list() {
    if !mock_agent_available() {
        return;
    }

    // Hermetic: the whole point is that the capability is absent.
    let _list_guard = EnvGuard::remove("MOCK_AGENT_SUPPORT_SESSION_LIST");

    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
    let config = build_test_config(temp_dir.path());

    let err = probe_agent_sessions(&config)
        .await
        .expect_err("probe must fail when the agent cannot list sessions");
    let message = format!("{err}");
    assert!(
        message.contains("session listing") || message.contains("session/list"),
        "the error must name the missing capability, got: {message}"
    );
}

/// The probe must tear its child down within a short bound even when the
/// child ignores stdin EOF (a hung broker release). The entry flow runs the
/// probe on every `nori cloud` boot — a stuck probe child would hang the
/// boot and leak a process per launch.
#[tokio::test]
#[serial]
async fn probe_returns_promptly_even_when_the_child_ignores_eof() {
    if !mock_agent_available() {
        return;
    }

    let _list_guard = EnvGuard::set("MOCK_AGENT_SUPPORT_SESSION_LIST", "1");
    let _resume_guard = EnvGuard::set("MOCK_AGENT_SUPPORT_SESSION_RESUME", "1");
    let _ignore_eof_guard = EnvGuard::set("MOCK_AGENT_IGNORE_EOF", "1");

    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
    let config = build_test_config(temp_dir.path());

    let started = std::time::Instant::now();
    let probe = probe_agent_sessions(&config)
        .await
        .expect("probe should succeed even if the child ignores EOF");
    let elapsed = started.elapsed();

    assert_eq!(probe.sessions.len(), 2);
    assert!(
        elapsed < std::time::Duration::from_secs(10),
        "probe must not wait out an uncooperative child (took {elapsed:?})"
    );
}
