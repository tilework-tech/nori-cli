use super::*;
use pretty_assertions::assert_eq;

#[test]
fn shortcut_hint_is_placeholder_only_until_shortcut_overlay_opens() {
    let (tx, _rx) = unbounded_channel::<AppEvent>();
    let sender = AppEventSender::new(tx);
    let composer = ChatComposer::new(true, sender, false, "? for shortcuts".to_string(), false);

    let area = Rect::new(0, 0, 40, 6);
    let mut buf = Buffer::empty(area);
    composer.render(area, &mut buf);

    let row_to_string = |y: u16| {
        let mut row = String::new();
        for x in 0..area.width {
            row.push(buf[(x, y)].symbol().chars().next().unwrap_or(' '));
        }
        row
    };

    let input_row = row_to_string(1);
    assert!(
        input_row.contains("? for shortcuts"),
        "expected shortcut hint in the empty composer placeholder: {input_row:?}",
    );

    let footer_rows = (3..area.height).map(row_to_string).collect::<Vec<_>>();
    assert_eq!(
        footer_rows
            .iter()
            .filter(|row| row.contains("? for shortcuts"))
            .count(),
        0,
        "shortcut hint should not render below the textarea: {footer_rows:?}",
    );
}

#[test]
fn shortcut_overlay_still_renders_below_prompt_when_requested() {
    use crossterm::event::KeyCode;
    use crossterm::event::KeyEvent;
    use crossterm::event::KeyModifiers;

    let (tx, _rx) = unbounded_channel::<AppEvent>();
    let sender = AppEventSender::new(tx);
    let mut composer = ChatComposer::new(true, sender, false, "? for shortcuts".to_string(), false);
    let _ = composer.handle_key_event(KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE));

    let area = Rect::new(0, 0, 80, 10);
    let mut buf = Buffer::empty(area);
    composer.render(area, &mut buf);

    let rendered = (0..area.height)
        .map(|y| {
            (0..area.width)
                .map(|x| buf[(x, y)].symbol().chars().next().unwrap_or(' '))
                .collect::<String>()
        })
        .collect::<Vec<_>>();

    assert!(
        rendered.iter().any(|row| row.contains("newline")),
        "shortcut overlay should still render below the prompt: {rendered:?}",
    );
}

#[test]
fn footer_mode_snapshots() {
    use crossterm::event::KeyCode;
    use crossterm::event::KeyEvent;
    use crossterm::event::KeyModifiers;

    snapshot_composer_state("footer_mode_shortcut_overlay", true, |composer| {
        composer.set_esc_backtrack_hint(true);
        let _ = composer.handle_key_event(KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE));
    });

    snapshot_composer_state("footer_mode_ctrl_c_quit", true, |composer| {
        composer.set_ctrl_c_quit_hint(true, true);
    });

    snapshot_composer_state("footer_mode_ctrl_c_interrupt", true, |composer| {
        composer.set_task_running(true);
        composer.set_ctrl_c_quit_hint(true, true);
    });

    snapshot_composer_state("footer_mode_ctrl_c_then_esc_hint", true, |composer| {
        composer.set_ctrl_c_quit_hint(true, true);
        let _ = composer.handle_key_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    });

    snapshot_composer_state("footer_mode_esc_hint_from_overlay", true, |composer| {
        let _ = composer.handle_key_event(KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE));
        let _ = composer.handle_key_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    });

    snapshot_composer_state("footer_mode_esc_hint_backtrack", true, |composer| {
        composer.set_esc_backtrack_hint(true);
        let _ = composer.handle_key_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    });

    snapshot_composer_state(
        "footer_mode_overlay_then_external_esc_hint",
        true,
        |composer| {
            let _ =
                composer.handle_key_event(KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE));
            composer.set_esc_backtrack_hint(true);
        },
    );

    snapshot_composer_state("footer_mode_hidden_while_typing", true, |composer| {
        type_chars_humanlike(composer, &['h']);
    });
}

