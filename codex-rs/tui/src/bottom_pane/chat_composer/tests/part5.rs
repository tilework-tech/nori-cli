use super::type_chars_humanlike;
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

/// Regression test: when agent commands arrive with an empty prefix and the
/// slug is updated later, the popup should display commands with the new prefix.
/// This exercises the race condition where AvailableCommandsUpdate arrives from
/// the ACP agent before the TUI has set the agent slug.
#[test]
fn agent_commands_use_updated_prefix_after_slug_change() {
    use crate::bottom_pane::command_popup::CommandItem;

    let (tx, _rx) = unbounded_channel::<AppEvent>();
    let sender = AppEventSender::new(tx);
    let mut composer = ChatComposer::new(
        true,
        sender,
        false,
        "Ask Nori to do anything".to_string(),
        false,
    );

    // Simulate: agent commands arrive with empty prefix (slug not yet set)
    composer.set_agent_commands(
        vec![nori_protocol::AgentCommandInfo {
            name: "loop".to_string(),
            description: "loop desc".to_string(),
            input_hint: None,
        }],
        String::new(),
    );

    // Now simulate: slug arrives (agent configured)
    composer.update_agent_command_prefix("claude-code".to_string());

    // Type /claude-code:lo to trigger popup with prefix filter
    type_chars_humanlike(
        &mut composer,
        &[
            '/', 'c', 'l', 'a', 'u', 'd', 'e', '-', 'c', 'o', 'd', 'e', ':', 'l', 'o',
        ],
    );

    // The filtered items should include the agent command matching the prefix
    let has_agent_loop = composer.command_popup_items().iter().any(|item| {
        matches!(item, CommandItem::AgentCommand(i) if composer.agent_command_name(*i).as_deref() == Some("loop"))
    });
    assert!(
        has_agent_loop,
        "expected agent command 'loop' to match filter 'claude-code:lo' after prefix update"
    );
}
