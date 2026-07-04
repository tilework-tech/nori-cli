//! Native compact and session branching (ACP `session/fork`) behavior.
//!
//! These tests drive the backend against the mock ACP agent and assert only
//! wire-observable behavior: the prompt text the agent receives (via
//! `MOCK_AGENT_ECHO_PROMPT`), the session a prompt targets (via
//! `MOCK_AGENT_ECHO_SESSION_ID`), and the client events the backend emits.

use super::*;
use pretty_assertions::assert_eq;

/// Everything observable from one prompt turn: the streamed answer text and
/// any `ContextCompacted` events, terminated by `PromptCompleted`.
struct TurnCapture {
    answer_text: String,
    context_compacted: Vec<nori_protocol::ContextCompacted>,
    prompt_completed: nori_protocol::PromptCompleted,
}

/// Collect client events until the turn's `PromptCompleted` arrives.
async fn capture_turn(
    client_event_rx: &mut mpsc::Receiver<nori_protocol::ClientEvent>,
) -> TurnCapture {
    use std::time::Duration;

    let mut answer_text = String::new();
    let mut context_compacted = Vec::new();
    let deadline = std::time::Instant::now() + Duration::from_secs(10);

    while std::time::Instant::now() < deadline {
        let Ok(event) = tokio::time::timeout(Duration::from_secs(1), client_event_rx.recv()).await
        else {
            continue;
        };
        match event {
            Some(nori_protocol::ClientEvent::MessageDelta(delta))
                if delta.stream == nori_protocol::MessageStream::Answer =>
            {
                answer_text.push_str(&delta.delta);
            }
            Some(nori_protocol::ClientEvent::ContextCompacted(compacted)) => {
                context_compacted.push(compacted);
            }
            Some(nori_protocol::ClientEvent::PromptCompleted(prompt_completed)) => {
                return TurnCapture {
                    answer_text,
                    context_compacted,
                    prompt_completed,
                };
            }
            Some(_) => {}
            None => break,
        }
    }
    panic!("turn did not complete within the deadline; answer so far: {answer_text:?}");
}

/// Wait until the backend reports the agent's advertised slash commands.
async fn wait_for_agent_commands(client_event_rx: &mut mpsc::Receiver<nori_protocol::ClientEvent>) {
    use std::time::Duration;

    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while std::time::Instant::now() < deadline {
        if let Ok(Some(nori_protocol::ClientEvent::AgentCommandsUpdate(_))) =
            tokio::time::timeout(Duration::from_secs(1), client_event_rx.recv()).await
        {
            return;
        }
    }
    panic!("agent commands update was not received");
}

/// Wait for a `SessionBranched` client event, panicking on timeout.
async fn wait_for_session_branched(
    client_event_rx: &mut mpsc::Receiver<nori_protocol::ClientEvent>,
) -> nori_protocol::SessionBranched {
    use std::time::Duration;

    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while std::time::Instant::now() < deadline {
        if let Ok(Some(nori_protocol::ClientEvent::SessionBranched(branched))) =
            tokio::time::timeout(Duration::from_secs(1), client_event_rx.recv()).await
        {
            return branched;
        }
    }
    panic!("SessionBranched event was not received");
}

/// Wait for a control-channel error event and return its message.
async fn wait_for_error_message(event_rx: &mut mpsc::Receiver<Event>) -> String {
    use std::time::Duration;

    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while std::time::Instant::now() < deadline {
        if let Ok(Some(event)) = tokio::time::timeout(Duration::from_secs(1), event_rx.recv()).await
            && let EventMsg::Error(error) = event.msg
        {
            return error.message;
        }
    }
    panic!("no error event was received");
}

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

async fn spawn_backend_for_test(
    temp_dir: &tempfile::TempDir,
) -> (
    AcpBackend,
    mpsc::Receiver<Event>,
    mpsc::Receiver<nori_protocol::ClientEvent>,
) {
    use std::time::Duration;

    let (event_tx, mut event_rx) = mpsc::channel(64);
    let (client_event_tx, client_event_rx) = mpsc::channel(64);
    let config = build_test_config(temp_dir.path());
    let backend = spawn_test_backend(&config, event_tx, Some(client_event_tx))
        .await
        .expect("Failed to spawn ACP backend");

    // Drain the SessionConfigured event.
    let _ = tokio::time::timeout(Duration::from_secs(2), event_rx.recv())
        .await
        .expect("Should receive SessionConfigured event");

    (backend, event_rx, client_event_rx)
}

