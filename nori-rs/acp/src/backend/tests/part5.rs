use super::*;

struct EnvGuard {
    name: &'static str,
    previous: Option<String>,
}

struct RegistryGuard;

impl RegistryGuard {
    fn with_agents(agents: Vec<crate::config::AgentConfigToml>) -> Self {
        crate::registry::initialize_registry(agents).expect("registry override should be valid");
        Self
    }
}

impl Drop for RegistryGuard {
    fn drop(&mut self) {
        crate::registry::initialize_registry(Vec::new()).expect("registry reset should be valid");
    }
}

impl EnvGuard {
    fn set(name: &'static str, value: &str) -> Self {
        let previous = std::env::var(name).ok();
        unsafe {
            std::env::set_var(name, value);
        }
        Self { name, previous }
    }

    fn remove(name: &'static str) -> Self {
        let previous = std::env::var(name).ok();
        unsafe {
            std::env::remove_var(name);
        }
        Self { name, previous }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        match &self.previous {
            Some(value) => unsafe {
                std::env::set_var(self.name, value);
            },
            None => unsafe {
                std::env::remove_var(self.name);
            },
        }
    }
}

async fn assert_no_prompt_completed(
    backend_event_rx: &mut mpsc::Receiver<BackendEvent>,
    window: std::time::Duration,
) {
    let start = std::time::Instant::now();
    while start.elapsed() < window {
        let remaining = window
            .saturating_sub(start.elapsed())
            .min(std::time::Duration::from_millis(100));
        if let Some(event) = recv_backend_client(backend_event_rx, remaining).await {
            assert!(
                !matches!(event, nori_protocol::ClientEvent::PromptCompleted(_)),
                "unexpected extra PromptCompleted event: {event:?}"
            );
        }
    }
}

async fn collect_completed_prompt_text(
    backend_event_rx: &mut mpsc::Receiver<BackendEvent>,
    timeout: std::time::Duration,
    timeout_message: &str,
) -> String {
    let mut agent_text = String::new();
    let start = std::time::Instant::now();
    loop {
        if start.elapsed() > timeout {
            panic!("{timeout_message}");
        }
        match recv_backend_client(backend_event_rx, std::time::Duration::from_millis(500)).await {
            Some(nori_protocol::ClientEvent::MessageDelta(delta)) => {
                agent_text.push_str(&delta.delta);
            }
            Some(nori_protocol::ClientEvent::PromptCompleted(_)) => return agent_text,
            Some(_) => {}
            None => {}
        }
    }
}

#[tokio::test]
#[serial]
async fn user_shell_command_executes_locally_and_emits_exec_lifecycle() {
    use codex_protocol::protocol::ExecCommandSource;
    use codex_protocol::protocol::ExecOutputStream;
    use pretty_assertions::assert_eq;
    use std::time::Duration;

    let mock_config =
        crate::registry::get_agent_config("mock-model").expect("mock-model should be registered");
    if !std::path::Path::new(&mock_config.command).exists() {
        eprintln!(
            "Skipping test: mock_acp_agent not found at {}",
            mock_config.command
        );
        return;
    }

    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
    let (backend_event_tx, mut backend_event_rx) = mpsc::channel(64);
    let config = build_test_config(temp_dir.path());
    let backend = AcpBackend::spawn(&config, backend_event_tx)
        .await
        .expect("Failed to spawn ACP backend");

    let _ = recv_backend_control(&mut backend_event_rx, Duration::from_secs(5))
        .await
        .expect("Should receive SessionConfigured event");

    backend
        .submit(Op::RunUserShellCommand {
            command: "printf nori-shell-mode".to_string(),
        })
        .await
        .expect("Failed to submit shell command");

    let mut saw_begin = false;
    let mut saw_stdout = false;
    let mut saw_end = false;
    let mut saw_complete = false;
    for _ in 0..8 {
        let event = recv_backend_control(&mut backend_event_rx, Duration::from_secs(5))
            .await
            .expect("expected shell command event");
        match event.msg {
            EventMsg::ExecCommandBegin(ev) => {
                assert_eq!(ev.source, ExecCommandSource::UserShell);
                assert_eq!(ev.cwd, temp_dir.path());
                saw_begin = true;
            }
            EventMsg::ExecCommandOutputDelta(ev) => {
                if ev.stream == ExecOutputStream::Stdout
                    && ev
                        .chunk
                        .windows(b"nori-shell-mode".len())
                        .any(|w| w == b"nori-shell-mode")
                {
                    saw_stdout = true;
                }
            }
            EventMsg::ExecCommandEnd(ev) => {
                assert_eq!(ev.source, ExecCommandSource::UserShell);
                assert!(
                    ev.stdout.contains("nori-shell-mode"),
                    "stdout should contain 'nori-shell-mode', got: {:?}",
                    ev.stdout
                );
                assert!(
                    ev.aggregated_output.contains("nori-shell-mode"),
                    "aggregated_output should contain 'nori-shell-mode', got: {:?}",
                    ev.aggregated_output
                );
                assert_eq!(ev.exit_code, 0);
                saw_end = true;
            }
            EventMsg::TaskComplete(ev) => {
                assert_eq!(ev.last_agent_message, None);
                saw_complete = true;
                break;
            }
            _ => {}
        }
    }

    assert!(saw_begin, "expected ExecCommandBegin");
    assert!(saw_stdout, "expected stdout delta");
    assert!(saw_end, "expected ExecCommandEnd");
    assert!(saw_complete, "expected TaskComplete");
}

