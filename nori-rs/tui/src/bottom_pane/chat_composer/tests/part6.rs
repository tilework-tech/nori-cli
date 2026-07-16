#[test]
fn empty_vim_insert_escape_enters_normal_mode() {
    use crate::bottom_pane::textarea::VimModeState;

    let (tx, _rx) = unbounded_channel::<AppEvent>();
    let sender = AppEventSender::new(tx);
    let mut composer = ChatComposer::new(
        true,
        sender,
        false,
        "Ask Nori to do anything".to_string(),
        true,
    );
    composer.set_vim_mode(nori_config::VimEnterBehavior::Submit);

    assert_eq!(composer.vim_mode_state(), VimModeState::Insert);

    let _ = composer.handle_key_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

    assert_eq!(composer.vim_mode_state(), VimModeState::Normal);
    assert!(composer.textarea.is_empty());
}

use super::snapshot_composer_state;
use super::type_chars_humanlike;
use crate::app_event::AppEvent;
use crate::bottom_pane::AppEventSender;
use crate::bottom_pane::ChatComposer;
use crate::bottom_pane::InputResult;
use crate::render::renderable::Renderable;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use pretty_assertions::assert_eq;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;
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

fn render_composer(composer: &ChatComposer) -> Buffer {
    let area = Rect::new(0, 0, 100, 8);
    let mut buf = Buffer::empty(area);
    composer.render(area, &mut buf);
    buf
}

