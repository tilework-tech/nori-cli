use super::*;
use pretty_assertions::assert_eq;

#[test]
fn custom_prompt_missing_required_args_reports_error() {
    let (tx, mut rx) = unbounded_channel::<AppEvent>();
    let sender = AppEventSender::new(tx);
    let mut composer = ChatComposer::new(
        true,
        sender,
        false,
        "Ask Codex to do anything".to_string(),
        false,
    );

    composer.set_custom_prompts(vec![CustomPrompt {
        name: "my-prompt".to_string(),
        path: "/tmp/my-prompt.md".to_string().into(),
        content: "Review $USER changes on $BRANCH".to_string(),
        description: None,
        argument_hint: None,
        kind: Default::default(),
    }]);

    // Provide only one of the required args
    composer.textarea.set_text("/prompts:my-prompt USER=Alice");

    let (result, _needs_redraw) =
        composer.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert_eq!(InputResult::None, result);
    assert_eq!("/prompts:my-prompt USER=Alice", composer.textarea.text());

    let mut found_error = false;
    while let Ok(event) = rx.try_recv() {
        if let AppEvent::InsertHistoryCell(cell) = event {
            let message = cell
                .display_lines(80)
                .into_iter()
                .map(|line| line.to_string())
                .collect::<Vec<_>>()
                .join("\n");
            assert!(message.to_lowercase().contains("missing required args"));
            assert!(message.contains("BRANCH"));
            found_error = true;
            break;
        }
    }
    assert!(
        found_error,
        "expected missing args error history cell to be sent"
    );
}

#[test]
fn selecting_custom_prompt_with_args_expands_placeholders() {
    // Support $1..$9 and $ARGUMENTS in prompt content.
    let prompt_text = "Header: $1\nArgs: $ARGUMENTS\nNinth: $9\n";

    let (tx, _rx) = unbounded_channel::<AppEvent>();
    let sender = AppEventSender::new(tx);
    let mut composer = ChatComposer::new(
        true,
        sender,
        false,
        "Ask Codex to do anything".to_string(),
        false,
    );

    composer.set_custom_prompts(vec![CustomPrompt {
        name: "my-prompt".to_string(),
        path: "/tmp/my-prompt.md".to_string().into(),
        content: prompt_text.to_string(),
        description: None,
        argument_hint: None,
        kind: Default::default(),
    }]);

    // Type the slash command with two args and hit Enter to submit.
    type_chars_humanlike(
        &mut composer,
        &[
            '/', 'p', 'r', 'o', 'm', 'p', 't', 's', ':', 'm', 'y', '-', 'p', 'r', 'o', 'm', 'p',
            't', ' ', 'f', 'o', 'o', ' ', 'b', 'a', 'r',
        ],
    );
    let (result, _needs_redraw) =
        composer.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    let expected = "Header: foo\nArgs: foo bar\nNinth: \n".to_string();
    assert_eq!(InputResult::Submitted(expected), result);
}

#[test]
fn numeric_prompt_positional_args_does_not_error() {
    // Ensure that a prompt with only numeric placeholders does not trigger
    // key=value parsing errors when given positional arguments.
    let (tx, _rx) = unbounded_channel::<AppEvent>();
    let sender = AppEventSender::new(tx);
    let mut composer = ChatComposer::new(
        true,
        sender,
        false,
        "Ask Codex to do anything".to_string(),
        false,
    );

    composer.set_custom_prompts(vec![CustomPrompt {
        name: "elegant".to_string(),
        path: "/tmp/elegant.md".to_string().into(),
        content: "Echo: $ARGUMENTS".to_string(),
        description: None,
        argument_hint: None,
        kind: Default::default(),
    }]);

    // Type positional args; should submit with numeric expansion, no errors.
    composer.textarea.set_text("/prompts:elegant hi");
    let (result, _needs_redraw) =
        composer.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert_eq!(InputResult::Submitted("Echo: hi".to_string()), result);
    assert!(composer.textarea.is_empty());
}

