use crate::app_event::AppEvent;
use crate::bottom_pane::AppEventSender;
use crate::bottom_pane::ChatComposer;
use crate::bottom_pane::InputResult;
use crate::bottom_pane::textarea::VimModeState;
use crate::slash_command::SlashCommand;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use crossterm::event::KeyModifiers;
use nori_config::VimEnterBehavior;
use pretty_assertions::assert_eq;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use tokio::sync::mpsc::unbounded_channel;

use crate::render::renderable::Renderable;

fn make_composer() -> ChatComposer {
    let (tx, _rx) = unbounded_channel::<AppEvent>();
    ChatComposer::new(
        true,
        AppEventSender::new(tx),
        true,
        "Ask Nori to do anything".to_string(),
        true,
    )
}

fn make_composer_with_paste_bursts() -> ChatComposer {
    let (tx, _rx) = unbounded_channel::<AppEvent>();
    ChatComposer::new(
        true,
        AppEventSender::new(tx),
        true,
        "Ask Nori to do anything".to_string(),
        false,
    )
}

fn make_composer_with_skills() -> ChatComposer {
    let mut composer = make_composer();
    composer.set_agent_commands(
        vec![nori_protocol::AgentCommandInfo {
            name: "$using-skills".to_string(),
            description: "Use skill instructions".to_string(),
            input_hint: None,
        }],
        "codex".to_string(),
    );
    composer
}

fn press(composer: &mut ChatComposer, code: KeyCode, modifiers: KeyModifiers) -> InputResult {
    composer.handle_key_event(KeyEvent::new(code, modifiers)).0
}

fn type_char(composer: &mut ChatComposer, ch: char) {
    let _ = press(composer, KeyCode::Char(ch), KeyModifiers::NONE);
    std::thread::sleep(ChatComposer::recommended_paste_flush_delay());
    let _ = composer.flush_paste_burst_if_due();
}

fn always_submit() -> VimEnterBehavior {
    *VimEnterBehavior::all_variants()
        .iter()
        .find(|behavior| behavior.toml_value() == "always_submit")
        .expect("always-submit Vim behavior should be available")
}

fn enter_normal(composer: &mut ChatComposer) {
    let result = press(composer, KeyCode::Esc, KeyModifiers::NONE);
    assert_eq!(result, InputResult::None);
    assert_eq!(composer.vim_mode_state(), VimModeState::Normal);
}

fn rendered_text(composer: &ChatComposer) -> String {
    let area = Rect::new(0, 0, 100, 16);
    let mut buffer = Buffer::empty(area);
    composer.render(area, &mut buffer);
    buffer
        .content
        .iter()
        .map(ratatui::buffer::Cell::symbol)
        .collect::<Vec<_>>()
        .join("")
}

#[test]
fn vim_always_submit_submits_from_insert_and_normal() {
    for start_normal in [false, true] {
        let mut composer = make_composer();
        composer.set_vim_mode(always_submit());
        composer.insert_str("hello");
        if start_normal {
            enter_normal(&mut composer);
        }

        assert_eq!(
            press(&mut composer, KeyCode::Enter, KeyModifiers::NONE),
            InputResult::Submitted("hello".to_string())
        );
        assert_eq!(composer.vim_mode_state(), VimModeState::Normal);
    }
}

#[test]
fn vim_newline_shortcuts_insert_in_both_modes_and_preserve_mode() {
    let shortcuts = [
        (KeyCode::Enter, KeyModifiers::SHIFT),
        (KeyCode::Enter, KeyModifiers::ALT),
        (KeyCode::Char('j'), KeyModifiers::CONTROL),
    ];

    for start_normal in [false, true] {
        for (code, modifiers) in shortcuts {
            let mut composer = make_composer();
            composer.set_vim_mode(always_submit());
            composer.insert_str("hello");
            if start_normal {
                enter_normal(&mut composer);
            }
            let expected_mode = composer.vim_mode_state();

            assert_eq!(press(&mut composer, code, modifiers), InputResult::None);
            assert!(composer.current_text().contains('\n'));
            assert_eq!(composer.vim_mode_state(), expected_mode);
        }
    }
}