#[tokio::test]
#[serial]
async fn user_shell_command_submit_returns_while_command_is_running() {
    use pretty_assertions::assert_eq;
    use std::time::Duration;

    let mock_config =
        crate::registry::get_agent_config("mock-model").expect("mock-model should be registered");
    if !std::path::Path::new(&mock_config.command).exists() {
        eprintln!(
            "Skipping test: mock_acp_agent not found at {}",
            mock_config.command
        );
        return;
    }

    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
    let (backend_event_tx, mut backend_event_rx) = mpsc::channel(64);
    let config = build_test_config(temp_dir.path());
    let backend = AcpBackend::spawn(&config, backend_event_tx)
        .await
        .expect("Failed to spawn ACP backend");

    let _ = recv_backend_control(&mut backend_event_rx, Duration::from_secs(5))
        .await
        .expect("Should receive SessionConfigured event");

    let submit_result = tokio::time::timeout(
        Duration::from_millis(250),
        backend.submit(Op::RunUserShellCommand {
            command: "printf nori-start; sleep 1; printf nori-end".to_string(),
        }),
    )
    .await
    .expect("shell command submission should not wait for command completion")
    .expect("Failed to submit shell command");

    assert_eq!(submit_result.is_empty(), false);

    let mut saw_complete = false;
    for _ in 0..8 {
        let event = recv_backend_control(&mut backend_event_rx, Duration::from_secs(5))
            .await
            .expect("expected shell command event");
        if let EventMsg::TaskComplete(ev) = event.msg {
            assert_eq!(ev.last_agent_message, None);
            saw_complete = true;
            break;
        }
    }

    assert!(
        saw_complete,
        "expected TaskComplete after shell command exits"
    );
}

/// Test that session_context is prepended to the first user prompt.
///
/// When `session_context` is set on `AcpBackendConfig`, its value should appear
/// in the prompt text sent to the ACP agent on the first user turn.
#[tokio::test]
#[serial]
async fn test_session_context_prepended_to_first_prompt() {
    use std::time::Duration;

    let mock_config =
        crate::registry::get_agent_config("mock-model").expect("mock-model should be registered");
    if !std::path::Path::new(&mock_config.command).exists() {
        eprintln!(
            "Skipping test: mock_acp_agent not found at {}",
            mock_config.command
        );
        return;
    }

    let _env_guard = EnvGuard::set("MOCK_AGENT_ECHO_PROMPT", "1");

    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
    let (backend_event_tx, mut backend_event_rx) = mpsc::channel(64);

    let mut config = build_test_config(temp_dir.path());
    config.session_context = Some("NORI_SESSION_CONTEXT_MARKER".to_string());

    let backend = AcpBackend::spawn(&config, backend_event_tx)
        .await
        .expect("Failed to spawn ACP backend");

    // Wait for SessionConfigured
    let _ = recv_backend_control(&mut backend_event_rx, Duration::from_secs(5))
        .await
        .expect("Should receive SessionConfigured event");

    // Send a user prompt
    backend
        .submit(Op::UserInput {
            items: vec![codex_protocol::user_input::UserInput::Text {
                text: "hello agent".to_string(),
            }],
        })
        .await
        .expect("Failed to submit user input");

    // Collect agent response text from MessageDelta events until PromptCompleted.
    let mut agent_text = String::new();
    let timeout = Duration::from_secs(10);
    let start = std::time::Instant::now();
    loop {
        if start.elapsed() > timeout {
            panic!("Timed out waiting for PromptCompleted");
        }
        match recv_backend_client(&mut backend_event_rx, Duration::from_secs(5)).await {
            Some(nori_protocol::ClientEvent::MessageDelta(delta)) => {
                agent_text.push_str(&delta.delta);
            }
            Some(nori_protocol::ClientEvent::PromptCompleted(_)) => break,
            Some(_) => continue,
            None => panic!("Backend event channel closed unexpectedly"),
        }
    }

    // The mock agent echoes back the prompt text. Verify it contains
    // both the session context and the user's message.
    assert!(
        agent_text.contains("NORI_SESSION_CONTEXT_MARKER"),
        "Expected session context in agent's echoed prompt, got: {agent_text}"
    );
    assert!(
        agent_text.contains("hello agent"),
        "Expected user prompt in agent's echoed prompt, got: {agent_text}"
    );
}

