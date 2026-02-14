use super::*;
use image::ImageBuffer;
use image::Rgba;
use pretty_assertions::assert_eq;
use std::path::PathBuf;
use tempfile::tempdir;

use crate::app_event::AppEvent;
use crate::bottom_pane::AppEventSender;
use crate::bottom_pane::ChatComposer;
use crate::bottom_pane::InputResult;
use crate::bottom_pane::chat_composer::AttachedImage;
use crate::bottom_pane::chat_composer::LARGE_PASTE_CHAR_THRESHOLD;
use crate::bottom_pane::prompt_args::extract_positional_args_for_prompt_line;
use crate::bottom_pane::textarea::TextArea;
use tokio::sync::mpsc::unbounded_channel;

#[test]
fn footer_hint_row_is_separated_from_composer() {
    let (tx, _rx) = unbounded_channel::<AppEvent>();
    let sender = AppEventSender::new(tx);
    let composer = ChatComposer::new(
        true,
        sender,
        false,
        "Ask Codex to do anything".to_string(),
        false,
    );

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

    let mut hint_row: Option<(u16, String)> = None;
    for y in 0..area.height {
        let row = row_to_string(y);
        if row.contains("? for shortcuts") {
            hint_row = Some((y, row));
            break;
        }
    }

    let (hint_row_idx, hint_row_contents) =
        hint_row.expect("expected footer hint row to be rendered");
    assert_eq!(
        hint_row_idx,
        area.height - 1,
        "hint row should occupy the bottom line: {hint_row_contents:?}",
    );

    assert!(
        hint_row_idx > 0,
        "expected a spacing row above the footer hints",
    );

    let spacing_row = row_to_string(hint_row_idx - 1);
    assert_eq!(
        spacing_row.trim(),
        "",
        "expected blank spacing row above hints but saw: {spacing_row:?}",
    );
}

fn snapshot_composer_state<F>(name: &str, enhanced_keys_supported: bool, setup: F)
where
    F: FnOnce(&mut ChatComposer),
{
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let width = 100;
    let (tx, _rx) = unbounded_channel::<AppEvent>();
    let sender = AppEventSender::new(tx);
    let mut composer = ChatComposer::new(
        true,
        sender,
        enhanced_keys_supported,
        "Ask Codex to do anything".to_string(),
        false,
    );
    setup(&mut composer);
    let footer_props = composer.footer_props();
    let footer_lines = footer_height(&footer_props);
    let footer_spacing = ChatComposer::footer_spacing(footer_lines);
    let height = footer_lines + footer_spacing + 8;
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal
        .draw(|f| composer.render(f.area(), f.buffer_mut()))
        .unwrap();
    insta::assert_snapshot!(name, terminal.backend());
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
        "Ask Codex to do anything".to_string(),
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
        "Ask Codex to do anything".to_string(),
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
        "Ask Codex to do anything".to_string(),
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
        "Ask Codex to do anything".to_string(),
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
        "Ask Codex to do anything".to_string(),
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
        "Ask Codex to do anything".to_string(),
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
        "Ask Codex to do anything".to_string(),
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
        "Ask Codex to do anything".to_string(),
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
        "Ask Codex to do anything".to_string(),
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
            "Ask Codex to do anything".to_string(),
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
        "Ask Codex to do anything".to_string(),
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
    use super::super::command_popup::CommandItem;
    let (tx, _rx) = unbounded_channel::<AppEvent>();
    let sender = AppEventSender::new(tx);
    let mut composer = ChatComposer::new(
        true,
        sender,
        false,
        "Ask Codex to do anything".to_string(),
        false,
    );
    type_chars_humanlike(&mut composer, &['/', 'm', 'o']);

    match &composer.active_popup {
        ActivePopup::Command(popup) => match popup.selected_item() {
            Some(CommandItem::Builtin(cmd)) => {
                assert_eq!(cmd.command(), "model")
            }
            Some(CommandItem::UserPrompt(_)) => {
                panic!("unexpected prompt selected for '/mo'")
            }
            None => panic!("no selected command for '/mo'"),
        },
        _ => panic!("slash popup not active after typing '/mo'"),
    }
}

