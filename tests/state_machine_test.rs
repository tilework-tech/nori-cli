use nori_cli::app::{AppMode, Message, Model};
use nori_cli::conversation::ConversationEvent;

#[test]
fn test_state_transitions() {
    let mut model = Model::default();

    // Start in Selection mode
    assert_eq!(model.current_mode, AppMode::Selection);

    // Select an item -> should transition to Input mode
    model.update(Message::SelectItem);
    assert_eq!(model.current_mode, AppMode::Input);

    // Submit input -> should transition to Streaming mode
    model.update(Message::SubmitInput);
    assert_eq!(model.current_mode, AppMode::Streaming);

    // Stream completes -> should return to Selection mode
    model.update(Message::StreamComplete);
    assert_eq!(model.current_mode, AppMode::Selection);

    // Exit from Input mode with Esc -> back to Selection
    model.update(Message::SelectItem);
    assert_eq!(model.current_mode, AppMode::Input);
    model.update(Message::ExitInputMode);
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
    model.update(Message::SelectItem);
    assert_eq!(model.current_mode, AppMode::Input);

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

    // Verify we can interact again - this verifies the state machine
    // The actual bug is in the event handler, not the Model
    model.update(Message::NextItem);
    assert_eq!(model.current_mode, AppMode::Selection);

    // Verify we can select again
    model.update(Message::SelectItem);
    assert_eq!(model.current_mode, AppMode::Input);
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