#[test]
fn vim_newline_shortcut_release_does_not_insert_twice() {
    let mut composer = make_composer();
    composer.set_vim_mode(always_submit());
    composer.insert_str("hello");

    assert_eq!(
        composer
            .handle_key_event(KeyEvent::new_with_kind(
                KeyCode::Enter,
                KeyModifiers::SHIFT,
                KeyEventKind::Release,
            ))
            .0,
        InputResult::None
    );
    assert_eq!(composer.current_text(), "hello");
}

#[test]
fn vim_newline_shortcut_preserves_buffered_paste_order() {
    let mut composer = make_composer_with_paste_bursts();
    composer.set_vim_mode(always_submit());
    for ch in "hello".chars() {
        let _ = press(&mut composer, KeyCode::Char(ch), KeyModifiers::NONE);
    }
    assert!(composer.is_in_paste_burst());

    assert_eq!(
        press(&mut composer, KeyCode::Enter, KeyModifiers::SHIFT),
        InputResult::None
    );
    std::thread::sleep(ChatComposer::recommended_paste_flush_delay());
    let _ = composer.flush_paste_burst_if_due();

    assert_eq!(composer.current_text(), "hello\n");
}

#[test]
fn vim_newline_shortcut_clears_pending_normal_operator() {
    let mut composer = make_composer();
    composer.set_vim_mode(always_submit());
    composer.insert_str("hello");
    enter_normal(&mut composer);
    let _ = press(&mut composer, KeyCode::Char('d'), KeyModifiers::NONE);
    assert!(composer.is_vim_operator_pending());

    assert_eq!(
        press(&mut composer, KeyCode::Enter, KeyModifiers::SHIFT),
        InputResult::None
    );

    assert!(!composer.is_vim_operator_pending());
    assert_eq!(composer.current_text(), "hell\no");
}

#[test]
fn empty_normal_slash_opens_picker_in_insert_mode() {
    let mut composer = make_composer();
    composer.set_vim_mode(always_submit());
    enter_normal(&mut composer);

    assert_eq!(
        press(&mut composer, KeyCode::Char('/'), KeyModifiers::NONE),
        InputResult::None
    );

    assert_eq!(composer.current_text(), "/");
    assert_eq!(composer.vim_mode_state(), VimModeState::Insert);
    assert!(composer.popup_active());
}

#[test]
fn slash_picker_escape_enters_normal_then_closes() {
    let mut composer = make_composer();
    composer.set_vim_mode(always_submit());
    enter_normal(&mut composer);
    let _ = press(&mut composer, KeyCode::Char('/'), KeyModifiers::NONE);

    assert_eq!(
        press(&mut composer, KeyCode::Esc, KeyModifiers::NONE),
        InputResult::None
    );
    assert_eq!(composer.vim_mode_state(), VimModeState::Normal);
    assert!(composer.popup_active());

    assert_eq!(
        press(&mut composer, KeyCode::Esc, KeyModifiers::NONE),
        InputResult::None
    );
    assert_eq!(composer.vim_mode_state(), VimModeState::Normal);
    assert!(!composer.popup_active());
    assert_eq!(composer.current_text(), "/");
}

#[test]
fn slash_picker_normal_j_navigates_before_enter_selection() {
    let mut composer = make_composer();
    composer.set_vim_mode(always_submit());
    enter_normal(&mut composer);
    let _ = press(&mut composer, KeyCode::Char('/'), KeyModifiers::NONE);
    let _ = press(&mut composer, KeyCode::Esc, KeyModifiers::NONE);

    let _ = press(&mut composer, KeyCode::Char('j'), KeyModifiers::NONE);
    assert_eq!(
        press(&mut composer, KeyCode::Enter, KeyModifiers::NONE),
        InputResult::Command(SlashCommand::Model)
    );
}