// Test helper: simulate human typing with a brief delay and flush the paste-burst buffer
fn type_chars_humanlike(composer: &mut ChatComposer, chars: &[char]) {
    use crossterm::event::KeyCode;
    use crossterm::event::KeyEvent;
    use crossterm::event::KeyModifiers;
    for &ch in chars {
        let _ = composer.handle_key_event(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        std::thread::sleep(ChatComposer::recommended_paste_flush_delay());
        let _ = composer.flush_paste_burst_if_due();
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
        "Ask Codex to do anything".to_string(),
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
        "Ask Codex to do anything".to_string(),
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
        "Ask Codex to do anything".to_string(),
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
        "Ask Codex to do anything".to_string(),
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
        "Ask Codex to do anything".to_string(),
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

// --- Image attachment tests ---
#[test]
fn attach_image_and_submit_includes_image_paths() {
    let (tx, _rx) = unbounded_channel::<AppEvent>();
    let sender = AppEventSender::new(tx);
    let mut composer = ChatComposer::new(
        true,
        sender,
        false,
        "Ask Codex to do anything".to_string(),
        false,
    );
    let path = PathBuf::from("/tmp/image1.png");
    composer.attach_image(path.clone(), 32, 16, "PNG");
    composer.handle_paste(" hi".into());
    let (result, _) = composer.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    match result {
        InputResult::Submitted(text) => assert_eq!(text, "[image1.png 32x16] hi"),
        _ => panic!("expected Submitted"),
    }
    let imgs = composer.take_recent_submission_images();
    assert_eq!(vec![path], imgs);
}

#[test]
fn attach_image_without_text_submits_empty_text_and_images() {
    let (tx, _rx) = unbounded_channel::<AppEvent>();
    let sender = AppEventSender::new(tx);
    let mut composer = ChatComposer::new(
        true,
        sender,
        false,
        "Ask Codex to do anything".to_string(),
        false,
    );
    let path = PathBuf::from("/tmp/image2.png");
    composer.attach_image(path.clone(), 10, 5, "PNG");
    let (result, _) = composer.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    match result {
        InputResult::Submitted(text) => assert_eq!(text, "[image2.png 10x5]"),
        _ => panic!("expected Submitted"),
    }
    let imgs = composer.take_recent_submission_images();
    assert_eq!(imgs.len(), 1);
    assert_eq!(imgs[0], path);
    assert!(composer.attached_images.is_empty());
}

#[test]
fn image_placeholder_backspace_behaves_like_text_placeholder() {
    let (tx, _rx) = unbounded_channel::<AppEvent>();
    let sender = AppEventSender::new(tx);
    let mut composer = ChatComposer::new(
        true,
        sender,
        false,
        "Ask Codex to do anything".to_string(),
        false,
    );
    let path = PathBuf::from("/tmp/image3.png");
    composer.attach_image(path.clone(), 20, 10, "PNG");
    let placeholder = composer.attached_images[0].placeholder.clone();

    // Case 1: backspace at end
    composer.textarea.move_cursor_to_end_of_line(false);
    composer.handle_key_event(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
    assert!(!composer.textarea.text().contains(&placeholder));
    assert!(composer.attached_images.is_empty());

    // Re-add and test backspace in middle: should break the placeholder string
    // and drop the image mapping (same as text placeholder behavior).
    composer.attach_image(path, 20, 10, "PNG");
    let placeholder2 = composer.attached_images[0].placeholder.clone();
    // Move cursor to roughly middle of placeholder
    if let Some(start_pos) = composer.textarea.text().find(&placeholder2) {
        let mid_pos = start_pos + (placeholder2.len() / 2);
        composer.textarea.set_cursor(mid_pos);
        composer.handle_key_event(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
        assert!(!composer.textarea.text().contains(&placeholder2));
        assert!(composer.attached_images.is_empty());
    } else {
        panic!("Placeholder not found in textarea");
    }
}

#[test]
fn backspace_with_multibyte_text_before_placeholder_does_not_panic() {
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

    // Insert an image placeholder at the start
    let path = PathBuf::from("/tmp/image_multibyte.png");
    composer.attach_image(path, 10, 5, "PNG");
    // Add multibyte text after the placeholder
    composer.textarea.insert_str("日本語");

    // Cursor is at end; pressing backspace should delete the last character
    // without panicking and leave the placeholder intact.
    composer.handle_key_event(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));

    assert_eq!(composer.attached_images.len(), 1);
    assert!(
        composer
            .textarea
            .text()
            .starts_with("[image_multibyte.png 10x5]")
    );
}

#[test]
fn deleting_one_of_duplicate_image_placeholders_removes_matching_entry() {
    let (tx, _rx) = unbounded_channel::<AppEvent>();
    let sender = AppEventSender::new(tx);
    let mut composer = ChatComposer::new(
        true,
        sender,
        false,
        "Ask Codex to do anything".to_string(),
        false,
    );

    let path1 = PathBuf::from("/tmp/image_dup1.png");
    let path2 = PathBuf::from("/tmp/image_dup2.png");

    composer.attach_image(path1, 10, 5, "PNG");
    // separate placeholders with a space for clarity
    composer.handle_paste(" ".into());
    composer.attach_image(path2.clone(), 10, 5, "PNG");

    let placeholder1 = composer.attached_images[0].placeholder.clone();
    let placeholder2 = composer.attached_images[1].placeholder.clone();
    let text = composer.textarea.text().to_string();
    let start1 = text.find(&placeholder1).expect("first placeholder present");
    let end1 = start1 + placeholder1.len();
    composer.textarea.set_cursor(end1);

    // Backspace should delete the first placeholder and its mapping.
    composer.handle_key_event(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));

    let new_text = composer.textarea.text().to_string();
    assert_eq!(
        0,
        new_text.matches(&placeholder1).count(),
        "first placeholder removed"
    );
    assert_eq!(
        1,
        new_text.matches(&placeholder2).count(),
        "second placeholder remains"
    );
    assert_eq!(
        vec![AttachedImage {
            path: path2,
            placeholder: "[image_dup2.png 10x5]".to_string()
        }],
        composer.attached_images,
        "one image mapping remains"
    );
}

#[test]
fn pasting_filepath_attaches_image() {
    let tmp = tempdir().expect("create TempDir");
    let tmp_path: PathBuf = tmp.path().join("nori_tui_test_paste_image.png");
    let img: ImageBuffer<Rgba<u8>, Vec<u8>> =
        ImageBuffer::from_fn(3, 2, |_x, _y| Rgba([1, 2, 3, 255]));
    img.save(&tmp_path).expect("failed to write temp png");

    let (tx, _rx) = unbounded_channel::<AppEvent>();
    let sender = AppEventSender::new(tx);
    let mut composer = ChatComposer::new(
        true,
        sender,
        false,
        "Ask Codex to do anything".to_string(),
        false,
    );

    let needs_redraw = composer.handle_paste(tmp_path.to_string_lossy().to_string());
    assert!(needs_redraw);
    assert!(
        composer
            .textarea
            .text()
            .starts_with("[nori_tui_test_paste_image.png 3x2] ")
    );

    let imgs = composer.take_recent_submission_images();
    assert_eq!(imgs, vec![tmp_path]);
}

#[test]
fn selecting_custom_prompt_without_args_submits_content() {
    let prompt_text = "Hello from saved prompt";

    let (tx, _rx) = unbounded_channel::<AppEvent>();
    let sender = AppEventSender::new(tx);
    let mut composer = ChatComposer::new(
        true,
        sender,
        false,
        "Ask Codex to do anything".to_string(),
        false,
    );

    // Inject prompts as if received via event.
    composer.set_custom_prompts(vec![CustomPrompt {
        name: "my-prompt".to_string(),
        path: "/tmp/my-prompt.md".to_string().into(),
        content: prompt_text.to_string(),
        description: None,
        argument_hint: None,
        kind: Default::default(),
    }]);

    type_chars_humanlike(
        &mut composer,
        &[
            '/', 'p', 'r', 'o', 'm', 'p', 't', 's', ':', 'm', 'y', '-', 'p', 'r', 'o', 'm', 'p',
            't',
        ],
    );

    let (result, _needs_redraw) =
        composer.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert_eq!(InputResult::Submitted(prompt_text.to_string()), result);
    assert!(composer.textarea.is_empty());
}

#[test]
fn custom_prompt_submission_expands_arguments() {
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
        content: "Review $USER changes on $BRANCH".to_string(),
        description: None,
        argument_hint: None,
        kind: Default::default(),
    }]);

    composer
        .textarea
        .set_text("/prompts:my-prompt USER=Alice BRANCH=main");

    let (result, _needs_redraw) =
        composer.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert_eq!(
        InputResult::Submitted("Review Alice changes on main".to_string()),
        result
    );
    assert!(composer.textarea.is_empty());
}

