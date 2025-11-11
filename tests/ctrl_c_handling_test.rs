use nori_cli::app::{Message, Model};
use std::time::Duration;

#[test]
fn test_first_ctrl_c_clears_textarea_and_shows_hint() {
    let mut model = Model::default();
    model.textarea.insert_str("some text");

    model.update(Message::ClearTextarea);

    // Textarea should be cleared
    assert!(model.textarea.lines()[0].is_empty());

    // Timestamp should be set
    assert!(model.last_ctrl_c_time.is_some());

    // Hint message should be shown
    assert_eq!(
        model.error_message,
        Some("Press Ctrl-C again to exit".to_string())
    );
}

#[test]
fn test_second_ctrl_c_within_timeout_clears_timestamp() {
    let mut model = Model::default();
    model.textarea.insert_str("text");

    // First Ctrl-C
    model.update(Message::ClearTextarea);
    assert!(model.last_ctrl_c_time.is_some());

    // Second Ctrl-C immediately (within 2 seconds)
    model.update(Message::ClearTextarea);

    // Timestamp should be None to signal quit needed
    assert!(model.last_ctrl_c_time.is_none());

    // Hint should be cleared since we're about to quit
    assert!(model.error_message.is_none());
}

#[test]
fn test_ctrl_c_after_timeout_clears_textarea_again() {
    let mut model = Model::default();

    // First Ctrl-C
    model.update(Message::ClearTextarea);
    let first_time = model.last_ctrl_c_time.unwrap();

    // Simulate 3 seconds passing by setting old timestamp
    model.last_ctrl_c_time = Some(first_time - Duration::from_secs(3));
    model.textarea.insert_str("new text");

    // Ctrl-C after timeout should behave like first press
    model.update(Message::ClearTextarea);

    // Textarea should be cleared
    assert!(model.textarea.lines()[0].is_empty());

    // Timestamp should be updated (not None)
    assert!(model.last_ctrl_c_time.is_some());

    // New timestamp should be more recent than the old one
    let new_time = model.last_ctrl_c_time.unwrap();
    assert!(new_time > first_time);

    // Hint should be shown again
    assert_eq!(
        model.error_message,
        Some("Press Ctrl-C again to exit".to_string())
    );
}
