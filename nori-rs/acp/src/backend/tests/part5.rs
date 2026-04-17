use super::*;

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

    // Configure mock agent to echo back the full prompt text.
    // SAFETY: Test-scoped environment variable for mock agent behavior.
    unsafe {
        std::env::set_var("MOCK_AGENT_ECHO_PROMPT", "1");
    }

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

    // Clean up env var
    unsafe {
        std::env::remove_var("MOCK_AGENT_ECHO_PROMPT");
    }
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