#[test]
fn esc_hint_stays_hidden_with_draft_content() {
    use crossterm::event::KeyCode;
    use crossterm::event::KeyEvent;
    use crossterm::event::KeyModifiers;

    let (tx, _rx) = unbounded_channel::<AppEvent>();
    let sender = AppEventSender::new(tx);
    let mut composer = ChatComposer::new(
        true,
        sender,
        true,
        "Ask Nori to do anything".to_string(),
        false,
    );

    type_chars_humanlike(&mut composer, &['d']);

    assert!(!composer.is_empty());
    assert_eq!(composer.current_text(), "d");
    assert_eq!(composer.footer_mode, FooterMode::ShortcutSummary);
    assert!(matches!(composer.active_popup, ActivePopup::None));

    let _ = composer.handle_key_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

    assert_eq!(composer.footer_mode, FooterMode::ShortcutSummary);
    assert!(!composer.esc_backtrack_hint);
}

#[test]
fn clear_for_ctrl_c_records_cleared_draft() {
    let (tx, _rx) = unbounded_channel::<AppEvent>();
    let sender = AppEventSender::new(tx);
    let mut composer = ChatComposer::new(
        true,
        sender,
        false,
        "Ask Nori to do anything".to_string(),
        false,
    );

    composer.set_text_content("draft text".to_string());
    assert_eq!(composer.clear_for_ctrl_c(), Some("draft text".to_string()));
    assert!(composer.is_empty());

    assert_eq!(
        composer.history.navigate_up(&composer.app_event_tx),
        Some("draft text".to_string())
    );
}

#[test]
fn question_mark_only_toggles_on_first_char() {
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

    let (result, needs_redraw) =
        composer.handle_key_event(KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE));
    assert_eq!(result, InputResult::None);
    assert!(needs_redraw, "toggling overlay should request redraw");
    assert_eq!(composer.footer_mode, FooterMode::ShortcutOverlay);

    // Toggle back to prompt mode so subsequent typing captures characters.
    let _ = composer.handle_key_event(KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE));
    assert_eq!(composer.footer_mode, FooterMode::ShortcutSummary);

    type_chars_humanlike(&mut composer, &['h']);
    assert_eq!(composer.textarea.text(), "h");
    assert_eq!(composer.footer_mode(), FooterMode::ContextOnly);

    let (result, needs_redraw) =
        composer.handle_key_event(KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE));
    assert_eq!(result, InputResult::None);
    assert!(needs_redraw, "typing should still mark the view dirty");
    std::thread::sleep(ChatComposer::recommended_paste_flush_delay());
    let _ = composer.flush_paste_burst_if_due();
    assert_eq!(composer.textarea.text(), "h?");
    assert_eq!(composer.footer_mode, FooterMode::ShortcutSummary);
    assert_eq!(composer.footer_mode(), FooterMode::ContextOnly);
}

#[test]
fn shortcut_overlay_persists_while_task_running() {
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

    let _ = composer.handle_key_event(KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE));
    assert_eq!(composer.footer_mode, FooterMode::ShortcutOverlay);

    composer.set_task_running(true);

    assert_eq!(composer.footer_mode, FooterMode::ShortcutOverlay);
    assert_eq!(composer.footer_mode(), FooterMode::ShortcutOverlay);
}

#[test]
fn test_current_at_token_basic_cases() {
    let test_cases = vec![
        // Valid @ tokens
        ("@hello", 3, Some("hello".to_string()), "Basic ASCII token"),
        (
            "@file.txt",
            4,
            Some("file.txt".to_string()),
            "ASCII with extension",
        ),
        (
            "hello @world test",
            8,
            Some("world".to_string()),
            "ASCII token in middle",
        ),
        (
            "@test123",
            5,
            Some("test123".to_string()),
            "ASCII with numbers",
        ),
        // Unicode examples
        ("@İstanbul", 3, Some("İstanbul".to_string()), "Turkish text"),
        (
            "@testЙЦУ.rs",
            8,
            Some("testЙЦУ.rs".to_string()),
            "Mixed ASCII and Cyrillic",
        ),
        ("@诶", 2, Some("诶".to_string()), "Chinese character"),
        ("@👍", 2, Some("👍".to_string()), "Emoji token"),
        // Invalid cases (should return None)
        ("hello", 2, None, "No @ symbol"),
        (
            "@",
            1,
            Some("".to_string()),
            "Only @ symbol triggers empty query",
        ),
        ("@ hello", 2, None, "@ followed by space"),
        ("test @ world", 6, None, "@ with spaces around"),
    ];

    for (input, cursor_pos, expected, description) in test_cases {
        let mut textarea = TextArea::new();
        textarea.insert_str(input);
        textarea.set_cursor(cursor_pos);

        let result = ChatComposer::current_at_token(&textarea);
        assert_eq!(
            result, expected,
            "Failed for case: {description} - input: '{input}', cursor: {cursor_pos}"
        );
    }
}