/// When a session goal is active, the next user prompt sent to the ACP agent
/// includes a structured goal context block.
#[tokio::test]
#[serial]
async fn test_goal_context_prepended_to_user_prompt() {
    use std::time::Duration;

    let mock_config =
        crate::registry::get_agent_config("mock-model").expect("mock-model should be registered");
    if !std::path::Path::new(&mock_config.command).exists() {
        eprintln!(
            "Skipping test: mock_acp_agent not found at {}",
            mock_config.command
        );
        return;
    }

    let _env_guard = EnvGuard::set("MOCK_AGENT_ECHO_PROMPT", "1");
    let _mcp_guard = EnvGuard::remove("MOCK_AGENT_MCP_HTTP");

    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
    let (backend_event_tx, mut backend_event_rx) = mpsc::channel(64);
    let config = build_test_config(temp_dir.path());

    let backend = AcpBackend::spawn(&config, backend_event_tx)
        .await
        .expect("Failed to spawn ACP backend");

    let _ = recv_backend_control(&mut backend_event_rx, Duration::from_secs(5))
        .await
        .expect("Should receive SessionConfigured event");

    backend
        .submit(Op::ThreadGoalSet {
            objective: Some("Keep the north star visible".to_string()),
            status: Some(codex_protocol::protocol::ThreadGoalStatus::Active),
        })
        .await
        .expect("Failed to set goal");

    let initial_goal_work = collect_completed_prompt_text(
        &mut backend_event_rx,
        Duration::from_secs(10),
        "Timed out waiting for initial goal continuation",
    )
    .await;
    assert!(
        initial_goal_work.contains("Continue working toward the active thread goal")
            && initial_goal_work.contains("Keep the north star visible"),
        "expected /goal to start immediate goal work, got: {initial_goal_work}"
    );

    backend
        .submit(Op::UserInput {
            items: vec![codex_protocol::user_input::UserInput::Text {
                text: "hello agent".to_string(),
            }],
        })
        .await
        .expect("Failed to submit user input");

    let agent_text = collect_completed_prompt_text(
        &mut backend_event_rx,
        Duration::from_secs(10),
        "Timed out waiting for PromptCompleted",
    )
    .await;

    assert!(
        agent_text.contains("<goal_context>")
            && agent_text.contains("Objective: Keep the north star visible")
            && agent_text.contains("hello agent"),
        "Expected goal context and user prompt in agent echo, got: {agent_text}"
    );
}

#[tokio::test]
#[serial]
async fn setting_active_goal_starts_hidden_continuation_without_extra_prompt() {
    use std::time::Duration;

    let mock_config =
        crate::registry::get_agent_config("mock-model").expect("mock-model should be registered");
    if !std::path::Path::new(&mock_config.command).exists() {
        eprintln!(
            "Skipping test: mock_acp_agent not found at {}",
            mock_config.command
        );
        return;
    }

    let _env_guard = EnvGuard::set("MOCK_AGENT_ECHO_PROMPT", "1");
    let _mcp_guard = EnvGuard::remove("MOCK_AGENT_MCP_HTTP");

    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
    let (backend_event_tx, mut backend_event_rx) = mpsc::channel(64);
    let config = build_test_config(temp_dir.path());

    let backend = AcpBackend::spawn(&config, backend_event_tx)
        .await
        .expect("Failed to spawn ACP backend");

    let _ = recv_backend_control(&mut backend_event_rx, Duration::from_secs(5))
        .await
        .expect("Should receive SessionConfigured event");

    backend
        .submit(Op::ThreadGoalSet {
            objective: Some("Begin immediately from the goal command".to_string()),
            status: Some(codex_protocol::protocol::ThreadGoalStatus::Active),
        })
        .await
        .expect("Failed to set goal");

    let agent_text = collect_completed_prompt_text(
        &mut backend_event_rx,
        Duration::from_secs(10),
        "Timed out waiting for hidden goal continuation",
    )
    .await;

    assert!(
        agent_text.contains("Continue working toward the active thread goal")
            && agent_text.contains("Begin immediately from the goal command"),
        "expected goal command to start a hidden continuation, got: {agent_text}"
    );

    assert_no_prompt_completed(&mut backend_event_rx, Duration::from_secs(2)).await;
}

