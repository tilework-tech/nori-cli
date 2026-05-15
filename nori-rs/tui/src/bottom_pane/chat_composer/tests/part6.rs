use super::snapshot_composer_state;
use super::type_chars_humanlike;
use crate::app_event::AppEvent;
use crate::bottom_pane::AppEventSender;
use crate::bottom_pane::ChatComposer;
use crate::bottom_pane::InputResult;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use pretty_assertions::assert_eq;
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

fn make_composer_with_commands(
    commands: Vec<&str>,
    prefix: &str,
) -> (ChatComposer, tokio::sync::mpsc::UnboundedReceiver<AppEvent>) {
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
        commands
            .into_iter()
            .map(|name| nori_protocol::AgentCommandInfo {
                name: name.to_string(),
                description: format!("Description for {name}"),
                input_hint: None,
            })
            .collect(),
        prefix.to_string(),
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
fn agent_command_with_args_strips_prefix_on_submit() {
    let (mut composer, _rx) = make_composer_with_agent_commands();

    // Type the full prefixed command with args
    type_chars_humanlike(
        &mut composer,
        &[
            '/', 'c', 'l', 'a', 'u', 'd', 'e', '-', 'c', 'o', 'd', 'e', ':', 'l', 'o', 'o', 'p',
            ' ', '5', 'm', ' ', 'h', 'i',
        ],
    );

    // Press Escape to dismiss popup, then Enter to submit
    let _ = composer.handle_key_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    let (result, _) = composer.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    // The agent should see "/loop 5m hi", not "/claude-code:loop 5m hi"
    match result {
        InputResult::Submitted(text) => {
            assert_eq!(text, "/loop 5m hi");
        }
        other => panic!("Expected Submitted for agent command with args, got: {other:?}"),
    }
}

#[test]
fn agent_command_without_args_strips_prefix_on_submit() {
    let (mut composer, _rx) = make_composer_with_agent_commands();

    // Type the full prefixed command without args
    type_chars_humanlike(
        &mut composer,
        &[
            '/', 'c', 'l', 'a', 'u', 'd', 'e', '-', 'c', 'o', 'd', 'e', ':', 's', 'c', 'h', 'e',
            'd', 'u', 'l', 'e',
        ],
    );

    // Press Escape to dismiss popup, then Enter to submit
    let _ = composer.handle_key_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    let (result, _) = composer.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    // The agent should see "/schedule", not "/claude-code:schedule"
    match result {
        InputResult::Submitted(text) => {
            assert_eq!(text, "/schedule");
        }
        other => panic!("Expected Submitted for agent command, got: {other:?}"),
    }
}

#[test]
fn agent_command_popup_selection_strips_prefix() {
    let (mut composer, _rx) = make_composer_with_agent_commands();

    // Type enough to filter to "loop" in the popup
    type_chars_humanlike(
        &mut composer,
        &[
            '/', 'c', 'l', 'a', 'u', 'd', 'e', '-', 'c', 'o', 'd', 'e', ':', 'l', 'o',
        ],
    );

    // Navigate to the agent command in the popup and press Enter to select+submit
    // The popup should be open; press Down to move to the agent command if needed,
    // then Enter to submit.
    let (result, _) = composer.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    // The agent should see "/loop", not "/claude-code:loop"
    match result {
        InputResult::Submitted(text) => {
            assert_eq!(text, "/loop");
        }
        other => panic!("Expected Submitted for popup agent command selection, got: {other:?}"),
    }
}

#[test]
fn builtin_command_is_not_affected_by_prefix_stripping() {
    let (mut composer, _rx) = make_composer_with_agent_commands();

    // Type a builtin command
    type_chars_humanlike(&mut composer, &['/', 'c', 'o', 'm', 'p', 'a', 'c', 't']);

    // Press Escape to dismiss popup, then Enter to submit
    let _ = composer.handle_key_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    let (result, _) = composer.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    // Builtin should be dispatched as a command, not submitted as text
    match result {
        InputResult::Command(_) => {}
        other => panic!("Builtin /compact should be dispatched as Command, got: {other:?}"),
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

#[test]
fn dollar_prefixed_agent_command_enter_inserts_native_skill_form() {
    let (mut composer, _rx) =
        make_composer_with_commands(vec!["$using-skills", "$writing-plans"], "codex");

    type_chars_humanlike(&mut composer, &['$', 'u', 's', 'i', 'n', 'g']);

    let (result, _) = composer.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert!(matches!(result, InputResult::None));
    assert_eq!(composer.current_text(), "$using-skills");
}

#[test]
fn accepted_dollar_skill_does_not_reopen_picker_before_submit() {
    let (mut composer, _rx) =
        make_composer_with_commands(vec!["$using-skills", "$writing-plans"], "codex");

    type_chars_humanlike(&mut composer, &['$', 'u', 's', 'i', 'n', 'g']);
    let (insert_result, _) =
        composer.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    let (submit_result, _) =
        composer.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert!(matches!(insert_result, InputResult::None));
    match submit_result {
        InputResult::Submitted(text) => assert_eq!(text, "$using-skills"),
        other => panic!("expected accepted skill text to submit on next Enter, got {other:?}"),
    }
}

#[test]
fn dollar_skill_picker_fuzzy_matches_de_sigiled_name() {
    let (mut composer, _rx) = make_composer_with_commands(
        vec![
            "$test-driven-development",
            "$finishing-a-development-branch",
        ],
        "codex",
    );

    type_chars_humanlike(&mut composer, &['$', 'f', 'i', 'n', 'd']);

    let (result, _) = composer.handle_key_event(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));

    assert!(matches!(result, InputResult::None));
    assert_eq!(composer.current_text(), "$finishing-a-development-branch");
}

#[test]
fn dollar_skill_picker_replaces_codex_skill_token_mid_prose() {
    let (mut composer, _rx) =
        make_composer_with_commands(vec!["$using-skills", "$writing-plans"], "codex");

    type_chars_humanlike(
        &mut composer,
        &[
            'U', 's', 'e', ' ', '$', 'w', 'r', 'i', 't', 'i', 'n', 'g', ' ', 't', 'o', ' ', 'r',
            'e', 'f', 'i', 'n', 'e',
        ],
    );
    for _ in 0.." to refine".len() {
        let _ = composer.handle_key_event(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
    }

    let (result, _) = composer.handle_key_event(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));

    assert!(matches!(result, InputResult::None));
    assert_eq!(composer.current_text(), "Use $writing-plans to refine");
}

#[test]
fn claude_dollar_skill_picker_inserts_prefixed_slash_form_at_prompt_start() {
    let (mut composer, _rx) =
        make_composer_with_commands(vec!["loop", "schedule", "status"], "claude-code");

    type_chars_humanlike(&mut composer, &['$', 'l', 'o']);

    let (result, _) = composer.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert!(matches!(result, InputResult::None));
    assert_eq!(composer.current_text(), "/claude-code:loop ");
}

#[test]
fn claude_dollar_skill_picker_is_not_available_mid_prose() {
    let (mut composer, _rx) = make_composer_with_commands(vec!["loop"], "claude-code");

    type_chars_humanlike(
        &mut composer,
        &['U', 's', 'e', ' ', '$', 'l', 'o', ' ', 'n', 'o', 'w'],
    );

    let (result, _) = composer.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    match result {
        InputResult::Submitted(text) => assert_eq!(text, "Use $lo now"),
        other => panic!("expected mid-prose Claude skill text to submit literally, got {other:?}"),
    }
}

#[test]
fn claude_dollar_skill_picker_is_not_available_after_newline() {
    let (mut composer, _rx) = make_composer_with_commands(vec!["loop"], "claude-code");

    type_chars_humanlike(
        &mut composer,
        &['h', 'e', 'l', 'l', 'o', '\n', '$', 'l', 'o'],
    );

    let (result, _) = composer.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    match result {
        InputResult::Submitted(text) => assert_eq!(text, "hello\n$lo"),
        other => panic!("expected multiline Claude skill text to submit literally, got {other:?}"),
    }
}

#[test]
fn pasted_dollar_skill_token_opens_skill_picker() {
    let (mut composer, _rx) =
        make_composer_with_commands(vec!["$using-skills", "$writing-plans"], "codex");

    composer.handle_paste("$w".to_string());

    let (result, _) = composer.handle_key_event(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));

    assert!(matches!(result, InputResult::None));
    assert_eq!(composer.current_text(), "$writing-plans");
}

#[test]
fn dollar_skill_popup_renders_de_sigiled_names() {
    snapshot_composer_state("dollar_skill_popup", false, |composer| {
        composer.set_agent_commands(
            vec![
                nori_protocol::AgentCommandInfo {
                    name: "$using-skills".to_string(),
                    description: "Use skill instructions".to_string(),
                    input_hint: None,
                },
                nori_protocol::AgentCommandInfo {
                    name: "$writing-plans".to_string(),
                    description: "Write an implementation plan".to_string(),
                    input_hint: None,
                },
            ],
            "codex".to_string(),
        );
        type_chars_humanlike(composer, &['$', 'w']);
    });
}
