use nori_cli::app::{Model, Message};

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