#[test]
fn selecting_custom_prompt_with_no_args_inserts_template() {
    let prompt_text = "X:$1 Y:$2 All:[$ARGUMENTS]";

    let (tx, _rx) = unbounded_channel::<AppEvent>();
    let sender = AppEventSender::new(tx);
    let mut composer = ChatComposer::new(
        true,
        sender,
        false,
        "Ask Codex to do anything".to_string(),
        false,
    );

    composer.set_custom_prompts(vec![CustomPrompt {
        name: "p".to_string(),
        path: "/tmp/p.md".to_string().into(),
        content: prompt_text.to_string(),
        description: None,
        argument_hint: None,
        kind: Default::default(),
    }]);

    type_chars_humanlike(
        &mut composer,
        &['/', 'p', 'r', 'o', 'm', 'p', 't', 's', ':', 'p'],
    );
    let (result, _needs_redraw) =
        composer.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    // With no args typed, selecting the prompt inserts the command template
    // and does not submit immediately.
    assert_eq!(InputResult::None, result);
    assert_eq!("/prompts:p ", composer.textarea.text());
}

#[test]
fn selecting_custom_prompt_preserves_literal_dollar_dollar() {
    // '$$' should remain untouched.
    let prompt_text = "Cost: $$ and first: $1";

    let (tx, _rx) = unbounded_channel::<AppEvent>();
    let sender = AppEventSender::new(tx);
    let mut composer = ChatComposer::new(
        true,
        sender,
        false,
        "Ask Codex to do anything".to_string(),
        false,
    );

    composer.set_custom_prompts(vec![CustomPrompt {
        name: "price".to_string(),
        path: "/tmp/price.md".to_string().into(),
        content: prompt_text.to_string(),
        description: None,
        argument_hint: None,
        kind: Default::default(),
    }]);

    type_chars_humanlike(
        &mut composer,
        &[
            '/', 'p', 'r', 'o', 'm', 'p', 't', 's', ':', 'p', 'r', 'i', 'c', 'e', ' ', 'x',
        ],
    );
    let (result, _needs_redraw) =
        composer.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert_eq!(
        InputResult::Submitted("Cost: $$ and first: x".to_string()),
        result
    );
}

#[test]
fn selecting_custom_prompt_reuses_cached_arguments_join() {
    let prompt_text = "First: $ARGUMENTS\nSecond: $ARGUMENTS";

    let (tx, _rx) = unbounded_channel::<AppEvent>();
    let sender = AppEventSender::new(tx);
    let mut composer = ChatComposer::new(
        true,
        sender,
        false,
        "Ask Codex to do anything".to_string(),
        false,
    );

    composer.set_custom_prompts(vec![CustomPrompt {
        name: "repeat".to_string(),
        path: "/tmp/repeat.md".to_string().into(),
        content: prompt_text.to_string(),
        description: None,
        argument_hint: None,
        kind: Default::default(),
    }]);

    type_chars_humanlike(
        &mut composer,
        &[
            '/', 'p', 'r', 'o', 'm', 'p', 't', 's', ':', 'r', 'e', 'p', 'e', 'a', 't', ' ', 'o',
            'n', 'e', ' ', 't', 'w', 'o',
        ],
    );
    let (result, _needs_redraw) =
        composer.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    let expected = "First: one two\nSecond: one two".to_string();
    assert_eq!(InputResult::Submitted(expected), result);
}