/// When the agent advertises a `compact` command, `/compact` must forward the
/// agent's own command in the current session: the agent receives the literal
/// text `/compact` (not the custom summarization prompt), no replacement
/// session is created, and the transcript records a summary-less compaction.
#[tokio::test]
#[serial]
async fn test_compact_forwards_native_command_when_advertised() {
    if !mock_agent_available() {
        return;
    }
    let _commands = EnvGuard::set("MOCK_AGENT_AVAILABLE_COMMANDS", "compact");
    let _echo_prompt = EnvGuard::set("MOCK_AGENT_ECHO_PROMPT", "1");
    let _echo_session = EnvGuard::set("MOCK_AGENT_ECHO_SESSION_ID", "1");

    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
    let (backend, _event_rx, mut client_event_rx) = spawn_backend_for_test(&temp_dir).await;
    wait_for_agent_commands(&mut client_event_rx).await;

    backend
        .submit(Op::Compact)
        .await
        .expect("Failed to submit Op::Compact");
    let compact_turn = capture_turn(&mut client_event_rx).await;

    assert!(
        compact_turn.answer_text.contains("/compact"),
        "agent should receive the literal /compact command, got: {:?}",
        compact_turn.answer_text
    );
    assert!(
        !compact_turn
            .answer_text
            .contains("CONTEXT CHECKPOINT COMPACTION"),
        "the custom summarization prompt must not be sent when the agent \
         advertises native compaction"
    );
    assert_eq!(
        compact_turn.context_compacted,
        vec![nori_protocol::ContextCompacted { summary: None }],
        "native compaction should be recorded without a client-side summary"
    );

    // The follow-up prompt stays on the original session and carries no
    // injected summary.
    backend
        .submit(Op::UserInput {
            items: vec![codex_protocol::user_input::UserInput::Text {
                text: "hello after compact".to_string(),
            }],
        })
        .await
        .expect("Failed to submit follow-up prompt");
    let follow_up_turn = capture_turn(&mut client_event_rx).await;

    assert!(
        follow_up_turn.answer_text.contains("SESSION:0"),
        "native compact must not swap to a new session, got: {:?}",
        follow_up_turn.answer_text
    );
    assert!(
        follow_up_turn.answer_text.contains("hello after compact"),
        "the follow-up prompt should be echoed back, got: {:?}",
        follow_up_turn.answer_text
    );
    assert!(
        !follow_up_turn
            .answer_text
            .contains("Another language model started to solve this problem"),
        "no compact summary may be injected into prompts after native compaction"
    );
}

/// A cancelled native compaction must not record a compaction, and the session
/// must keep accepting prompts afterwards.
#[tokio::test]
#[serial]
async fn test_native_compact_cancelled_records_no_compaction() {
    use std::time::Duration;

    if !mock_agent_available() {
        return;
    }
    let _commands = EnvGuard::set("MOCK_AGENT_AVAILABLE_COMMANDS", "compact");
    let _stream = EnvGuard::set("MOCK_AGENT_STREAM_UNTIL_CANCEL", "1");

    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
    let (backend, _event_rx, mut client_event_rx) = spawn_backend_for_test(&temp_dir).await;
    wait_for_agent_commands(&mut client_event_rx).await;

    backend
        .submit(Op::Compact)
        .await
        .expect("Failed to submit Op::Compact");

    // Wait for streaming to start, then interrupt.
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        assert!(
            std::time::Instant::now() < deadline,
            "compact turn never started streaming"
        );
        if let Ok(Some(nori_protocol::ClientEvent::MessageDelta(_))) =
            tokio::time::timeout(Duration::from_secs(1), client_event_rx.recv()).await
        {
            break;
        }
    }
    backend
        .submit(Op::Interrupt)
        .await
        .expect("Failed to submit Op::Interrupt");
    let cancelled_turn = capture_turn(&mut client_event_rx).await;

    assert_eq!(
        cancelled_turn.prompt_completed.stop_reason,
        nori_protocol::StopReason::Cancelled
    );
    assert_eq!(
        cancelled_turn.context_compacted,
        Vec::new(),
        "a cancelled compaction must not be recorded as a compaction"
    );

    // The session still accepts prompts: the next turn streams and can be
    // cancelled cleanly.
    backend
        .submit(Op::UserInput {
            items: vec![codex_protocol::user_input::UserInput::Text {
                text: "still alive?".to_string(),
            }],
        })
        .await
        .expect("Failed to submit follow-up prompt");
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        assert!(
            std::time::Instant::now() < deadline,
            "follow-up turn never started streaming"
        );
        if let Ok(Some(nori_protocol::ClientEvent::MessageDelta(_))) =
            tokio::time::timeout(Duration::from_secs(1), client_event_rx.recv()).await
        {
            break;
        }
    }
    backend
        .submit(Op::Interrupt)
        .await
        .expect("Failed to submit follow-up interrupt");
    let follow_up_turn = capture_turn(&mut client_event_rx).await;
    assert_eq!(
        follow_up_turn.prompt_completed.stop_reason,
        nori_protocol::StopReason::Cancelled
    );
}

