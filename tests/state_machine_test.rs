use nori_cli::app::{AppMode, Message, Model};

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

#[test]
fn test_stream_chunk_accumulation() {
    let mut model = Model::default();

    model.update(Message::StreamChunk("Hello ".to_string()));
    model.update(Message::StreamChunk("World".to_string()));

    assert_eq!(model.response_text.len(), 2);
    assert_eq!(model.response_text[0], "Hello ");
    assert_eq!(model.response_text[1], "World");
}