#[tokio::test]
#[serial]
async fn active_goal_submits_one_hidden_continuation_after_user_turn() {
    use std::time::Duration;

    let mock_config =
        crate::registry::get_agent_config("mock-model").expect("mock-model should be registered");
    if !std::path::Path::new(&mock_config.command).exists() {
        eprintln!(
            "Skipping test: mock_acp_agent not found at {}",
            mock_config.command
        );
        return;
    }

    let _env_guard = EnvGuard::set("MOCK_AGENT_ECHO_PROMPT", "1");
    let _mcp_guard = EnvGuard::remove("MOCK_AGENT_MCP_HTTP");

    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
    let (backend_event_tx, mut backend_event_rx) = mpsc::channel(64);
    let config = build_test_config(temp_dir.path());

    let backend = AcpBackend::spawn(&config, backend_event_tx)
        .await
        .expect("Failed to spawn ACP backend");

    let _ = recv_backend_control(&mut backend_event_rx, Duration::from_secs(5))
        .await
        .expect("Should receive SessionConfigured event");

    backend
        .submit(Op::ThreadGoalSet {
            objective: Some("Ship the ACP goal command".to_string()),
            status: Some(codex_protocol::protocol::ThreadGoalStatus::Active),
        })
        .await
        .expect("Failed to set goal");

    let initial_goal_work = collect_completed_prompt_text(
        &mut backend_event_rx,
        Duration::from_secs(10),
        "Timed out waiting for initial goal continuation",
    )
    .await;
    assert!(
        initial_goal_work.contains("Continue working toward the active thread goal")
            && initial_goal_work.contains("Ship the ACP goal command"),
        "expected /goal to start immediate goal work, got: {initial_goal_work}"
    );

    backend
        .submit(Op::UserInput {
            items: vec![codex_protocol::user_input::UserInput::Text {
                text: "start the work".to_string(),
            }],
        })
        .await
        .expect("Failed to submit user input");

    let user_turn = collect_completed_prompt_text(
        &mut backend_event_rx,
        Duration::from_secs(10),
        "Timed out waiting for visible user turn",
    )
    .await;
    let post_user_goal_work = collect_completed_prompt_text(
        &mut backend_event_rx,
        Duration::from_secs(10),
        "Timed out waiting for hidden goal continuation",
    )
    .await;

    assert!(
        user_turn.contains("start the work"),
        "expected visible user turn after initial goal work, got: {user_turn}"
    );
    assert!(
        post_user_goal_work.contains("Continue working toward the active thread goal")
            && post_user_goal_work.contains("Ship the ACP goal command"),
        "expected hidden goal continuation after user turn, got: {post_user_goal_work}"
    );

    assert_no_prompt_completed(&mut backend_event_rx, Duration::from_secs(2)).await;
}

#[tokio::test]
#[serial]
async fn http_mcp_agent_without_goal_mcp_connection_does_not_chain_hidden_continuations() {
    use std::time::Duration;

    let mock_config =
        crate::registry::get_agent_config("mock-model").expect("mock-model should be registered");
    if !std::path::Path::new(&mock_config.command).exists() {
        eprintln!(
            "Skipping test: mock_acp_agent not found at {}",
            mock_config.command
        );
        return;
    }

    let _echo_guard = EnvGuard::set("MOCK_AGENT_ECHO_PROMPT", "1");
    let _mcp_guard = EnvGuard::set("MOCK_AGENT_MCP_HTTP", "1");

    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
    let wire_log_dir = temp_dir.path().join("acp-wire");
    let (backend_event_tx, mut backend_event_rx) = mpsc::channel(64);
    let mut config = build_test_config(temp_dir.path());
    config.acp_proxy = crate::config::AcpProxyConfig {
        enabled: true,
        log_dir: wire_log_dir.clone(),
    };

    let backend = AcpBackend::spawn(&config, backend_event_tx)
        .await
        .expect("Failed to spawn ACP backend");

    let _ = recv_backend_control(&mut backend_event_rx, Duration::from_secs(5))
        .await
        .expect("Should receive SessionConfigured event");

    backend
        .submit(Op::ThreadGoalSet {
            objective: Some("Keep going until the goal tool stops the loop".to_string()),
            status: Some(codex_protocol::protocol::ThreadGoalStatus::Active),
        })
        .await
        .expect("Failed to set goal");

    let initial_goal_work = collect_completed_prompt_text(
        &mut backend_event_rx,
        Duration::from_secs(10),
        "Timed out waiting for initial goal continuation",
    )
    .await;
    assert!(
        initial_goal_work.contains("Continue working toward the active thread goal"),
        "expected /goal to start immediate goal work, got: {initial_goal_work}"
    );

    backend
        .submit(Op::UserInput {
            items: vec![codex_protocol::user_input::UserInput::Text {
                text: "start the chain".to_string(),
            }],
        })
        .await
        .expect("Failed to submit user input");

    let user_turn = collect_completed_prompt_text(
        &mut backend_event_rx,
        Duration::from_secs(10),
        "Timed out waiting for visible user turn",
    )
    .await;
    let post_user_goal_work = collect_completed_prompt_text(
        &mut backend_event_rx,
        Duration::from_secs(10),
        "Timed out waiting for goal continuation",
    )
    .await;

    assert!(
        user_turn.contains("start the chain"),
        "expected visible user turn after initial goal work, got: {user_turn}"
    );
    assert!(
        post_user_goal_work.contains("Continue working toward the active thread goal"),
        "expected post-user hidden goal continuation, got: {post_user_goal_work}"
    );

    tokio::time::sleep(Duration::from_secs(2)).await;
    assert_eq!(
        count_logged_requests(&wire_log_dir, "session/prompt"),
        3,
        "expected no chained goal continuation before the goal MCP server is connected"
    );

    backend
        .submit(Op::Shutdown)
        .await
        .expect("Failed to shut down backend");
}

