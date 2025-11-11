use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use nori_cli::app::{Message, Model};

#[test]
fn test_textarea_wraps_long_input() {
    let mut model = Model::default();

    // Set a narrow wrap width
    model.set_wrap_width(20);

    // Type a long line
    let long_text = "This is a very long line that should wrap at word boundaries";
    for ch in long_text.chars() {
        let key_event = KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE);
        model.update(Message::KeyPress(key_event));
    }

    // The textarea should have multiple lines after wrapping
    let lines = model.textarea.lines();
    assert!(
        lines.len() > 1,
        "Long line should be wrapped into multiple lines"
    );

    // Each line should be <= wrap width
    for line in lines {
        assert!(line.len() <= 20, "Line '{}' exceeds wrap width", line);
    }
}

#[test]
fn test_textarea_preserves_user_newlines() {
    let mut model = Model::default();
    model.set_wrap_width(80);

    // Type "Line 1" + Enter + "Line 2"
    for ch in "Line 1".chars() {
        let key_event = KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE);
        model.update(Message::KeyPress(key_event));
    }

    let enter_event = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
    model.update(Message::KeyPress(enter_event));

    for ch in "Line 2".chars() {
        let key_event = KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE);
        model.update(Message::KeyPress(key_event));
    }

    let lines = model.textarea.lines();
    assert_eq!(lines.len(), 2);
}

#[test]
fn test_textarea_rewraps_on_width_change() {
    let mut model = Model::default();

    // Type a medium line at wide width
    model.set_wrap_width(80);
    let text = "This is a medium length line";
    for ch in text.chars() {
        let key_event = KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE);
        model.update(Message::KeyPress(key_event));
    }

    // Should be one line at wide width
    assert_eq!(model.textarea.lines().len(), 1);

    // Change to narrow width
    model.set_wrap_width(15);

    // Should now be wrapped into multiple lines
    assert!(model.textarea.lines().len() > 1);
}