#[test]
fn slash_picker_normal_i_returns_to_insert_without_closing() {
    let mut composer = make_composer();
    composer.set_vim_mode(always_submit());
    enter_normal(&mut composer);
    let _ = press(&mut composer, KeyCode::Char('/'), KeyModifiers::NONE);
    let _ = press(&mut composer, KeyCode::Esc, KeyModifiers::NONE);
    let _ = press(&mut composer, KeyCode::Char('i'), KeyModifiers::NONE);
    assert_eq!(composer.vim_mode_state(), VimModeState::Insert);
    assert!(composer.popup_active());
}

#[test]
fn picker_enter_precedes_always_submit_and_modified_enter_inserts_newline() {
    let mut composer = make_composer();
    composer.set_vim_mode(always_submit());
    type_char(&mut composer, '/');

    assert_eq!(
        press(&mut composer, KeyCode::Enter, KeyModifiers::NONE),
        InputResult::Command(SlashCommand::Agent)
    );

    type_char(&mut composer, '/');
    assert_eq!(
        press(&mut composer, KeyCode::Enter, KeyModifiers::SHIFT),
        InputResult::None
    );
    assert_eq!(composer.current_text(), "/\n");
    assert!(!composer.popup_active());
}

#[test]
fn history_search_keeps_ownership_of_modified_enter() {
    let mut composer = make_composer();
    composer.set_vim_mode(always_submit());
    composer.insert_str("draft");
    let _ = press(&mut composer, KeyCode::Char('r'), KeyModifiers::CONTROL);
    assert!(composer.popup_active());

    assert_eq!(
        press(&mut composer, KeyCode::Enter, KeyModifiers::SHIFT),
        InputResult::None
    );
    assert!(!composer.popup_active());
    assert_eq!(composer.current_text(), "draft");
}

#[test]
fn empty_normal_bang_enters_insert_and_empty_normal_escape_exits_shell_mode() {
    let mut composer = make_composer();
    composer.set_vim_mode(always_submit());
    enter_normal(&mut composer);

    let _ = press(&mut composer, KeyCode::Char('!'), KeyModifiers::NONE);
    assert_eq!(composer.current_text(), "!");
    assert_eq!(composer.vim_mode_state(), VimModeState::Insert);

    let _ = press(&mut composer, KeyCode::Esc, KeyModifiers::NONE);
    assert_eq!(composer.current_text(), "!");
    assert_eq!(composer.vim_mode_state(), VimModeState::Normal);

    let _ = press(&mut composer, KeyCode::Esc, KeyModifiers::NONE);
    assert_eq!(composer.current_text(), "");
    assert_eq!(composer.vim_mode_state(), VimModeState::Normal);
}

#[test]
fn empty_normal_question_toggles_shortcuts_without_entering_insert() {
    let mut composer = make_composer();
    composer.set_vim_mode(always_submit());
    enter_normal(&mut composer);

    assert_eq!(
        press(&mut composer, KeyCode::Char('?'), KeyModifiers::NONE),
        InputResult::None
    );
    assert_eq!(composer.current_text(), "");
    assert_eq!(composer.vim_mode_state(), VimModeState::Normal);
    assert!(rendered_text(&composer).contains("/ for commands"));
}

#[test]
fn file_and_skill_pickers_close_in_normal_and_reopen_in_insert() {
    for (mut composer, sigil, suffix) in [
        (make_composer(), '@', ""),
        (make_composer_with_skills(), '$', "u"),
    ] {
        composer.set_vim_mode(always_submit());
        type_char(&mut composer, sigil);
        for ch in suffix.chars() {
            type_char(&mut composer, ch);
        }
        assert!(composer.popup_active());

        let _ = press(&mut composer, KeyCode::Esc, KeyModifiers::NONE);
        assert_eq!(composer.vim_mode_state(), VimModeState::Normal);
        assert!(!composer.popup_active());

        let _ = press(&mut composer, KeyCode::Char('i'), KeyModifiers::NONE);
        assert_eq!(composer.vim_mode_state(), VimModeState::Insert);
        assert!(composer.popup_active());
    }
}