#[tokio::test]
#[serial]
async fn goal_mcp_initialize_allows_chained_hidden_continuations() {
    use std::time::Duration;

    let mock_config =
        crate::registry::get_agent_config("mock-model").expect("mock-model should be registered");
    if !std::path::Path::new(&mock_config.command).exists() {
        eprintln!(
            "Skipping test: mock_acp_agent not found at {}",
            mock_config.command
        );
        return;
    }

    let _echo_guard = EnvGuard::set("MOCK_AGENT_ECHO_PROMPT", "1");
    let _mcp_guard = EnvGuard::set("MOCK_AGENT_MCP_HTTP", "1");

    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
    let wire_log_dir = temp_dir.path().join("acp-wire");
    let (backend_event_tx, mut backend_event_rx) = mpsc::channel(64);
    let mut config = build_test_config(temp_dir.path());
    config.acp_proxy = crate::config::AcpProxyConfig {
        enabled: true,
        log_dir: wire_log_dir.clone(),
    };

    let backend = AcpBackend::spawn(&config, backend_event_tx)
        .await
        .expect("Failed to spawn ACP backend");

    let _ = recv_backend_control(&mut backend_event_rx, Duration::from_secs(5))
        .await
        .expect("Should receive SessionConfigured event");

    let new_session = latest_logged_new_session(&wire_log_dir);
    let goal_mcp_url = nori_client_http_url(&new_session);
    initialize_nori_client_mcp(&goal_mcp_url).await;

    backend
        .submit(Op::ThreadGoalSet {
            objective: Some("Keep going until a later tool call stops it".to_string()),
            status: Some(codex_protocol::protocol::ThreadGoalStatus::Active),
        })
        .await
        .expect("Failed to set goal");

    let start = std::time::Instant::now();
    while count_logged_requests(&wire_log_dir, "session/prompt") < 3 {
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "expected initialized goal MCP server to allow chaining beyond the initial goal continuation"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    backend
        .submit(Op::Shutdown)
        .await
        .expect("Failed to shut down backend");
}

#[tokio::test]
#[serial]
async fn completed_goal_stops_chained_hidden_continuations() {
    use std::time::Duration;

    let mock_config =
        crate::registry::get_agent_config("mock-model").expect("mock-model should be registered");
    if !std::path::Path::new(&mock_config.command).exists() {
        eprintln!(
            "Skipping test: mock_acp_agent not found at {}",
            mock_config.command
        );
        return;
    }

    let _mcp_guard = EnvGuard::set("MOCK_AGENT_MCP_HTTP", "1");
    let _echo_guard = EnvGuard::set("MOCK_AGENT_ECHO_PROMPT", "1");
    let _delay_guard = EnvGuard::set("MOCK_AGENT_DELAY_MS", "200");

    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
    let (backend_event_tx, mut backend_event_rx) = mpsc::channel(64);
    let config = build_test_config(temp_dir.path());

    let backend = AcpBackend::spawn(&config, backend_event_tx)
        .await
        .expect("Failed to spawn ACP backend");

    let _ = recv_backend_control(&mut backend_event_rx, Duration::from_secs(5))
        .await
        .expect("Should receive SessionConfigured event");

    backend
        .submit(Op::ThreadGoalSet {
            objective: Some("Stop after the goal is complete".to_string()),
            status: Some(codex_protocol::protocol::ThreadGoalStatus::Active),
        })
        .await
        .expect("Failed to set goal");

    let initial_goal_work = collect_completed_prompt_text(
        &mut backend_event_rx,
        Duration::from_secs(10),
        "Timed out waiting for initial goal continuation",
    )
    .await;
    assert!(
        initial_goal_work.contains("Continue working toward the active thread goal"),
        "expected /goal to start immediate goal work, got: {initial_goal_work}"
    );

    backend
        .submit(Op::UserInput {
            items: vec![codex_protocol::user_input::UserInput::Text {
                text: "start then finish".to_string(),
            }],
        })
        .await
        .expect("Failed to submit user input");

    let user_turn = collect_completed_prompt_text(
        &mut backend_event_rx,
        Duration::from_secs(10),
        "Timed out waiting for visible user turn",
    )
    .await;
    assert!(
        user_turn.contains("start then finish"),
        "expected visible user turn after initial goal work, got: {user_turn}"
    );

    backend
        .submit(Op::ThreadGoalSet {
            objective: None,
            status: Some(codex_protocol::protocol::ThreadGoalStatus::Complete),
        })
        .await
        .expect("Failed to complete goal");

    let already_queued_goal_work = collect_completed_prompt_text(
        &mut backend_event_rx,
        Duration::from_secs(10),
        "Timed out waiting for already queued hidden goal continuation",
    )
    .await;
    assert!(
        already_queued_goal_work.contains("Continue working toward the active thread goal"),
        "expected already queued goal continuation, got: {already_queued_goal_work}"
    );

    assert_no_prompt_completed(&mut backend_event_rx, Duration::from_millis(500)).await;

    backend
        .submit(Op::Shutdown)
        .await
        .expect("Failed to shut down backend");
}

#[tokio::test]
#[serial]
async fn usage_updates_refresh_goal_token_count() {
    use std::time::Duration;

    let mock_config =
        crate::registry::get_agent_config("mock-model").expect("mock-model should be registered");
    if !std::path::Path::new(&mock_config.command).exists() {
        eprintln!(
            "Skipping test: mock_acp_agent not found at {}",
            mock_config.command
        );
        return;
    }

    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
    let (backend_event_tx, mut backend_event_rx) = mpsc::channel(64);
    let config = build_test_config(temp_dir.path());

    let backend = AcpBackend::spawn(&config, backend_event_tx)
        .await
        .expect("Failed to spawn ACP backend");

    let _ = recv_backend_control(&mut backend_event_rx, Duration::from_secs(5))
        .await
        .expect("Should receive SessionConfigured event");

    backend
        .apply_session_event(session_reducer::InboundEvent::Notification(Box::new(
            acp::SessionUpdate::UsageUpdate(acp::UsageUpdate::new(100, 4096)),
        )))
        .await;

    backend
        .submit(Op::ThreadGoalSet {
            objective: Some("Track token budget".to_string()),
            status: Some(codex_protocol::protocol::ThreadGoalStatus::Active),
        })
        .await
        .expect("Failed to set goal");

    backend
        .apply_session_event(session_reducer::InboundEvent::Notification(Box::new(
            acp::SessionUpdate::UsageUpdate(acp::UsageUpdate::new(175, 4096)),
        )))
        .await;

    let mut saw_goal_update_with_tokens = false;
    for _ in 0..4 {
        match recv_backend_client(&mut backend_event_rx, Duration::from_secs(5)).await {
            Some(nori_protocol::ClientEvent::ThreadGoalUpdated(update))
                if update.goal.tokens_used == 75 =>
            {
                saw_goal_update_with_tokens = true;
                break;
            }
            Some(_) => {}
            None => panic!("Backend event channel closed unexpectedly"),
        }
    }

    assert!(saw_goal_update_with_tokens);
}

#[tokio::test]
#[serial]
async fn goal_mcp_server_is_advertised_to_http_mcp_agents() {
    let Some(new_session) = logged_new_session_for_mock_agent(true).await else {
        return;
    };

    assert!(
        new_session.mcp_servers.iter().any(|server| {
            matches!(
                server,
                acp::McpServer::Http(http)
                    if http.name == "nori-client" && http.url.starts_with("http://127.0.0.1:")
            )
        }),
        "expected session/new to advertise the local loopback nori-client MCP server, got: {:?}",
        new_session.mcp_servers
    );
}

#[tokio::test]
#[serial]
async fn goal_mcp_server_is_not_advertised_without_http_mcp_capability() {
    let Some(new_session) = logged_new_session_for_mock_agent(false).await else {
        return;
    };

    assert!(
        new_session.mcp_servers.iter().all(|server| {
            !matches!(
                server,
                acp::McpServer::Http(http) if http.name == "nori-client"
            )
        }),
        "expected session/new to omit nori-client without HTTP MCP capability, got: {:?}",
        new_session.mcp_servers
    );
}

#[tokio::test]
#[serial]
async fn codex_agent_receives_loopback_http_goal_mcp_server() {
    let mock_config =
        crate::registry::get_agent_config("mock-model").expect("mock-model should be registered");
    if !std::path::Path::new(&mock_config.command).exists() {
        eprintln!(
            "Skipping test: mock_acp_agent not found at {}",
            mock_config.command
        );
        return;
    }

    let _registry_guard = RegistryGuard::with_agents(vec![crate::config::AgentConfigToml {
        name: "Codex".to_string(),
        slug: "codex".to_string(),
        distribution: crate::config::AgentDistributionToml {
            local: Some(crate::config::LocalDistribution {
                command: mock_config.command.clone(),
                args: mock_config.args.clone(),
                env: mock_config.env.clone(),
            }),
            ..Default::default()
        },
        context_window_size: None,
        auth_hint: None,
        transcript_base_dir: None,
    }]);
    let Some(new_session) = logged_new_session_for_agent("codex", true).await else {
        return;
    };

    assert!(
        new_session.mcp_servers.iter().any(|server| {
            matches!(
                server,
                acp::McpServer::Http(http)
                    if http.name == "nori-client" && http.url.starts_with("http://127.0.0.1:")
            )
        }),
        "expected session/new to advertise a real loopback HTTP nori-client server for Codex ACP, got: {:?}",
        new_session.mcp_servers
    );
}

async fn logged_new_session_for_mock_agent(
    advertise_http_mcp: bool,
) -> Option<acp::NewSessionRequest> {
    logged_new_session_for_agent("mock-model", advertise_http_mcp).await
}

async fn logged_new_session_for_agent(
    agent: &str,
    advertise_http_mcp: bool,
) -> Option<acp::NewSessionRequest> {
    use std::time::Duration;

    let mock_config = crate::registry::get_agent_config(agent).expect("agent should be registered");
    if !std::path::Path::new(&mock_config.command).exists() {
        eprintln!(
            "Skipping test: mock_acp_agent not found at {}",
            mock_config.command
        );
        return None;
    }

    let _env_guard = if advertise_http_mcp {
        EnvGuard::set("MOCK_AGENT_MCP_HTTP", "1")
    } else {
        EnvGuard::remove("MOCK_AGENT_MCP_HTTP")
    };

    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
    let wire_log_dir = temp_dir.path().join("acp-wire");
    let (backend_event_tx, mut backend_event_rx) = mpsc::channel(64);
    let mut config = build_test_config(temp_dir.path());
    config.agent = agent.to_string();
    config.acp_proxy = crate::config::AcpProxyConfig {
        enabled: true,
        log_dir: wire_log_dir.clone(),
    };

    let backend = AcpBackend::spawn(&config, backend_event_tx)
        .await
        .expect("Failed to spawn ACP backend");

    let configured = recv_backend_control(&mut backend_event_rx, Duration::from_secs(5))
        .await
        .expect("Should receive SessionConfigured event");
    assert!(
        matches!(configured.msg, EventMsg::SessionConfigured(_)),
        "expected SessionConfigured event, got: {configured:?}"
    );

    backend
        .submit(Op::Shutdown)
        .await
        .expect("Failed to shut down ACP backend");

    Some(latest_logged_new_session(&wire_log_dir))
}

fn latest_logged_new_session(log_dir: &std::path::Path) -> acp::NewSessionRequest {
    let log_content = read_wire_log(log_dir);
    let new_session = log_content
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).expect("json wire log line"))
        .filter(|record| {
            record["direction"] == "client_to_agent" && record["message"]["method"] == "session/new"
        })
        .next_back()
        .expect("session/new should be logged");
    serde_json::from_value(new_session["message"]["params"].clone())
        .expect("session/new params should match ACP schema")
}

