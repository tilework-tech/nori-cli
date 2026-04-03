use super::*;
use pretty_assertions::assert_eq;

#[test]
fn edit_clears_pending_paste() {
    use crossterm::event::KeyCode;
    use crossterm::event::KeyEvent;
    use crossterm::event::KeyModifiers;

    let large = "y".repeat(LARGE_PASTE_CHAR_THRESHOLD + 1);
    let (tx, _rx) = unbounded_channel::<AppEvent>();
    let sender = AppEventSender::new(tx);
    let mut composer = ChatComposer::new(
        true,
        sender,
        false,
        "Ask Nori to do anything".to_string(),
        false,
    );

    composer.handle_paste(large);
    assert_eq!(composer.pending_pastes.len(), 1);

    // Any edit that removes the placeholder should clear pending_paste
    composer.handle_key_event(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
    assert!(composer.pending_pastes.is_empty());
}

#[test]
fn ui_snapshots() {
    use crossterm::event::KeyCode;
    use crossterm::event::KeyEvent;
    use crossterm::event::KeyModifiers;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let (tx, _rx) = unbounded_channel::<AppEvent>();
    let sender = AppEventSender::new(tx);
    let mut terminal = match Terminal::new(TestBackend::new(100, 10)) {
        Ok(t) => t,
        Err(e) => panic!("Failed to create terminal: {e}"),
    };

    let test_cases = vec![
        ("empty", None),
        ("small", Some("short".to_string())),
        ("large", Some("z".repeat(LARGE_PASTE_CHAR_THRESHOLD + 5))),
        ("multiple_pastes", None),
        ("backspace_after_pastes", None),
    ];

    for (name, input) in test_cases {
        // Create a fresh composer for each test case
        let mut composer = ChatComposer::new(
            true,
            sender.clone(),
            false,
            "Ask Nori to do anything".to_string(),
            false,
        );

        if let Some(text) = input {
            composer.handle_paste(text);
        } else if name == "multiple_pastes" {
            // First large paste
            composer.handle_paste("x".repeat(LARGE_PASTE_CHAR_THRESHOLD + 3));
            // Second large paste
            composer.handle_paste("y".repeat(LARGE_PASTE_CHAR_THRESHOLD + 7));
            // Small paste
            composer.handle_paste(" another short paste".to_string());
        } else if name == "backspace_after_pastes" {
            // Three large pastes
            composer.handle_paste("a".repeat(LARGE_PASTE_CHAR_THRESHOLD + 2));
            composer.handle_paste("b".repeat(LARGE_PASTE_CHAR_THRESHOLD + 4));
            composer.handle_paste("c".repeat(LARGE_PASTE_CHAR_THRESHOLD + 6));
            // Move cursor to end and press backspace
            composer.textarea.set_cursor(composer.textarea.text().len());
            composer.handle_key_event(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
        }

        terminal
            .draw(|f| composer.render(f.area(), f.buffer_mut()))
            .unwrap_or_else(|e| panic!("Failed to draw {name} composer: {e}"));

        insta::assert_snapshot!(name, terminal.backend());
    }
}

#[test]
fn slash_popup_model_first_for_mo_ui() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let (tx, _rx) = unbounded_channel::<AppEvent>();
    let sender = AppEventSender::new(tx);

    let mut composer = ChatComposer::new(
        true,
        sender,
        false,
        "Ask Nori to do anything".to_string(),
        false,
    );

    // Type "/mo" humanlike so paste-burst doesn’t interfere.
    type_chars_humanlike(&mut composer, &['/', 'm', 'o']);

    let mut terminal = match Terminal::new(TestBackend::new(60, 5)) {
        Ok(t) => t,
        Err(e) => panic!("Failed to create terminal: {e}"),
    };
    terminal
        .draw(|f| composer.render(f.area(), f.buffer_mut()))
        .unwrap_or_else(|e| panic!("Failed to draw composer: {e}"));

    // Visual snapshot should show the slash popup with /model as the first entry.
    insta::assert_snapshot!("slash_popup_mo", terminal.backend());
}

#[test]
fn slash_popup_model_first_for_mo_logic() {
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
    type_chars_humanlike(&mut composer, &['/', 'm', 'o']);

    match &composer.active_popup {
        ActivePopup::Command(popup) => match popup.selected_item() {
            Some(CommandItem::Builtin(cmd)) => {
                assert_eq!(cmd.command(), "model")
            }
            Some(CommandItem::UserPrompt(_) | CommandItem::AgentCommand(_)) => {
                panic!("unexpected non-builtin selected for '/mo'")
            }
            None => panic!("no selected command for '/mo'"),
        },
        _ => panic!("slash popup not active after typing '/mo'"),
    }
}

#[test]
fn slash_init_dispatches_command_and_does_not_submit_literal_text() {
    use crossterm::event::KeyCode;
    use crossterm::event::KeyEvent;
    use crossterm::event::KeyModifiers;

    let (tx, _rx) = unbounded_channel::<AppEvent>();
    let sender = AppEventSender::new(tx);
    let mut composer = ChatComposer::new(
        true,
        sender,
        false,
        "Ask Nori to do anything".to_string(),
        false,
    );

    // Type the slash command.
    type_chars_humanlike(&mut composer, &['/', 'i', 'n', 'i', 't']);

    // Press Enter to dispatch the selected command.
    let (result, _needs_redraw) =
        composer.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    // When a slash command is dispatched, the composer should return a
    // Command result (not submit literal text) and clear its textarea.
    match result {
        InputResult::Command(cmd) => {
            assert_eq!(cmd.command(), "init");
        }
        InputResult::Submitted(text) => {
            panic!("expected command dispatch, but composer submitted literal text: {text}")
        }
        InputResult::None => panic!("expected Command result for '/init'"),
    }
    assert!(composer.textarea.is_empty(), "composer should be cleared");
}

#[test]
fn extract_args_supports_quoted_paths_single_arg() {
    let args =
        extract_positional_args_for_prompt_line("/prompts:review \"docs/My File.md\"", "review");
    assert_eq!(args, vec!["docs/My File.md".to_string()]);
}

#[test]
fn extract_args_supports_mixed_quoted_and_unquoted() {
    let args =
        extract_positional_args_for_prompt_line("/prompts:cmd \"with spaces\" simple", "cmd");
    assert_eq!(args, vec!["with spaces".to_string(), "simple".to_string()]);
}

#[test]
fn slash_tab_completion_moves_cursor_to_end() {
    use crossterm::event::KeyCode;
    use crossterm::event::KeyEvent;
    use crossterm::event::KeyModifiers;

    let (tx, _rx) = unbounded_channel::<AppEvent>();
    let sender = AppEventSender::new(tx);
    let mut composer = ChatComposer::new(
        true,
        sender,
        false,
        "Ask Nori to do anything".to_string(),
        false,
    );

    // Use /di to match /diff, not /compact
    type_chars_humanlike(&mut composer, &['/', 'd', 'i']);

    let (_result, _needs_redraw) =
        composer.handle_key_event(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));

    assert_eq!(composer.textarea.text(), "/diff ");
    assert_eq!(composer.textarea.cursor(), composer.textarea.text().len());
}