#[test]
fn custom_prompt_submission_accepts_quoted_values() {
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
        content: "Pair $USER with $BRANCH".to_string(),
        description: None,
        argument_hint: None,
        kind: Default::default(),
    }]);

    composer
        .textarea
        .set_text("/prompts:my-prompt USER=\"Alice Smith\" BRANCH=dev-main");

    let (result, _needs_redraw) =
        composer.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert_eq!(
        InputResult::Submitted("Pair Alice Smith with dev-main".to_string()),
        result
    );
    assert!(composer.textarea.is_empty());
}

#[test]
fn custom_prompt_with_large_paste_expands_correctly() {
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

    // Create a custom prompt with positional args (no named args like $USER)
    composer.set_custom_prompts(vec![CustomPrompt {
        name: "code-review".to_string(),
        path: "/tmp/code-review.md".to_string().into(),
        content: "Please review the following code:\n\n$1".to_string(),
        description: None,
        argument_hint: None,
        kind: Default::default(),
    }]);

    // Type the slash command
    let command_text = "/prompts:code-review ";
    composer.textarea.set_text(command_text);
    composer.textarea.set_cursor(command_text.len());

    // Paste large content (>3000 chars) to trigger placeholder
    let large_content = "x".repeat(LARGE_PASTE_CHAR_THRESHOLD + 3000);
    composer.handle_paste(large_content.clone());

    // Verify placeholder was created
    let placeholder = format!("[Pasted Content {} chars]", large_content.chars().count());
    assert_eq!(
        composer.textarea.text(),
        format!("/prompts:code-review {}", placeholder)
    );
    assert_eq!(composer.pending_pastes.len(), 1);
    assert_eq!(composer.pending_pastes[0].0, placeholder);
    assert_eq!(composer.pending_pastes[0].1, large_content);

    // Submit by pressing Enter
    let (result, _needs_redraw) =
        composer.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    // Verify the custom prompt was expanded with the large content as positional arg
    match result {
        InputResult::Submitted(text) => {
            // The prompt should be expanded, with the large content replacing $1
            assert_eq!(
                text,
                format!("Please review the following code:\n\n{}", large_content),
                "Expected prompt expansion with large content as $1"
            );
        }
        _ => panic!("expected Submitted, got: {result:?}"),
    }
    assert!(composer.textarea.is_empty());
    assert!(composer.pending_pastes.is_empty());
}

