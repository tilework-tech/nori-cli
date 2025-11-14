use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use insta::assert_snapshot;
use nori_cli::app::{Message, Model};
use nori_cli::ui;
use ratatui::backend::TestBackend;
use ratatui::Terminal;

/// Helper to create a KeyEvent for testing
fn key_event(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

#[test]
fn test_initial_app_renders_empty_textarea() {
    // Create a fresh app model
    let mut model = Model::default();

    // Create a test terminal with standard dimensions
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();

    // Render the app to the test terminal
    terminal
        .draw(|frame| ui::render(&mut model, frame))
        .unwrap();

    // Snapshot the rendered output
    assert_snapshot!("initial_state", terminal.backend());
}

#[test]
fn test_typing_updates_textarea() {
    // Create a fresh app model
    let mut model = Model::default();

    // Simulate typing "hi"
    model.update(Message::KeyPress(key_event(KeyCode::Char('h'))));
    model.update(Message::KeyPress(key_event(KeyCode::Char('i'))));

    // Create terminal and render
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    terminal
        .draw(|frame| ui::render(&mut model, frame))
        .unwrap();

    // Snapshot should show "hi" in the textarea
    assert_snapshot!("typed_hi", terminal.backend());
}

#[test]
fn test_long_input_wraps_properly() {
    let mut model = Model::default();

    // Type a long message that will wrap in a 40-column terminal
    let long_text = "This is a very long message that should wrap properly in a narrow terminal";
    for ch in long_text.chars() {
        model.update(Message::KeyPress(key_event(KeyCode::Char(ch))));
    }

    // Use narrow terminal to force wrapping
    let mut terminal = Terminal::new(TestBackend::new(40, 10)).unwrap();
    terminal
        .draw(|frame| ui::render(&mut model, frame))
        .unwrap();

    assert_snapshot!("long_text_wrapping", terminal.backend());
}

#[test]
fn test_empty_input_does_not_submit() {
    let mut model = Model::default();

    // Try to submit empty input (should not add to history)
    let events_before = model.response_events.len();
    model.update(Message::SubmitInput);
    let events_after = model.response_events.len();

    // No new events should be added for empty submission
    assert_eq!(
        events_before, events_after,
        "Empty input should not create events"
    );

    // UI should still show empty textarea
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    terminal
        .draw(|frame| ui::render(&mut model, frame))
        .unwrap();

    assert_snapshot!("empty_submission_prevented", terminal.backend());
}

#[test]
fn test_unicode_input_renders_correctly() {
    let mut model = Model::default();

    // Type unicode characters including emoji
    let unicode_text = "Hello 世界 👋";
    for ch in unicode_text.chars() {
        model.update(Message::KeyPress(key_event(KeyCode::Char(ch))));
    }

    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    terminal
        .draw(|frame| ui::render(&mut model, frame))
        .unwrap();

    assert_snapshot!("unicode_input", terminal.backend());
}

#[test]
fn test_multiline_input() {
    let mut model = Model::default();

    // Type text with newlines (Shift+Enter in the real UI, but we'll simulate with actual newlines)
    model.update(Message::KeyPress(key_event(KeyCode::Char('L'))));
    model.update(Message::KeyPress(key_event(KeyCode::Char('i'))));
    model.update(Message::KeyPress(key_event(KeyCode::Char('n'))));
    model.update(Message::KeyPress(key_event(KeyCode::Char('e'))));
    model.update(Message::KeyPress(key_event(KeyCode::Char(' '))));
    model.update(Message::KeyPress(key_event(KeyCode::Char('1'))));
    model.update(Message::KeyPress(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::SHIFT,
    )));
    model.update(Message::KeyPress(key_event(KeyCode::Char('L'))));
    model.update(Message::KeyPress(key_event(KeyCode::Char('i'))));
    model.update(Message::KeyPress(key_event(KeyCode::Char('n'))));
    model.update(Message::KeyPress(key_event(KeyCode::Char('e'))));
    model.update(Message::KeyPress(key_event(KeyCode::Char(' '))));
    model.update(Message::KeyPress(key_event(KeyCode::Char('2'))));

    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    terminal
        .draw(|frame| ui::render(&mut model, frame))
        .unwrap();

    assert_snapshot!("multiline_input", terminal.backend());
}
