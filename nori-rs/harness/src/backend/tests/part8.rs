//! Session-lifecycle tests for agents that advertise the ACP
//! `sessionCapabilities.{resume,close}` methods with `loadSession: false` —
//! the nori cloud contract (`nori-handroll acp --type cloud`). Reattach must
//! go over `session/resume` (never `session/load`, never a fresh
//! `session/new`), and explicit cleanup goes over `session/close`.

use super::*;

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

/// When the agent advertises `session/resume` but not `session/load`,
/// resume must reattach via `session/resume` — not silently create a fresh
/// session. `MOCK_AGENT_FAIL_NEW_SESSION_FROM=0` makes any `session/new`
/// fallback fail loudly, so success proves the resume path was taken.
#[tokio::test]
#[serial]
async fn resume_uses_session_resume_when_agent_cannot_load() {
    use std::time::Duration;

    if !mock_agent_available() {
        return;
    }

    let _resume_guard = EnvGuard::set("MOCK_AGENT_SUPPORT_SESSION_RESUME", "1");
    let _fail_new_guard = EnvGuard::set("MOCK_AGENT_FAIL_NEW_SESSION_FROM", "0");

    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
    let (backend_event_tx, mut backend_event_rx) = mpsc::channel(64);
    let config = build_test_config(temp_dir.path());

    let result =
        AcpBackend::resume_session(&config, Some("acp-session-42"), None, backend_event_tx).await;

    let _backend = result.expect(
        "resume_session should reattach via session/resume instead of creating a new session",
    );

    let event = recv_backend_control(&mut backend_event_rx, Duration::from_secs(5))
        .await
        .expect("Should receive SessionConfigured within timeout");
    match event.msg {
        EventMsg::SessionConfigured(_) => {}
        other => panic!(
            "Expected SessionConfigured event, got: {:?}",
            std::mem::discriminant(&other)
        ),
    }
}

/// A failed `session/resume` must propagate a clear user-facing error — not
/// silently fall back to `session/new`, which on a cloud agent would claim a
/// brand-new VM the user never asked for.
#[tokio::test]
#[serial]
async fn resume_failure_propagates_a_clear_error_without_new_session_fallback() {
    if !mock_agent_available() {
        return;
    }

    let _resume_guard = EnvGuard::set("MOCK_AGENT_SUPPORT_SESSION_RESUME", "1");
    let _fail_guard = EnvGuard::set("MOCK_AGENT_RESUME_SESSION_FAIL", "1");
    let _fail_new_guard = EnvGuard::set("MOCK_AGENT_FAIL_NEW_SESSION_FROM", "0");

    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
    let (backend_event_tx, _backend_event_rx) = mpsc::channel(64);
    let config = build_test_config(temp_dir.path());

    let result =
        AcpBackend::resume_session(&config, Some("gone-session"), None, backend_event_tx).await;

    let err = result
        .err()
        .expect("resume_session must propagate a session/resume failure");
    let message = format!("{err}");
    assert!(
        message.contains("no longer exists"),
        "the error must explain the session is gone in user terms, got: {message}"
    );
    assert!(
        message.contains("the session is no longer claimed"),
        "the agent-supplied error.data.detail must reach the user verbatim, got: {message}"
    );
    assert!(
        !message.contains("new_session"),
        "no session/new fallback may run on a resume failure, got: {message}"
    );
}

/// A failed `session/close` propagates through `close_active_session` with
/// the enhanced structured-code-aware message — proving the request crosses
/// the process boundary (a no-op close could not observe the agent-side
/// error) and that close failures get the same message treatment as
/// spawn/resume failures.
#[tokio::test]
#[serial]
async fn close_active_session_failure_carries_the_enhanced_message() {
    use std::time::Duration;

    if !mock_agent_available() {
        return;
    }

    let _close_guard = EnvGuard::set("MOCK_AGENT_SUPPORT_SESSION_CLOSE", "1");
    let _fail_guard = EnvGuard::set("MOCK_AGENT_CLOSE_SESSION_FAIL", "1");

    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
    let (backend_event_tx, mut backend_event_rx) = mpsc::channel(64);
    let config = build_test_config(temp_dir.path());

    let backend = AcpBackend::spawn(&config, backend_event_tx)
        .await
        .expect("Failed to spawn ACP backend");

    // Wait for the session to be fully configured before closing it.
    let event = recv_backend_control(&mut backend_event_rx, Duration::from_secs(5))
        .await
        .expect("Should receive SessionConfigured within timeout");
    match event.msg {
        EventMsg::SessionConfigured(_) => {}
        other => panic!(
            "Expected SessionConfigured event, got: {:?}",
            std::mem::discriminant(&other)
        ),
    }

    let err = backend
        .close_active_session()
        .await
        .expect_err("close_active_session must propagate the agent's failure");
    let message = format!("{err}");
    assert!(
        message.contains("no longer exists"),
        "close failures must carry the enhanced session-not-found message, got: {message}"
    );
}

/// The capabilities view projected to frontends must expose the agent's
/// `sessionCapabilities.{resume,close}` so the TUI can gate the cloud resume
/// picker and the /close command.
#[tokio::test]
#[serial]
async fn capabilities_view_exposes_session_resume_and_close() {
    use std::time::Duration;

    if !mock_agent_available() {
        return;
    }

    let _resume_guard = EnvGuard::set("MOCK_AGENT_SUPPORT_SESSION_RESUME", "1");
    let _close_guard = EnvGuard::set("MOCK_AGENT_SUPPORT_SESSION_CLOSE", "1");

    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
    let (backend_event_tx, mut backend_event_rx) = mpsc::channel(64);
    let config = build_test_config(temp_dir.path());

    let _backend = AcpBackend::spawn(&config, backend_event_tx)
        .await
        .expect("Failed to spawn ACP backend");

    let update = loop {
        match recv_backend_client(&mut backend_event_rx, Duration::from_secs(5)).await {
            Some(nori_protocol::ClientEvent::SessionCapabilitiesChanged(update)) => break update,
            Some(_) => {}
            None => panic!("Timed out waiting for SessionCapabilitiesChanged"),
        }
    };

    assert_eq!(
        update.agent,
        nori_protocol::AgentCapabilitiesView {
            http_mcp: false,
            load_session: false,
            session_list: false,
            session_resume: true,
            session_close: true,
        },
        "the view must mirror exactly what the agent advertised"
    );
}