#[test]
fn slash_path_input_submits_without_command_error() {
    use crossterm::event::KeyCode;
    use crossterm::event::KeyEvent;
    use crossterm::event::KeyModifiers;

    let (tx, mut rx) = unbounded_channel::<AppEvent>();
    let sender = AppEventSender::new(tx);
    let mut composer = ChatComposer::new(
        true,
        sender,
        false,
        "Ask Codex to do anything".to_string(),
        false,
    );

    composer
        .textarea
        .set_text("/Users/example/project/src/main.rs");

    let (result, _needs_redraw) =
        composer.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    if let InputResult::Submitted(text) = result {
        assert_eq!(text, "/Users/example/project/src/main.rs");
    } else {
        panic!("expected Submitted");
    }
    assert!(composer.textarea.is_empty());
    match rx.try_recv() {
        Ok(event) => panic!("unexpected event: {event:?}"),
        Err(tokio::sync::mpsc::error::TryRecvError::Empty) => {}
        Err(err) => panic!("unexpected channel state: {err:?}"),
    }
}

#[test]
fn slash_with_leading_space_submits_as_text() {
    use crossterm::event::KeyCode;
    use crossterm::event::KeyEvent;
    use crossterm::event::KeyModifiers;

    let (tx, mut rx) = unbounded_channel::<AppEvent>();
    let sender = AppEventSender::new(tx);
    let mut composer = ChatComposer::new(
        true,
        sender,
        false,
        "Ask Codex to do anything".to_string(),
        false,
    );

    composer.textarea.set_text(" /this-looks-like-a-command");

    let (result, _needs_redraw) =
        composer.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    if let InputResult::Submitted(text) = result {
        assert_eq!(text, "/this-looks-like-a-command");
    } else {
        panic!("expected Submitted");
    }
    assert!(composer.textarea.is_empty());
    match rx.try_recv() {
        Ok(event) => panic!("unexpected event: {event:?}"),
        Err(tokio::sync::mpsc::error::TryRecvError::Empty) => {}
        Err(err) => panic!("unexpected channel state: {err:?}"),
    }
}

#[test]
fn custom_prompt_invalid_args_reports_error() {
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
        content: "Review $USER changes".to_string(),
        description: None,
        argument_hint: None,
        kind: Default::default(),
    }]);

    composer
        .textarea
        .set_text("/prompts:my-prompt USER=Alice stray");

    let (result, _needs_redraw) =
        composer.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert_eq!(InputResult::None, result);
    assert_eq!(
        "/prompts:my-prompt USER=Alice stray",
        composer.textarea.text()
    );

    let mut found_error = false;
    while let Ok(event) = rx.try_recv() {
        if let AppEvent::InsertHistoryCell(cell) = event {
            let message = cell
                .display_lines(80)
                .into_iter()
                .map(|line| line.to_string())
                .collect::<Vec<_>>()
                .join("\n");
            assert!(message.contains("expected key=value"));
            found_error = true;
            break;
        }
    }
    assert!(found_error, "expected error history cell to be sent");
}

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