fn read_wire_log(log_dir: &std::path::Path) -> String {
    let log_path = std::fs::read_dir(log_dir)
        .expect("wire log dir exists")
        .map(|entry| entry.expect("wire log entry").path())
        .find(|path| path.extension().is_some_and(|ext| ext == "jsonl"))
        .expect("wire log should be written");
    std::fs::read_to_string(log_path).expect("wire log should be readable")
}

fn count_logged_requests(log_dir: &std::path::Path, method: &str) -> usize {
    let log_content = read_wire_log(log_dir);
    log_content
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).expect("json wire log line"))
        .filter(|record| {
            record["direction"] == "client_to_agent" && record["message"]["method"] == method
        })
        .count()
}

fn nori_client_http_url(new_session: &acp::NewSessionRequest) -> String {
    new_session
        .mcp_servers
        .iter()
        .find_map(|server| match server {
            acp::McpServer::Http(http) if http.name == "nori-client" => Some(http.url.clone()),
            _ => None,
        })
        .expect("session/new should advertise nori-client HTTP MCP server")
}

/// Drive a real MCP `initialize` against the loopback `nori-client` server,
/// mirroring how an external agent connects. The rmcp streamable-HTTP server is
/// spec-compliant, so the POST must send `Accept: application/json,
/// text/event-stream` and the result arrives as an SSE `data:` line.
async fn initialize_nori_client_mcp(url: &str) {
    use tokio::io::AsyncReadExt;
    use tokio::io::AsyncWriteExt;

    let without_scheme = url
        .strip_prefix("http://")
        .expect("test URL should be plain HTTP");
    let (host_port, path) = without_scheme
        .split_once('/')
        .expect("test URL should include a path");
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": { "name": "nori-test", "version": "0" }
        }
    })
    .to_string();
    let request = format!(
        "POST /{path} HTTP/1.1\r\n\
Host: {host_port}\r\n\
Accept: application/json, text/event-stream\r\n\
Content-Type: application/json\r\n\
Content-Length: {}\r\n\
Connection: close\r\n\r\n\
{body}",
        body.len()
    );
    let mut stream = tokio::net::TcpStream::connect(host_port)
        .await
        .expect("nori-client MCP server should accept initialize");
    stream
        .write_all(request.as_bytes())
        .await
        .expect("initialize request should write");
    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .await
        .expect("initialize response should read");
    assert!(
        response.contains("200 OK") && response.contains("\"serverInfo\""),
        "expected successful nori-client MCP initialize response, got: {response}"
    );
}

