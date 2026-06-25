//! E2E test for the agent-sourced `/resume` session picker.
//!
//! When the live ACP agent advertises both the `session/list` and
//! `load_session` capabilities, the in-session `/resume` command sources its
//! picker rows from the agent (via `session/list`) instead of the local
//! transcript store, and resuming a row loads it over ACP via `session/load`.
//! This drives the real `nori` binary against the mock agent and verifies the
//! agent's sessions show up in the picker and can be resumed.

use std::time::Duration;
use tui_pty_e2e::Key;
use tui_pty_e2e::SessionConfig;
use tui_pty_e2e::TIMEOUT;
use tui_pty_e2e::TIMEOUT_INPUT;
use tui_pty_e2e::TuiSession;

#[test]
#[cfg(target_os = "linux")]
fn resume_picker_lists_agent_sessions_when_session_list_supported() {
    let config = SessionConfig::new()
        .with_model("mock-model".to_owned())
        .with_agent_env("MOCK_AGENT_SUPPORT_SESSION_LIST", "1")
        .with_agent_env("MOCK_AGENT_SUPPORT_LOAD_SESSION", "1");

    let mut session =
        TuiSession::spawn_with_config(30, 100, config).expect("Failed to spawn session");

    session
        .wait_for_text("›", TIMEOUT)
        .expect("TUI should start");
    // Let the session-capability projection arrive before opening the picker.
    std::thread::sleep(TIMEOUT_INPUT);

    // Dispatch the in-session `/resume` slash command.
    session.send_str("/resume").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();

    session
        .wait_for_text("Resume previous session", Duration::from_secs(5))
        .expect("Should show the agent-sourced resume picker");

    // The mock agent returns two sessions: the first carries a title, the second
    // falls back to its session id because it has none.
    session
        .wait_for_text("First mock session", Duration::from_secs(5))
        .expect("Picker should list the agent's titled session");
    session
        .wait_for_text("mock-session-2", Duration::from_secs(2))
        .expect("Picker should list the untitled session by its id");

    // Selecting the first (default-highlighted) row resumes it over ACP.
    session.send_key(Key::Enter).unwrap();
    session
        .wait_for_text("Resuming session with", Duration::from_secs(5))
        .expect("Selecting a listed session should resume it over ACP");
}