/// Branching with an agent that advertises `session/fork` swaps the runtime to
/// the forked session: subsequent prompts arrive on the new agent session.
#[tokio::test]
#[serial]
async fn test_branch_session_swaps_to_forked_session() {
    if !mock_agent_available() {
        return;
    }
    let _fork = EnvGuard::set("MOCK_AGENT_SUPPORT_SESSION_FORK", "1");
    let _echo_session = EnvGuard::set("MOCK_AGENT_ECHO_SESSION_ID", "1");

    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
    let (backend, _event_rx, mut client_event_rx) = spawn_backend_for_test(&temp_dir).await;

    backend
        .submit(Op::BranchSession)
        .await
        .expect("Failed to submit Op::BranchSession");
    let branched = wait_for_session_branched(&mut client_event_rx).await;
    assert_ne!(
        branched.new_session_id, "0",
        "fork must yield a new session"
    );

    backend
        .submit(Op::UserInput {
            items: vec![codex_protocol::user_input::UserInput::Text {
                text: "hello branched session".to_string(),
            }],
        })
        .await
        .expect("Failed to submit prompt after branch");
    let turn = capture_turn(&mut client_event_rx).await;

    assert!(
        turn.answer_text
            .contains(&format!("SESSION:{}", branched.new_session_id)),
        "prompts after branching must target the forked session, got: {:?}",
        turn.answer_text
    );
}

/// Branching with an agent that does NOT advertise `session/fork` surfaces a
/// clear error and leaves the current session untouched.
#[tokio::test]
#[serial]
async fn test_branch_session_unsupported_agent_reports_error() {
    if !mock_agent_available() {
        return;
    }
    let _no_fork = EnvGuard::remove("MOCK_AGENT_SUPPORT_SESSION_FORK");
    let _echo_session = EnvGuard::set("MOCK_AGENT_ECHO_SESSION_ID", "1");

    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
    let (backend, mut event_rx, mut client_event_rx) = spawn_backend_for_test(&temp_dir).await;

    backend
        .submit(Op::BranchSession)
        .await
        .expect("Failed to submit Op::BranchSession");
    let message = wait_for_error_message(&mut event_rx).await;
    assert!(
        message.contains("does not support branching"),
        "unsupported branching should be reported as such, got: {message:?}"
    );

    // No branch may be reported when the agent lacks the capability.
    while let Ok(Some(event)) = tokio::time::timeout(
        std::time::Duration::from_millis(300),
        client_event_rx.recv(),
    )
    .await
    {
        assert!(
            !matches!(event, nori_protocol::ClientEvent::SessionBranched(_)),
            "no SessionBranched event may be emitted for an unsupported agent"
        );
    }

    backend
        .submit(Op::UserInput {
            items: vec![codex_protocol::user_input::UserInput::Text {
                text: "hello original session".to_string(),
            }],
        })
        .await
        .expect("Failed to submit prompt after failed branch");
    let turn = capture_turn(&mut client_event_rx).await;
    assert!(
        turn.answer_text.contains("SESSION:0"),
        "a failed branch must leave the original session active, got: {:?}",
        turn.answer_text
    );
}

/// The fork response's `config_options` replace the live session config
/// snapshot (the mock advertises `thought_level: high` for forked sessions).
#[tokio::test]
#[serial]
async fn test_branch_session_applies_forked_config_options() {
    if !mock_agent_available() {
        return;
    }
    let _fork = EnvGuard::set("MOCK_AGENT_SUPPORT_SESSION_FORK", "1");

    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
    let (backend, _event_rx, mut client_event_rx) = spawn_backend_for_test(&temp_dir).await;

    let thought_level_value = |options: &[acp::SessionConfigOption]| -> Option<String> {
        options.iter().find_map(|option| {
            if option.id.to_string() != "thought_level" {
                return None;
            }
            match &option.kind {
                acp::SessionConfigKind::Select(select) => Some(select.current_value.to_string()),
                _ => None,
            }
        })
    };

    assert_eq!(
        thought_level_value(&backend.config_options()).as_deref(),
        Some("medium"),
        "precondition: fresh mock sessions start at thought_level medium"
    );

    backend
        .submit(Op::BranchSession)
        .await
        .expect("Failed to submit Op::BranchSession");
    let _ = wait_for_session_branched(&mut client_event_rx).await;

    assert_eq!(
        thought_level_value(&backend.config_options()).as_deref(),
        Some("high"),
        "the fork response's config_options must replace the live snapshot"
    );
}