/// Test that session_context is consumed after the first prompt (not repeated).
#[tokio::test]
#[serial]
async fn test_session_context_consumed_after_first_prompt() {
    use std::time::Duration;

    let mock_config =
        crate::registry::get_agent_config("mock-model").expect("mock-model should be registered");
    if !std::path::Path::new(&mock_config.command).exists() {
        eprintln!(
            "Skipping test: mock_acp_agent not found at {}",
            mock_config.command
        );
        return;
    }

    unsafe {
        std::env::set_var("MOCK_AGENT_ECHO_PROMPT", "1");
    }

    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
    let (backend_event_tx, mut backend_event_rx) = mpsc::channel(64);

    let mut config = build_test_config(temp_dir.path());
    config.session_context = Some("NORI_CONTEXT_ONCE".to_string());

    let backend = AcpBackend::spawn(&config, backend_event_tx)
        .await
        .expect("Failed to spawn ACP backend");

    let _ = recv_backend_control(&mut backend_event_rx, Duration::from_secs(5))
        .await
        .expect("Should receive SessionConfigured event");

    // First prompt — should include session context
    backend
        .submit(Op::UserInput {
            items: vec![codex_protocol::user_input::UserInput::Text {
                text: "first message".to_string(),
            }],
        })
        .await
        .expect("Failed to submit first user input");

    let mut first_response = String::new();
    let timeout = Duration::from_secs(10);
    let start = std::time::Instant::now();
    loop {
        if start.elapsed() > timeout {
            panic!("Timed out waiting for first PromptCompleted");
        }
        match recv_backend_client(&mut backend_event_rx, Duration::from_secs(5)).await {
            Some(nori_protocol::ClientEvent::MessageDelta(delta)) => {
                first_response.push_str(&delta.delta);
            }
            Some(nori_protocol::ClientEvent::PromptCompleted(_)) => break,
            Some(_) => continue,
            None => panic!("Channel closed"),
        }
    }

    assert!(
        first_response.contains("NORI_CONTEXT_ONCE"),
        "First prompt should contain session context, got: {first_response}"
    );

    // Second prompt — should NOT include session context
    backend
        .submit(Op::UserInput {
            items: vec![codex_protocol::user_input::UserInput::Text {
                text: "second message".to_string(),
            }],
        })
        .await
        .expect("Failed to submit second user input");

    let mut second_response = String::new();
    let start = std::time::Instant::now();
    loop {
        if start.elapsed() > timeout {
            panic!("Timed out waiting for second PromptCompleted");
        }
        match recv_backend_client(&mut backend_event_rx, Duration::from_secs(5)).await {
            Some(nori_protocol::ClientEvent::MessageDelta(delta)) => {
                second_response.push_str(&delta.delta);
            }
            Some(nori_protocol::ClientEvent::PromptCompleted(_)) => break,
            Some(_) => continue,
            None => panic!("Channel closed"),
        }
    }

    assert!(
        !second_response.contains("NORI_CONTEXT_ONCE"),
        "Second prompt should NOT contain session context, got: {second_response}"
    );
    assert!(
        second_response.contains("second message"),
        "Second prompt should contain user text, got: {second_response}"
    );

    unsafe {
        std::env::remove_var("MOCK_AGENT_ECHO_PROMPT");
    }
}