#[test]
fn test_current_at_token_cursor_positions() {
    let test_cases = vec![
        // Different cursor positions within a token
        ("@test", 0, Some("test".to_string()), "Cursor at @"),
        ("@test", 1, Some("test".to_string()), "Cursor after @"),
        ("@test", 5, Some("test".to_string()), "Cursor at end"),
        // Multiple tokens - cursor determines which token
        ("@file1 @file2", 0, Some("file1".to_string()), "First token"),
        (
            "@file1 @file2",
            8,
            Some("file2".to_string()),
            "Second token",
        ),
        // Edge cases
        ("@", 0, Some("".to_string()), "Only @ symbol"),
        ("@a", 2, Some("a".to_string()), "Single character after @"),
        ("", 0, None, "Empty input"),
    ];

    for (input, cursor_pos, expected, description) in test_cases {
        let mut textarea = TextArea::new();
        textarea.insert_str(input);
        textarea.set_cursor(cursor_pos);

        let result = ChatComposer::current_at_token(&textarea);
        assert_eq!(
            result, expected,
            "Failed for cursor position case: {description} - input: '{input}', cursor: {cursor_pos}",
        );
    }
}

#[test]
fn test_current_at_token_whitespace_boundaries() {
    let test_cases = vec![
        // Space boundaries
        (
            "aaa@aaa",
            4,
            None,
            "Connected @ token - no completion by design",
        ),
        (
            "aaa @aaa",
            5,
            Some("aaa".to_string()),
            "@ token after space",
        ),
        (
            "test @file.txt",
            7,
            Some("file.txt".to_string()),
            "@ token after space",
        ),
        // Full-width space boundaries
        (
            "test　@İstanbul",
            8,
            Some("İstanbul".to_string()),
            "@ token after full-width space",
        ),
        (
            "@ЙЦУ　@诶",
            10,
            Some("诶".to_string()),
            "Full-width space between Unicode tokens",
        ),
        // Tab and newline boundaries
        (
            "test\t@file",
            6,
            Some("file".to_string()),
            "@ token after tab",
        ),
    ];

    for (input, cursor_pos, expected, description) in test_cases {
        let mut textarea = TextArea::new();
        textarea.insert_str(input);
        textarea.set_cursor(cursor_pos);

        let result = ChatComposer::current_at_token(&textarea);
        assert_eq!(
            result, expected,
            "Failed for whitespace boundary case: {description} - input: '{input}', cursor: {cursor_pos}",
        );
    }
}

#[test]
fn ascii_prefix_survives_non_ascii_followup() {
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

    let _ = composer.handle_key_event(KeyEvent::new(KeyCode::Char('1'), KeyModifiers::NONE));
    assert!(composer.is_in_paste_burst());

    let _ = composer.handle_key_event(KeyEvent::new(KeyCode::Char('あ'), KeyModifiers::NONE));

    let (result, _) = composer.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    match result {
        InputResult::Submitted(text) => assert_eq!(text, "1あ"),
        _ => panic!("expected Submitted"),
    }
}

#[test]
fn handle_paste_small_inserts_text() {
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

    let needs_redraw = composer.handle_paste("hello".to_string());
    assert!(needs_redraw);
    assert_eq!(composer.textarea.text(), "hello");
    assert!(composer.pending_pastes.is_empty());

    let (result, _) = composer.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    match result {
        InputResult::Submitted(text) => assert_eq!(text, "hello"),
        _ => panic!("expected Submitted"),
    }
}

#[test]
fn empty_enter_returns_none() {
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

    // Ensure composer is empty and press Enter.
    assert!(composer.textarea.text().is_empty());
    let (result, _needs_redraw) =
        composer.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    match result {
        InputResult::None => {}
        other => panic!("expected None for empty enter, got: {other:?}"),
    }
}