#[test]
fn burst_paste_fast_small_buffers_and_flushes_on_stop() {
    use crossterm::event::KeyCode;
    use crossterm::event::KeyEvent;
    use crossterm::event::KeyModifiers;

    let (tx, _rx) = unbounded_channel::<AppEvent>();
    let sender = AppEventSender::new(tx);
    let mut composer = ChatComposer::new(
        true,
        sender,
        false,
        "Ask Codex to do anything".to_string(),
        false,
    );

    let count = 32;
    for _ in 0..count {
        let _ = composer.handle_key_event(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
        assert!(
            composer.is_in_paste_burst(),
            "expected active paste burst during fast typing"
        );
        assert!(
            composer.textarea.text().is_empty(),
            "text should not appear during burst"
        );
    }

    assert!(
        composer.textarea.text().is_empty(),
        "text should remain empty until flush"
    );
    std::thread::sleep(ChatComposer::recommended_paste_flush_delay());
    let flushed = composer.flush_paste_burst_if_due();
    assert!(flushed, "expected buffered text to flush after stop");
    assert_eq!(composer.textarea.text(), "a".repeat(count));
    assert!(
        composer.pending_pastes.is_empty(),
        "no placeholder for small burst"
    );
}

#[test]
fn burst_paste_fast_large_inserts_placeholder_on_flush() {
    use crossterm::event::KeyCode;
    use crossterm::event::KeyEvent;
    use crossterm::event::KeyModifiers;

    let (tx, _rx) = unbounded_channel::<AppEvent>();
    let sender = AppEventSender::new(tx);
    let mut composer = ChatComposer::new(
        true,
        sender,
        false,
        "Ask Codex to do anything".to_string(),
        false,
    );

    let count = LARGE_PASTE_CHAR_THRESHOLD + 1; // > threshold to trigger placeholder
    for _ in 0..count {
        let _ = composer.handle_key_event(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
    }

    // Nothing should appear until we stop and flush
    assert!(composer.textarea.text().is_empty());
    std::thread::sleep(ChatComposer::recommended_paste_flush_delay());
    let flushed = composer.flush_paste_burst_if_due();
    assert!(flushed, "expected flush after stopping fast input");

    let expected_placeholder = format!("[Pasted Content {count} chars]");
    assert_eq!(composer.textarea.text(), expected_placeholder);
    assert_eq!(composer.pending_pastes.len(), 1);
    assert_eq!(composer.pending_pastes[0].0, expected_placeholder);
    assert_eq!(composer.pending_pastes[0].1.len(), count);
    assert!(composer.pending_pastes[0].1.chars().all(|c| c == 'x'));
}

#[test]
fn humanlike_typing_1000_chars_appears_live_no_placeholder() {
    let (tx, _rx) = unbounded_channel::<AppEvent>();
    let sender = AppEventSender::new(tx);
    let mut composer = ChatComposer::new(
        true,
        sender,
        false,
        "Ask Codex to do anything".to_string(),
        false,
    );

    let count = LARGE_PASTE_CHAR_THRESHOLD; // 1000 in current config
    let chars: Vec<char> = vec!['z'; count];
    type_chars_humanlike(&mut composer, &chars);

    assert_eq!(composer.textarea.text(), "z".repeat(count));
    assert!(composer.pending_pastes.is_empty());
}

#[test]
fn vim_mode_escape_enters_normal_mode_with_content() {
    use crate::bottom_pane::textarea::VimModeState;

    let (tx, _rx) = unbounded_channel::<AppEvent>();
    let sender = AppEventSender::new(tx);
    let mut composer = ChatComposer::new(
        true,
        sender,
        false,
        "Ask Codex to do anything".to_string(),
        true, // disable_paste_burst to avoid timing issues
    );

    // Enable vim mode
    composer.set_vim_mode_enabled(true);

    // Verify we start in Insert mode
    assert_eq!(composer.vim_mode_state(), VimModeState::Insert);

    // Type some text
    composer.insert_str("hello");
    assert_eq!(composer.current_text(), "hello");

    // Press Escape - should enter Normal mode
    let _ = composer.handle_key_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

    // Verify we're now in Normal mode
    assert_eq!(
        composer.vim_mode_state(),
        VimModeState::Normal,
        "Escape should transition from Insert to Normal mode when textarea has content"
    );
}

#[test]
fn vim_mode_hjkl_navigation_in_normal_mode() {
    use crate::bottom_pane::textarea::VimModeState;

    let (tx, _rx) = unbounded_channel::<AppEvent>();
    let sender = AppEventSender::new(tx);
    let mut composer = ChatComposer::new(
        true,
        sender,
        false,
        "Ask Codex to do anything".to_string(),
        true,
    );

    composer.set_vim_mode_enabled(true);
    composer.insert_str("hello");

    // Enter Normal mode
    let _ = composer.handle_key_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert_eq!(composer.vim_mode_state(), VimModeState::Normal);

    // Get cursor position (should be at end after typing)
    let cursor_before = composer.textarea.cursor();

    // Press 'h' to move left
    let _ = composer.handle_key_event(KeyEvent::new(KeyCode::Char('h'), KeyModifiers::NONE));

    let cursor_after = composer.textarea.cursor();

    // Cursor should have moved left (only if we weren't already at position 0)
    if cursor_before > 0 {
        assert!(
            cursor_after < cursor_before,
            "h in Normal mode should move cursor left: before={cursor_before}, after={cursor_after}"
        );
    }

    // Press 'i' to return to Insert mode
    let _ = composer.handle_key_event(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE));
    assert_eq!(
        composer.vim_mode_state(),
        VimModeState::Insert,
        "'i' should return to Insert mode"
    );
}

#[test]
fn test_ctrl_r_opens_history_search_popup() {
    let (tx, _rx) = unbounded_channel::<AppEvent>();
    let sender = AppEventSender::new(tx);
    let mut composer = ChatComposer::new(true, sender, false, "Ask a question".to_string(), true);

    composer.handle_key_event(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL));

    assert!(
        matches!(composer.active_popup, ActivePopup::HistorySearch(_)),
        "Ctrl+R should open the history search popup"
    );
}

