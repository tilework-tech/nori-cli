use super::*;
use crate::slash_command::SlashCommand;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use pretty_assertions::assert_eq;

fn new_composer() -> (ChatComposer, tokio::sync::mpsc::UnboundedReceiver<AppEvent>) {
    let (tx, rx) = unbounded_channel::<AppEvent>();
    let sender = AppEventSender::new(tx);
    let composer = ChatComposer::new(
        true,
        sender,
        false,
        "Ask Nori to do anything".to_string(),
        false,
    );
    (composer, rx)
}

#[test]
fn bare_exit_dispatches_exit_command() {
    let (mut composer, _rx) = new_composer();

    type_chars_humanlike(&mut composer, &['e', 'x', 'i', 't']);
    let (result, _needs_redraw) =
        composer.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert_eq!(InputResult::Command(SlashCommand::Exit), result);
}

#[test]
fn bare_quit_dispatches_quit_command() {
    let (mut composer, _rx) = new_composer();

    type_chars_humanlike(&mut composer, &['q', 'u', 'i', 't']);
    let (result, _needs_redraw) =
        composer.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert_eq!(InputResult::Command(SlashCommand::Quit), result);
}

#[test]
fn exit_with_trailing_space_dispatches_exit_command() {
    let (mut composer, _rx) = new_composer();

    type_chars_humanlike(&mut composer, &['e', 'x', 'i', 't', ' ']);
    let (result, _needs_redraw) =
        composer.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert_eq!(InputResult::Command(SlashCommand::Exit), result);
}

#[test]
fn leading_space_exit_submits_literal_text() {
    let (mut composer, _rx) = new_composer();

    type_chars_humanlike(&mut composer, &[' ', 'e', 'x', 'i', 't']);
    let (result, _needs_redraw) =
        composer.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert_eq!(InputResult::Submitted("exit".to_string()), result);
}

#[test]
fn exit_with_trailing_text_submits_literal_text() {
    let (mut composer, _rx) = new_composer();

    type_chars_humanlike(&mut composer, &['e', 'x', 'i', 't', ' ', 'n', 'o', 'w']);
    let (result, _needs_redraw) =
        composer.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert_eq!(InputResult::Submitted("exit now".to_string()), result);
}

#[test]
fn exiting_word_submits_literal_text() {
    let (mut composer, _rx) = new_composer();

    type_chars_humanlike(&mut composer, &['e', 'x', 'i', 't', 'i', 'n', 'g']);
    let (result, _needs_redraw) =
        composer.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert_eq!(InputResult::Submitted("exiting".to_string()), result);
}

#[test]
fn uppercase_exit_submits_literal_text() {
    let (mut composer, _rx) = new_composer();

    type_chars_humanlike(&mut composer, &['E', 'x', 'i', 't']);
    let (result, _needs_redraw) =
        composer.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert_eq!(InputResult::Submitted("Exit".to_string()), result);
}
