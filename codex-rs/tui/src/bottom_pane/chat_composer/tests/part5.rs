use crate::app_event::AppEvent;
use crate::bottom_pane::AppEventSender;
use crate::bottom_pane::ChatComposer;
use crate::bottom_pane::InputResult;
use crate::bottom_pane::textarea::VimModeState;
use codex_acp::config::VimEnterBehavior;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use tokio::sync::mpsc::unbounded_channel;

fn make_composer() -> ChatComposer {
    let (tx, _rx) = unbounded_channel::<AppEvent>();
    let sender = AppEventSender::new(tx);
    ChatComposer::new(
        true,
        sender,
        false,
        "Ask Nori to do anything".to_string(),
        true, // disable_paste_burst
    )
}

#[test]
fn vim_enter_newline_insert_mode_inserts_newline() {
    let mut composer = make_composer();
    composer.set_vim_mode(VimEnterBehavior::Newline);

    assert_eq!(composer.vim_mode_state(), VimModeState::Insert);

    composer.insert_str("hello");
    let (result, _) = composer.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert_eq!(
        result,
        InputResult::None,
        "INSERT Enter with Newline behavior should not submit"
    );
    assert_eq!(
        composer.current_text(),
        "hello\n",
        "INSERT Enter with Newline behavior should insert newline"
    );
}

#[test]
fn vim_enter_newline_normal_mode_submits() {
    let mut composer = make_composer();
    composer.set_vim_mode(VimEnterBehavior::Newline);

    composer.insert_str("hello");

    // Esc to enter Normal mode
    let _ = composer.handle_key_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert_eq!(composer.vim_mode_state(), VimModeState::Normal);

    let (result, _) = composer.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    match result {
        InputResult::Submitted(text) => assert_eq!(text, "hello"),
        other => panic!("NORMAL Enter with Newline behavior should submit, got: {other:?}"),
    }
}

#[test]
fn vim_enter_submit_insert_mode_submits() {
    let mut composer = make_composer();
    composer.set_vim_mode(VimEnterBehavior::Submit);

    assert_eq!(composer.vim_mode_state(), VimModeState::Insert);

    composer.insert_str("hello");
    let (result, _) = composer.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    match result {
        InputResult::Submitted(text) => assert_eq!(text, "hello"),
        other => panic!("INSERT Enter with Submit behavior should submit, got: {other:?}"),
    }
}

#[test]
fn vim_enter_submit_normal_mode_inserts_newline() {
    let mut composer = make_composer();
    composer.set_vim_mode(VimEnterBehavior::Submit);

    composer.insert_str("hello");

    // Esc to enter Normal mode
    let _ = composer.handle_key_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert_eq!(composer.vim_mode_state(), VimModeState::Normal);

    let (result, _) = composer.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(
        result,
        InputResult::None,
        "NORMAL Enter with Submit behavior should not submit"
    );
    assert!(
        composer.current_text().contains('\n'),
        "NORMAL Enter with Submit behavior should insert newline, got: {:?}",
        composer.current_text()
    );
}

#[test]
fn vim_disabled_enter_always_submits() {
    let mut composer = make_composer();
    // vim mode is disabled by default

    composer.insert_str("hello");
    let (result, _) = composer.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    match result {
        InputResult::Submitted(text) => assert_eq!(text, "hello"),
        other => panic!("Enter with vim disabled should submit, got: {other:?}"),
    }
}
