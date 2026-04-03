use super::type_chars_humanlike;
use crate::app_event::AppEvent;
use crate::bottom_pane::AppEventSender;
use crate::bottom_pane::ChatComposer;
use crate::bottom_pane::InputResult;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use tokio::sync::mpsc::unbounded_channel;

fn make_composer_with_agent_commands()
-> (ChatComposer, tokio::sync::mpsc::UnboundedReceiver<AppEvent>) {
    let (tx, rx) = unbounded_channel::<AppEvent>();
    let sender = AppEventSender::new(tx);
    let mut composer = ChatComposer::new(
        true,
        sender,
        false,
        "Ask Nori to do anything".to_string(),
        true,
    );
    composer.set_agent_commands(
        vec![
            nori_protocol::AgentCommandInfo {
                name: "loop".to_string(),
                description: "Run a command on a recurring interval".to_string(),
                input_hint: None,
            },
            nori_protocol::AgentCommandInfo {
                name: "schedule".to_string(),
                description: "Schedule a remote agent".to_string(),
                input_hint: None,
            },
        ],
        "claude-code".to_string(),
    );
    (composer, rx)
}

#[test]
fn agent_command_tab_completion_uses_prefix() {
    let (mut composer, _rx) = make_composer_with_agent_commands();

    // Type /claude-code:lo to uniquely match the agent command "loop"
    type_chars_humanlike(
        &mut composer,
        &[
            '/', 'c', 'l', 'a', 'u', 'd', 'e', '-', 'c', 'o', 'd', 'e', ':', 'l', 'o',
        ],
    );

    // Press Tab to complete
    let (_result, _) = composer.handle_key_event(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));

    // Should insert the prefixed form, not just "/loop "
    assert_eq!(
        composer.textarea.text(),
        "/claude-code:loop ",
        "Tab completion should insert the fully-qualified agent command name with prefix"
    );
    assert_eq!(
        composer.textarea.cursor(),
        composer.textarea.text().len(),
        "Cursor should be at the end after tab completion"
    );
}

#[test]
fn agent_command_with_args_submits_successfully() {
    let (mut composer, _rx) = make_composer_with_agent_commands();

    // Type the full prefixed command with args
    type_chars_humanlike(
        &mut composer,
        &[
            '/', 'c', 'l', 'a', 'u', 'd', 'e', '-', 'c', 'o', 'd', 'e', ':', 'l', 'o', 'o', 'p',
            ' ', '5', 'm', ' ', 'h', 'i',
        ],
    );

    // Press Enter to submit
    let (result, _) = composer.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    match result {
        InputResult::Submitted(text) => {
            assert_eq!(text, "/claude-code:loop 5m hi");
        }
        other => panic!("Expected Submitted for agent command with args, got: {other:?}"),
    }
}

#[test]
fn bare_agent_command_name_without_prefix_is_unrecognized() {
    let (mut composer, mut rx) = make_composer_with_agent_commands();

    // Type bare /loop (no prefix) with args
    type_chars_humanlike(&mut composer, &['/', 'l', 'o', 'o', 'p', ' ', '5', 'm']);

    // Press Escape to dismiss the popup first, then press Enter
    let _ = composer.handle_key_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    let (result, _) = composer.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    // Should NOT submit — should show unrecognized error
    match result {
        InputResult::None => {}
        other => panic!("Expected None (unrecognized command) for bare '/loop 5m', got: {other:?}"),
    }

    // Verify an error event was sent to the channel
    match rx.try_recv() {
        Ok(AppEvent::InsertHistoryCell(_)) => {}
        other => {
            panic!("Expected InsertHistoryCell event for unrecognized command, got: {other:?}")
        }
    }
}
