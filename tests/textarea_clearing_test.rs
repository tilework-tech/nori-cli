use nori_cli::app::{AppMode, Message, Model};
use nori_cli::conversation::ConversationEvent;

#[test]
fn test_textarea_clears_immediately_on_submit() {
    let mut model = Model::default();

    // User types a message
    model.textarea.insert_str("test message");
    assert!(!model.textarea.lines()[0].is_empty());

    // User submits
    model.update(Message::SubmitInput);

    // Textarea should be cleared immediately
    assert!(model.textarea.lines()[0].is_empty(),
        "Textarea should be empty after SubmitInput");

    // Mode should transition to Streaming
    assert_eq!(model.current_mode, AppMode::Streaming);
}

#[test]
fn test_empty_input_does_not_submit() {
    let mut model = Model::default();

    // Start with empty textarea
    assert!(model.textarea.lines()[0].is_empty());

    // Try to submit
    model.update(Message::SubmitInput);

    // Should stay in Selection mode
    assert_eq!(model.current_mode, AppMode::Selection);

    // No events should be added
    assert_eq!(model.response_events.len(), 0);
}

#[test]
fn test_whitespace_only_input_does_not_submit() {
    let mut model = Model::default();

    // User types only whitespace
    model.textarea.insert_str("   \n  ");

    // Try to submit
    model.update(Message::SubmitInput);

    // Should stay in Selection mode
    assert_eq!(model.current_mode, AppMode::Selection);

    // No events should be added
    assert_eq!(model.response_events.len(), 0);
}

#[test]
fn test_textarea_stays_clear_during_streaming() {
    let mut model = Model::default();

    // Submit a message
    model.textarea.insert_str("test");
    model.update(Message::SubmitInput);
    assert!(model.textarea.lines()[0].is_empty());

    // Receive stream events
    model.update(Message::StreamEvent(ConversationEvent::AssistantMessage {
        text: "response".to_string(),
    }));

    // Textarea should still be empty
    assert!(model.textarea.lines()[0].is_empty(),
        "Textarea should remain empty during streaming");
}

#[test]
fn test_textarea_stays_clear_after_stream_complete() {
    let mut model = Model::default();

    // Submit and clear
    model.textarea.insert_str("test");
    model.update(Message::SubmitInput);
    assert!(model.textarea.lines()[0].is_empty());

    // Stream completes
    model.update(Message::StreamComplete);

    // Textarea should still be empty
    assert!(model.textarea.lines()[0].is_empty(),
        "Textarea should remain empty after StreamComplete");

    // Should be back in Selection mode
    assert_eq!(model.current_mode, AppMode::Selection);
}

#[test]
fn test_textarea_stays_clear_after_cancel() {
    use tokio_util::sync::CancellationToken;

    let mut model = Model::default();

    // Submit and clear
    model.textarea.insert_str("test message");
    model.update(Message::SubmitInput);
    assert!(model.textarea.lines()[0].is_empty());

    // Set up cancellation token for streaming
    model.current_stream_token = Some(CancellationToken::new());

    // Cancel stream
    model.update(Message::CancelStream);

    // Textarea should still be empty (not restored)
    assert!(model.textarea.lines()[0].is_empty(),
        "Textarea should remain empty after cancellation");

    // Should be back in Selection mode
    assert_eq!(model.current_mode, AppMode::Selection);
}