#[test]
fn handle_paste_large_uses_placeholder_and_replaces_on_submit() {
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

    let large = "x".repeat(LARGE_PASTE_CHAR_THRESHOLD + 10);
    let needs_redraw = composer.handle_paste(large.clone());
    assert!(needs_redraw);
    let placeholder = format!("[Pasted Content {} chars]", large.chars().count());
    assert_eq!(composer.textarea.text(), placeholder);
    assert_eq!(composer.pending_pastes.len(), 1);
    assert_eq!(composer.pending_pastes[0].0, placeholder);
    assert_eq!(composer.pending_pastes[0].1, large);

    let (result, _) = composer.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    match result {
        InputResult::Submitted(text) => assert_eq!(text, large),
        _ => panic!("expected Submitted"),
    }
    assert!(composer.pending_pastes.is_empty());
}

#[test]
fn paste_normalizes_newlines_and_sanitizes_terminal_controls() {
    let (tx, _rx) = unbounded_channel::<AppEvent>();
    let sender = AppEventSender::new(tx);
    let mut composer = ChatComposer::new(
        true,
        sender,
        false,
        "Ask Nori to do anything".to_string(),
        false,
    );

    composer.handle_paste("one\r\ntwo\rthree\t_count_r\x1b[13;2:3uows\n\0four\u{7f}".to_string());

    assert_eq!(
        composer.current_text(),
        "one\ntwo\nthree\t_count_rows\nfour"
    );
}

#[test]
fn equal_length_large_pastes_expand_in_order_on_submit() {
    let (tx, _rx) = unbounded_channel::<AppEvent>();
    let sender = AppEventSender::new(tx);
    let mut composer = ChatComposer::new(
        true,
        sender,
        false,
        "Ask Nori to do anything".to_string(),
        false,
    );
    let first = "a".repeat(LARGE_PASTE_CHAR_THRESHOLD + 10);
    let second = "b".repeat(LARGE_PASTE_CHAR_THRESHOLD + 10);
    let base = format!("[Pasted Content {} chars]", first.chars().count());

    composer.handle_paste(first.clone());
    composer.handle_paste(second.clone());

    assert_eq!(composer.current_text(), format!("{base}{base} #2"));
    let (result, _) = composer.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(result, InputResult::Submitted(format!("{first}{second}")));
}

#[test]
fn equal_length_large_pastes_expand_in_order_in_shell_mode() {
    let (tx, _rx) = unbounded_channel::<AppEvent>();
    let sender = AppEventSender::new(tx);
    let mut composer = ChatComposer::new(
        true,
        sender,
        false,
        "Ask Nori to do anything".to_string(),
        false,
    );
    let first = "a".repeat(LARGE_PASTE_CHAR_THRESHOLD + 10);
    let second = "b".repeat(LARGE_PASTE_CHAR_THRESHOLD + 10);

    composer.handle_paste(format!("!{first}"));
    composer.handle_paste(second.clone());

    let (result, _) = composer.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(result, InputResult::Submitted(format!("!{first}{second}")));
}

#[test]
fn history_recall_places_cursor_at_end_and_keeps_navigating() {
    let (tx, _rx) = unbounded_channel::<AppEvent>();
    let sender = AppEventSender::new(tx);
    let mut composer = ChatComposer::new(
        true,
        sender,
        false,
        "Ask Nori to do anything".to_string(),
        false,
    );
    composer.history.record_local_submission("older");
    composer.history.record_local_submission("newer\nentry");

    composer.handle_key_event(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
    assert_eq!(composer.current_text(), "newer\nentry");
    assert_eq!(composer.textarea.cursor(), "newer\nentry".len());

    composer.handle_key_event(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
    assert_eq!(composer.current_text(), "older");
    assert_eq!(composer.textarea.cursor(), "older".len());
}

#[test]
fn asynchronous_history_recall_places_cursor_at_end() {
    let (tx, _rx) = unbounded_channel::<AppEvent>();
    let sender = AppEventSender::new(tx);
    let mut composer = ChatComposer::new(
        true,
        sender,
        false,
        "Ask Nori to do anything".to_string(),
        false,
    );
    composer.set_history_metadata(7, 1);
    composer.handle_key_event(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));

    assert!(composer.on_history_entry_response(7, 0, Some("remote\nentry".to_string())));
    assert_eq!(composer.current_text(), "remote\nentry");
    assert_eq!(composer.textarea.cursor(), "remote\nentry".len());
}
