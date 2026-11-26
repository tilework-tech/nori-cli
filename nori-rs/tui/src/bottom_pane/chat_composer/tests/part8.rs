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

#[test]
fn tab_on_a_command_draft_does_not_run_the_command_while_working() {
    for draft in ["quit", "exit", "!echo command", " /cancel "] {
        let (mut composer, _rx) = new_composer();
        composer.handle_paste(draft.to_owned());
        let (result, _) = composer
            .handle_key_event_with_queue(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE), true);
        assert_eq!(result, InputResult::None, "draft {draft}");
    }
}

fn file_searches(rx: &mut tokio::sync::mpsc::UnboundedReceiver<AppEvent>) -> Vec<String> {
    let mut queries = Vec::new();
    while let Ok(event) = rx.try_recv() {
        if let AppEvent::StartFileSearch(query) = event {
            queries.push(query);
        }
    }
    queries
}

#[test]
fn a_file_popup_stops_asking_once_answered_and_asks_again_when_reopened() {
    let (mut composer, mut rx) = new_composer();
    type_chars_humanlike(&mut composer, &['@', 'l', 'e', 'x']);
    assert_eq!(
        file_searches(&mut rx).last().map(String::as_str),
        Some("lex")
    );

    composer.on_file_search_result(
        "lex".to_string(),
        vec![codex_file_search::FileMatch {
            score: 0,
            path: "src/lexer.rs".to_string(),
            indices: None,
        }],
    );
    for code in [KeyCode::Down, KeyCode::Up] {
        composer.handle_key_event(KeyEvent::new(code, KeyModifiers::NONE));
    }
    assert_eq!(
        file_searches(&mut rx),
        Vec::<String>::new(),
        "moving through the matches asks for nothing"
    );

    for _ in 0..4 {
        composer.handle_key_event(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
    }
    let _ = file_searches(&mut rx);
    type_chars_humanlike(&mut composer, &['@', 'l', 'e', 'x']);
    let reopened = file_searches(&mut rx);
    assert_eq!(
        reopened.last().map(String::as_str),
        Some("lex"),
        "the reopened popup asks again: {reopened:?}"
    );
}

#[test]
fn a_file_popup_still_waiting_asks_again_on_the_next_key() {
    let (mut composer, mut rx) = new_composer();
    type_chars_humanlike(&mut composer, &['@', 'l', 'e', 'x']);
    let _ = file_searches(&mut rx);

    composer.handle_key_event(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));

    assert_eq!(
        file_searches(&mut rx).last().map(String::as_str),
        Some("lex"),
        "an answer that never reached the popup is asked for again"
    );
}
