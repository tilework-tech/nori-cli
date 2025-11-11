use nori_cli::app::{AppMode, Message, Model};
use nori_cli::conversation::ConversationEvent;

#[test]
fn test_state_transitions() {
    let mut model = Model::default();

    // Start in Selection mode
    assert_eq!(model.current_mode, AppMode::Selection);

    // Select an item -> should close overlay
    model.update(Message::SelectItem);
    assert!(!model.show_agent_router);
    assert!(model.selected_agent_index.is_some());

    // Submit input -> should transition to Streaming mode
    model.textarea.insert_str("test input");
    model.update(Message::SubmitInput);
    assert_eq!(model.current_mode, AppMode::Streaming);

    // Stream completes -> should return to Selection mode
    model.update(Message::StreamComplete);
    assert_eq!(model.current_mode, AppMode::Selection);
}

// This test is now obsolete - replaced by test_stream_event_accumulation
// Keeping as comment for reference
// #[test]
// fn test_stream_chunk_accumulation() {
//     let mut model = Model::default();
//     model.update(Message::StreamChunk("Hello ".to_string()));
//     model.update(Message::StreamChunk("World".to_string()));
//     assert_eq!(model.response_text.len(), 2);
// }

#[test]
fn test_post_stream_state_handling() {
    let mut model = Model::default();

    // Simulate complete user flow
    model.textarea.insert_str("test input");
    model.update(Message::SubmitInput);
    assert_eq!(model.current_mode, AppMode::Streaming);

    // Simulate receiving stream events
    let event = ConversationEvent::AssistantMessage {
        text: "test response".to_string(),
    };
    model.update(Message::StreamEvent(event));

    // Stream completes - should return to Selection
    model.update(Message::StreamComplete);
    assert_eq!(model.current_mode, AppMode::Selection);

    // Verify we can still interact with chat
    model.textarea.insert_str("another message");
    model.update(Message::SubmitInput);
    assert_eq!(model.current_mode, AppMode::Streaming);
}

#[test]
fn test_stream_event_accumulation() {
    let mut model = Model::default();

    let event1 = ConversationEvent::AssistantMessage {
        text: "Hello".to_string(),
    };
    let event2 = ConversationEvent::SystemEvent {
        subtype: "init".to_string(),
        details: None,
    };

    model.update(Message::StreamEvent(event1.clone()));
    model.update(Message::StreamEvent(event2.clone()));

    assert_eq!(model.response_events.len(), 2);
    assert!(matches!(
        model.response_events[0],
        ConversationEvent::AssistantMessage { .. }
    ));
    assert!(matches!(
        model.response_events[1],
        ConversationEvent::SystemEvent { .. }
    ));
}

#[test]
fn test_toggle_agent_router_overlay() {
    let mut model = Model::default();
    assert!(!model.show_agent_router);

    model.update(Message::ToggleAgentRouter);
    assert!(model.show_agent_router);

    model.update(Message::ToggleAgentRouter);
    assert!(!model.show_agent_router);
}

#[test]
fn test_submit_input_adds_user_message_to_history() {
    let mut model = Model::default();

    model.textarea.insert_str("Hello agent");

    model.update(Message::SubmitInput);

    // Verify response_events contains UserMessage
    assert_eq!(model.response_events.len(), 1);
    match &model.response_events[0] {
        ConversationEvent::UserMessage { text } => assert_eq!(text, "Hello agent"),
        _ => panic!("Expected UserMessage"),
    }
}

#[test]
fn test_show_install_prompt_message() {
    let mut model = Model::default();

    assert!(!model.show_install_prompt);

    model.update(Message::ShowInstallPrompt {
        backend: "Claude Code".to_string(),
        url: "https://code.claude.com".to_string(),
        install_cmd: None,
    });

    assert!(model.show_install_prompt);
    assert_eq!(
        model.install_prompt_backend,
        Some("Claude Code".to_string())
    );
    assert_eq!(
        model.install_prompt_url,
        Some("https://code.claude.com".to_string())
    );
}

#[test]
fn test_cancel_install_prompt() {
    let mut model = Model::default();

    // Show the prompt first
    model.update(Message::ShowInstallPrompt {
        backend: "Claude Code".to_string(),
        url: "https://code.claude.com".to_string(),
        install_cmd: None,
    });
    assert!(model.show_install_prompt);

    // Cancel should hide it
    model.update(Message::CancelInstall);
    assert!(!model.show_install_prompt);
    assert_eq!(model.install_prompt_backend, None);
    assert_eq!(model.install_prompt_url, None);
}

#[test]
fn test_cancel_stream_during_streaming() {
    use tokio_util::sync::CancellationToken;

    let mut model = Model::default();

    // Set up streaming state with a cancellation token
    model.current_mode = AppMode::Streaming;
    let token = CancellationToken::new();
    model.current_stream_token = Some(token.clone());
    model.textarea.insert_str("test prompt");

    // Add a user message to simulate stream start
    model.response_events.push(ConversationEvent::UserMessage {
        text: "test prompt".to_string(),
    });

    // Send cancel message (simulates user pressing Escape)
    model.update(Message::CancelStream);

    // Verify state transitions correctly
    assert_eq!(model.current_mode, AppMode::Selection);
    assert!(model.current_stream_token.is_none());
    assert!(model.textarea.lines()[0].is_empty());
    assert!(token.is_cancelled());

    // Verify cancellation event was added to history
    let last_event = model.response_events.last();
    assert!(matches!(
        last_event,
        Some(ConversationEvent::StreamCancelled)
    ));
}

#[test]
fn test_navigate_install_choice_with_install_cmd() {
    use nori_cli::app::InstallChoice;

    let mut model = Model::default();

    model.update(Message::ShowInstallPrompt {
        backend: "Claude Code".to_string(),
        url: "https://code.claude.com".to_string(),
        install_cmd: Some(vec!["npm".to_string(), "install".to_string()]),
    });

    // Should start at RunInstallation
    assert_eq!(model.install_prompt_choice, InstallChoice::RunInstallation);

    model.update(Message::NavigateInstallChoice);
    assert_eq!(model.install_prompt_choice, InstallChoice::OpenInstallPage);

    model.update(Message::NavigateInstallChoice);
    assert_eq!(model.install_prompt_choice, InstallChoice::Cancel);

    model.update(Message::NavigateInstallChoice);
    // Should wrap around to first
    assert_eq!(model.install_prompt_choice, InstallChoice::RunInstallation);
}