#[test]
fn test_ctrl_r_history_search_escape_closes_popup() {
    let (tx, _rx) = unbounded_channel::<AppEvent>();
    let sender = AppEventSender::new(tx);
    let mut composer = ChatComposer::new(true, sender, false, "Ask a question".to_string(), true);

    // Open history search popup with Ctrl+R
    composer.handle_key_event(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL));
    assert!(
        matches!(composer.active_popup, ActivePopup::HistorySearch(_)),
        "Ctrl+R should open the history search popup"
    );

    // Press Escape to close
    composer.handle_key_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(
        matches!(composer.active_popup, ActivePopup::None),
        "Escape should close the history search popup"
    );
}

#[test]
fn test_ctrl_r_history_search_enter_selects_and_closes() {
    use codex_protocol::message_history::HistoryEntry;

    let (tx, _rx) = unbounded_channel::<AppEvent>();
    let sender = AppEventSender::new(tx);
    let mut composer = ChatComposer::new(true, sender, false, "Ask a question".to_string(), true);

    // Open history search popup with Ctrl+R
    composer.handle_key_event(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL));

    // Populate the popup with entries
    if let ActivePopup::HistorySearch(popup) = &mut composer.active_popup {
        popup.set_entries(vec![
            HistoryEntry {
                conversation_id: "sess-1".to_string(),
                ts: 1,
                text: "first entry".to_string(),
            },
            HistoryEntry {
                conversation_id: "sess-2".to_string(),
                ts: 2,
                text: "second entry".to_string(),
            },
        ]);
    } else {
        panic!("Expected HistorySearch popup to be active after Ctrl+R");
    }

    // Press Enter to select the current entry and close the popup
    composer.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert!(
        matches!(composer.active_popup, ActivePopup::None),
        "Enter should close the history search popup"
    );
    // The selected entry text should be placed in the composer.
    let text = composer.current_text();
    assert_eq!(
        text, "first entry",
        "Composer should contain the first (default-selected) history entry"
    );
}