#[test]
fn slash_tab_then_enter_dispatches_builtin_command() {
    let (tx, _rx) = unbounded_channel::<AppEvent>();
    let sender = AppEventSender::new(tx);
    let mut composer = ChatComposer::new(
        true,
        sender,
        false,
        "Ask Nori to do anything".to_string(),
        false,
    );

    // Type a prefix and complete with Tab, which inserts a trailing space
    // and moves the cursor beyond the '/name' token (hides the popup).
    type_chars_humanlike(&mut composer, &['/', 'd', 'i']);
    let (_res, _redraw) =
        composer.handle_key_event(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert_eq!(composer.textarea.text(), "/diff ");

    // Press Enter: should dispatch the command, not submit literal text.
    let (result, _needs_redraw) =
        composer.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    match result {
        InputResult::Command(cmd) => assert_eq!(cmd.command(), "diff"),
        InputResult::Submitted(text) => {
            panic!("expected command dispatch after Tab completion, got literal submit: {text}")
        }
        InputResult::None => panic!("expected Command result for '/diff'"),
    }
    assert!(composer.textarea.is_empty());
}

#[test]
fn slash_mention_dispatches_command_and_inserts_at() {
    use crossterm::event::KeyCode;
    use crossterm::event::KeyEvent;
    use crossterm::event::KeyModifiers;

    let (tx, _rx) = unbounded_channel::<AppEvent>();
    let sender = AppEventSender::new(tx);
    let mut composer = ChatComposer::new(
        true,
        sender,
        false,
        "Ask Nori to do anything".to_string(),
        false,
    );

    type_chars_humanlike(&mut composer, &['/', 'm', 'e', 'n', 't', 'i', 'o', 'n']);

    let (result, _needs_redraw) =
        composer.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    match result {
        InputResult::Command(cmd) => {
            assert_eq!(cmd.command(), "mention");
        }
        InputResult::Submitted(text) => {
            panic!("expected command dispatch, but composer submitted literal text: {text}")
        }
        InputResult::None => panic!("expected Command result for '/mention'"),
    }
    assert!(composer.textarea.is_empty(), "composer should be cleared");
    composer.insert_str("@");
    assert_eq!(composer.textarea.text(), "@");
}

#[test]
fn test_multiple_pastes_submission() {
    use crossterm::event::KeyCode;
    use crossterm::event::KeyEvent;
    use crossterm::event::KeyModifiers;

    let (tx, _rx) = unbounded_channel::<AppEvent>();
    let sender = AppEventSender::new(tx);
    let mut composer = ChatComposer::new(
        true,
        sender,
        false,
        "Ask Nori to do anything".to_string(),
        false,
    );

    // Define test cases: (paste content, is_large)
    let test_cases = [
        ("x".repeat(LARGE_PASTE_CHAR_THRESHOLD + 3), true),
        (" and ".to_string(), false),
        ("y".repeat(LARGE_PASTE_CHAR_THRESHOLD + 7), true),
    ];

    // Expected states after each paste
    let mut expected_text = String::new();
    let mut expected_pending_count = 0;

    // Apply all pastes and build expected state
    let states: Vec<_> = test_cases
        .iter()
        .map(|(content, is_large)| {
            composer.handle_paste(content.clone());
            if *is_large {
                let placeholder = format!("[Pasted Content {} chars]", content.chars().count());
                expected_text.push_str(&placeholder);
                expected_pending_count += 1;
            } else {
                expected_text.push_str(content);
            }
            (expected_text.clone(), expected_pending_count)
        })
        .collect();

    // Verify all intermediate states were correct
    assert_eq!(
        states,
        vec![
            (
                format!("[Pasted Content {} chars]", test_cases[0].0.chars().count()),
                1
            ),
            (
                format!(
                    "[Pasted Content {} chars] and ",
                    test_cases[0].0.chars().count()
                ),
                1
            ),
            (
                format!(
                    "[Pasted Content {} chars] and [Pasted Content {} chars]",
                    test_cases[0].0.chars().count(),
                    test_cases[2].0.chars().count()
                ),
                2
            ),
        ]
    );

    // Submit and verify final expansion
    let (result, _) = composer.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    if let InputResult::Submitted(text) = result {
        assert_eq!(text, format!("{} and {}", test_cases[0].0, test_cases[2].0));
    } else {
        panic!("expected Submitted");
    }
}

#[test]
fn test_placeholder_deletion() {
    use crossterm::event::KeyCode;
    use crossterm::event::KeyEvent;
    use crossterm::event::KeyModifiers;

    let (tx, _rx) = unbounded_channel::<AppEvent>();
    let sender = AppEventSender::new(tx);
    let mut composer = ChatComposer::new(
        true,
        sender,
        false,
        "Ask Nori to do anything".to_string(),
        false,
    );

    // Define test cases: (content, is_large)
    let test_cases = [
        ("a".repeat(LARGE_PASTE_CHAR_THRESHOLD + 5), true),
        (" and ".to_string(), false),
        ("b".repeat(LARGE_PASTE_CHAR_THRESHOLD + 6), true),
    ];

    // Apply all pastes
    let mut current_pos = 0;
    let states: Vec<_> = test_cases
        .iter()
        .map(|(content, is_large)| {
            composer.handle_paste(content.clone());
            if *is_large {
                let placeholder = format!("[Pasted Content {} chars]", content.chars().count());
                current_pos += placeholder.len();
            } else {
                current_pos += content.len();
            }
            (
                composer.textarea.text().to_string(),
                composer.pending_pastes.len(),
                current_pos,
            )
        })
        .collect();

    // Delete placeholders one by one and collect states
    let mut deletion_states = vec![];

    // First deletion
    composer.textarea.set_cursor(states[0].2);
    composer.handle_key_event(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
    deletion_states.push((
        composer.textarea.text().to_string(),
        composer.pending_pastes.len(),
    ));

    // Second deletion
    composer.textarea.set_cursor(composer.textarea.text().len());
    composer.handle_key_event(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
    deletion_states.push((
        composer.textarea.text().to_string(),
        composer.pending_pastes.len(),
    ));

    // Verify all states
    assert_eq!(
        deletion_states,
        vec![
            (" and [Pasted Content 1006 chars]".to_string(), 1),
            (" and ".to_string(), 0),
        ]
    );
}

#[test]
fn test_partial_placeholder_deletion() {
    use crossterm::event::KeyCode;
    use crossterm::event::KeyEvent;
    use crossterm::event::KeyModifiers;

    let (tx, _rx) = unbounded_channel::<AppEvent>();
    let sender = AppEventSender::new(tx);
    let mut composer = ChatComposer::new(
        true,
        sender,
        false,
        "Ask Nori to do anything".to_string(),
        false,
    );

    // Define test cases: (cursor_position_from_end, expected_pending_count)
    let test_cases = [
        5, // Delete from middle - should clear tracking
        0, // Delete from end - should clear tracking
    ];

    let paste = "x".repeat(LARGE_PASTE_CHAR_THRESHOLD + 4);
    let placeholder = format!("[Pasted Content {} chars]", paste.chars().count());

    let states: Vec<_> = test_cases
        .into_iter()
        .map(|pos_from_end| {
            composer.handle_paste(paste.clone());
            composer
                .textarea
                .set_cursor(placeholder.len() - pos_from_end);
            composer.handle_key_event(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
            let result = (
                composer.textarea.text().contains(&placeholder),
                composer.pending_pastes.len(),
            );
            composer.textarea.set_text("");
            result
        })
        .collect();

    assert_eq!(
        states,
        vec![
            (false, 0), // After deleting from middle
            (false, 0), // After deleting from end
        ]
    );
}