fn input_row(buf: &Buffer) -> String {
    (0..100)
        .map(|x| buf[(x, 1)].symbol().chars().next().unwrap_or(' '))
        .collect()
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

#[test]
fn bang_escape_exits_shell_mode_without_submitting_text() {
    let (mut composer, _rx) =
        make_composer_with_commands(vec!["$using-skills", "$writing-plans"], "codex");

    type_chars_humanlike(&mut composer, &['!']);
    assert_eq!(composer.current_text(), "!");

    let (result, _) = composer.handle_key_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

    assert!(matches!(result, InputResult::None));
    assert_eq!(composer.current_text(), "");
}

#[test]
fn shell_mode_slash_text_submits_without_slash_dispatch() {
    let (mut composer, _rx) = make_composer_with_commands(Vec::new(), "codex");

    type_chars_humanlike(&mut composer, &['!', '/', 'd', 'i', 'f', 'f']);

    let (result, _) = composer.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    match result {
        InputResult::Submitted(text) => assert_eq!(text, "!/diff"),
        other => panic!("expected shell text submission, got {other:?}"),
    }
}

#[test]
fn bang_enter_exits_shell_mode_without_submitting_empty_command() {
    let (mut composer, _rx) = make_composer_with_commands(Vec::new(), "codex");

    type_chars_humanlike(&mut composer, &['!']);

    let (result, handled) =
        composer.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert!(handled);
    assert!(matches!(result, InputResult::None));
    assert_eq!(composer.current_text(), "");

    type_chars_humanlike(&mut composer, &['x']);
    let (result, _) = composer.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    match result {
        InputResult::Submitted(text) => assert_eq!(text, "x"),
        other => panic!("expected plain text submission after shell mode exit, got {other:?}"),
    }
}

#[test]
fn bang_mid_prose_is_plain_text() {
    let (mut composer, _rx) = make_composer_with_commands(Vec::new(), "codex");

    type_chars_humanlike(&mut composer, &['U', 's', 'e', ' ', '!', 'p', 'w', 'd']);

    let (result, _) = composer.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    match result {
        InputResult::Submitted(text) => assert_eq!(text, "Use !pwd"),
        other => panic!("expected plain text submission, got {other:?}"),
    }
}

#[test]
fn fast_text_ending_with_bang_does_not_enter_shell_mode() {
    let (mut composer, _rx) = make_composer_with_commands(Vec::new(), "codex");

    for ch in "testing!!!".chars() {
        let _ = composer.handle_key_event(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
    }
    std::thread::sleep(ChatComposer::recommended_paste_flush_delay());
    let _ = composer.flush_paste_burst_if_due();

    let (result, _) = composer.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    match result {
        InputResult::Submitted(text) => assert_eq!(text, "testing!!!"),
        other => panic!("expected plain text submission, got {other:?}"),
    }
}

#[test]
fn shell_mode_renders_bang_prefix() {
    snapshot_composer_state("shell_mode_bang_prefix", false, |composer| {
        type_chars_humanlike(composer, &['!', 'p', 'w', 'd']);
    });
}

#[test]
fn shortcut_mode_prompt_uses_gray_question_mark_without_duplicate_prefix() {
    let (tx, _rx) = unbounded_channel::<AppEvent>();
    let sender = AppEventSender::new(tx);
    let mut composer = ChatComposer::new(true, sender, false, "? for shortcuts".to_string(), false);

    let _ = composer.handle_key_event(KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE));

    let buf = render_composer(&composer);
    assert_eq!(buf[(0, 1)].symbol(), "?");
    assert_eq!(buf[(0, 1)].fg, Color::DarkGray);
    assert!(
        input_row(&buf).starts_with("? for shortcuts"),
        "shortcut prompt should not duplicate the placeholder sigil: {:?}",
        input_row(&buf)
    );
}

#[test]
fn slash_mode_prompt_uses_cyan_slash_without_duplicate_prefix() {
    let (tx, _rx) = unbounded_channel::<AppEvent>();
    let sender = AppEventSender::new(tx);
    let mut composer = ChatComposer::new(
        true,
        sender,
        false,
        "Ask Nori to do anything".to_string(),
        false,
    );

    type_chars_humanlike(&mut composer, &['/', 'i', 'n', 'i', 't']);

    let buf = render_composer(&composer);
    assert_eq!(buf[(0, 1)].symbol(), "/");
    assert_eq!(buf[(0, 1)].fg, Color::Cyan);
    assert!(
        input_row(&buf).starts_with("/ init"),
        "slash prompt should hide the editable leading slash: {:?}",
        input_row(&buf)
    );
}

#[test]
fn slash_mode_prompt_stays_active_after_command_arguments() {
    let (tx, _rx) = unbounded_channel::<AppEvent>();
    let sender = AppEventSender::new(tx);
    let mut composer = ChatComposer::new(
        true,
        sender,
        false,
        "Ask Nori to do anything".to_string(),
        false,
    );

    type_chars_humanlike(
        &mut composer,
        &['/', 'm', 'o', 'd', 'e', 'l', ' ', 'm', 'o', 'c', 'k'],
    );

    let buf = render_composer(&composer);
    assert_eq!(buf[(0, 1)].symbol(), "/");
    assert_eq!(buf[(0, 1)].fg, Color::Cyan);
    assert!(
        input_row(&buf).starts_with("/ model mock"),
        "slash prompt should stay active for command arguments: {:?}",
        input_row(&buf)
    );
}

#[test]
fn builtin_goal_command_with_arguments_submits_literal_text() {
    let (tx, _rx) = unbounded_channel::<AppEvent>();
    let sender = AppEventSender::new(tx);
    let mut composer = ChatComposer::new(
        true,
        sender,
        false,
        "Ask Nori to do anything".to_string(),
        false,
    );

    type_chars_humanlike(
        &mut composer,
        &[
            '/', 'g', 'o', 'a', 'l', ' ', 'S', 'h', 'i', 'p', ' ', 'i', 't',
        ],
    );

    let (result, _needs_redraw) =
        composer.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert_eq!(InputResult::Submitted("/goal Ship it".to_string()), result);
    assert!(composer.textarea.is_empty(), "composer should be cleared");
}

#[test]
fn shell_mode_prompt_uses_red_bang_without_duplicate_prefix() {
    let (tx, _rx) = unbounded_channel::<AppEvent>();
    let sender = AppEventSender::new(tx);
    let mut composer = ChatComposer::new(
        true,
        sender,
        false,
        "Ask Nori to do anything".to_string(),
        false,
    );

    type_chars_humanlike(&mut composer, &['!', 'p', 'w', 'd']);

    let buf = render_composer(&composer);
    assert_eq!(buf[(0, 1)].symbol(), "!");
    assert_eq!(buf[(0, 1)].fg, Color::Red);
    assert!(
        input_row(&buf).starts_with("! pwd"),
        "shell prompt should render the shell sigil as the prompt character: {:?}",
        input_row(&buf)
    );
}
