use super::*;

#[test]
fn cursor_left_and_right_handle_graphemes() {
    let mut t = ta_with("a👍b");
    t.set_cursor(t.text().len());

    t.move_cursor_left(); // before 'b'
    let after_first_left = t.cursor();
    t.move_cursor_left(); // before '👍'
    let after_second_left = t.cursor();
    t.move_cursor_left(); // before 'a'
    let after_third_left = t.cursor();

    assert!(after_first_left < t.text().len());
    assert!(after_second_left < after_first_left);
    assert!(after_third_left < after_second_left);

    // Move right back to end safely
    t.move_cursor_right();
    t.move_cursor_right();
    t.move_cursor_right();
    assert_eq!(t.cursor(), t.text().len());
}

#[test]
fn control_b_and_f_move_cursor() {
    let mut t = ta_with("abcd");
    t.set_cursor(1);

    t.input(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL));
    assert_eq!(t.cursor(), 2);

    t.input(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::CONTROL));
    assert_eq!(t.cursor(), 1);
}

#[test]
fn control_b_f_fallback_control_chars_move_cursor() {
    let mut t = ta_with("abcd");
    t.set_cursor(2);

    // Simulate terminals that send C0 control chars without CONTROL modifier.
    // ^B (U+0002) should move left
    t.input(KeyEvent::new(KeyCode::Char('\u{0002}'), KeyModifiers::NONE));
    assert_eq!(t.cursor(), 1);

    // ^F (U+0006) should move right
    t.input(KeyEvent::new(KeyCode::Char('\u{0006}'), KeyModifiers::NONE));
    assert_eq!(t.cursor(), 2);
}

#[test]
fn delete_backward_word_alt_keys() {
    // Test the custom Alt+Ctrl+h binding
    let mut t = ta_with("hello world");
    t.set_cursor(t.text().len()); // cursor at the end
    t.input(KeyEvent::new(
        KeyCode::Char('h'),
        KeyModifiers::CONTROL | KeyModifiers::ALT,
    ));
    assert_eq!(t.text(), "hello ");
    assert_eq!(t.cursor(), 6);

    // Test the standard Alt+Backspace binding
    let mut t = ta_with("hello world");
    t.set_cursor(t.text().len()); // cursor at the end
    t.input(KeyEvent::new(KeyCode::Backspace, KeyModifiers::ALT));
    assert_eq!(t.text(), "hello ");
    assert_eq!(t.cursor(), 6);
}

#[test]
fn delete_backward_word_handles_narrow_no_break_space() {
    let mut t = ta_with("32\u{202F}AM");
    t.set_cursor(t.text().len());
    t.input(KeyEvent::new(KeyCode::Backspace, KeyModifiers::ALT));
    pretty_assertions::assert_eq!(t.text(), "32\u{202F}");
    pretty_assertions::assert_eq!(t.cursor(), t.text().len());
}

#[test]
fn delete_forward_word_with_without_alt_modifier() {
    let mut t = ta_with("hello world");
    t.set_cursor(0);
    t.input(KeyEvent::new(KeyCode::Delete, KeyModifiers::ALT));
    assert_eq!(t.text(), " world");
    assert_eq!(t.cursor(), 0);

    let mut t = ta_with("hello");
    t.set_cursor(0);
    t.input(KeyEvent::new(KeyCode::Delete, KeyModifiers::NONE));
    assert_eq!(t.text(), "ello");
    assert_eq!(t.cursor(), 0);
}

#[test]
fn control_h_backspace() {
    // Test Ctrl+H as backspace
    let mut t = ta_with("12345");
    t.set_cursor(3); // cursor after '3'
    t.input(KeyEvent::new(KeyCode::Char('h'), KeyModifiers::CONTROL));
    assert_eq!(t.text(), "1245");
    assert_eq!(t.cursor(), 2);

    // Test Ctrl+H at beginning (should be no-op)
    t.set_cursor(0);
    t.input(KeyEvent::new(KeyCode::Char('h'), KeyModifiers::CONTROL));
    assert_eq!(t.text(), "1245");
    assert_eq!(t.cursor(), 0);

    // Test Ctrl+H at end
    t.set_cursor(t.text().len());
    t.input(KeyEvent::new(KeyCode::Char('h'), KeyModifiers::CONTROL));
    assert_eq!(t.text(), "124");
    assert_eq!(t.cursor(), 3);
}

#[cfg_attr(not(windows), ignore = "AltGr modifier only applies on Windows")]
#[test]
fn altgr_ctrl_alt_char_inserts_literal() {
    let mut t = ta_with("");
    t.input(KeyEvent::new(
        KeyCode::Char('c'),
        KeyModifiers::CONTROL | KeyModifiers::ALT,
    ));
    assert_eq!(t.text(), "c");
    assert_eq!(t.cursor(), 1);
}

#[test]
fn cursor_vertical_movement_across_lines_and_bounds() {
    let mut t = ta_with("short\nloooooooooong\nmid");
    // Place cursor on second line, column 5
    let second_line_start = 6; // after first '\n'
    t.set_cursor(second_line_start + 5);

    // Move up: target column preserved, clamped by line length
    t.move_cursor_up();
    assert_eq!(t.cursor(), 5); // first line has len 5

    // Move up again goes to start of text
    t.move_cursor_up();
    assert_eq!(t.cursor(), 0);

    // Move down: from start to target col tracked
    t.move_cursor_down();
    // On first move down, we should land on second line, at col 0 (target col remembered as 0)
    let pos_after_down = t.cursor();
    assert!(pos_after_down >= second_line_start);

    // Move down again to third line; clamp to its length
    t.move_cursor_down();
    let third_line_start = t.text().find("mid").unwrap();
    let third_line_end = third_line_start + 3;
    assert!(t.cursor() >= third_line_start && t.cursor() <= third_line_end);

    // Moving down at last line jumps to end
    t.move_cursor_down();
    assert_eq!(t.cursor(), t.text().len());
}

#[test]
fn home_end_and_emacs_style_home_end() {
    let mut t = ta_with("one\ntwo\nthree");
    // Position at middle of second line
    let second_line_start = t.text().find("two").unwrap();
    t.set_cursor(second_line_start + 1);

    t.move_cursor_to_beginning_of_line(false);
    assert_eq!(t.cursor(), second_line_start);

    // Ctrl-A behavior: if at BOL, go to beginning of previous line
    t.move_cursor_to_beginning_of_line(true);
    assert_eq!(t.cursor(), 0); // beginning of first line

    // Move to EOL of first line
    t.move_cursor_to_end_of_line(false);
    assert_eq!(t.cursor(), 3);

    // Ctrl-E: if at EOL, go to end of next line
    t.move_cursor_to_end_of_line(true);
    // end of second line ("two") is right before its '\n'
    let end_second_nl = t.text().find("\nthree").unwrap();
    assert_eq!(t.cursor(), end_second_nl);
}

#[test]
fn end_of_line_or_down_at_end_of_text() {
    let mut t = ta_with("one\ntwo");
    // Place cursor at absolute end of the text
    t.set_cursor(t.text().len());
    // Should remain at end without panicking
    t.move_cursor_to_end_of_line(true);
    assert_eq!(t.cursor(), t.text().len());

    // Also verify behavior when at EOL of a non-final line:
    let eol_first_line = 3; // index of '\n' in "one\ntwo"
    t.set_cursor(eol_first_line);
    t.move_cursor_to_end_of_line(true);
    assert_eq!(t.cursor(), t.text().len()); // moves to end of next (last) line
}

#[test]
fn word_navigation_helpers() {
    let t = ta_with("  alpha  beta   gamma");
    let mut t = t; // make mutable for set_cursor
    // Put cursor after "alpha"
    let after_alpha = t.text().find("alpha").unwrap() + "alpha".len();
    t.set_cursor(after_alpha);
    assert_eq!(t.beginning_of_previous_word(), 2); // skip initial spaces

    // Put cursor at start of beta
    let beta_start = t.text().find("beta").unwrap();
    t.set_cursor(beta_start);
    assert_eq!(t.end_of_next_word(), beta_start + "beta".len());

    // If at end, end_of_next_word returns len
    t.set_cursor(t.text().len());
    assert_eq!(t.end_of_next_word(), t.text().len());
}

#[test]
fn wrapping_and_cursor_positions() {
    let mut t = ta_with("hello world here");
    let area = Rect::new(0, 0, 6, 10); // width 6 -> wraps words
    // desired height counts wrapped lines
    assert!(t.desired_height(area.width) >= 3);

    // Place cursor in "world"
    let world_start = t.text().find("world").unwrap();
    t.set_cursor(world_start + 3);
    let (_x, y) = t.cursor_pos(area).unwrap();
    assert_eq!(y, 1); // world should be on second wrapped line

    // With state and small height, cursor is mapped onto visible row
    let mut state = TextAreaState::default();
    let small_area = Rect::new(0, 0, 6, 1);
    // First call: cursor not visible -> effective scroll ensures it is
    let (_x, y) = t.cursor_pos_with_state(small_area, state).unwrap();
    assert_eq!(y, 0);

    // Render with state to update actual scroll value
    let mut buf = Buffer::empty(small_area);
    ratatui::widgets::StatefulWidgetRef::render_ref(&(&t), small_area, &mut buf, &mut state);
    // After render, state.scroll should be adjusted so cursor row fits
    let effective_lines = t.desired_height(small_area.width);
    assert!(state.scroll < effective_lines);
}

#[test]
fn cursor_pos_with_state_basic_and_scroll_behaviors() {
    // Case 1: No wrapping needed, height fits — scroll ignored, y maps directly.
    let mut t = ta_with("hello world");
    t.set_cursor(3);
    let area = Rect::new(2, 5, 20, 3);
    // Even if an absurd scroll is provided, when content fits the area the
    // effective scroll is 0 and the cursor position matches cursor_pos.
    let bad_state = TextAreaState { scroll: 999 };
    let (x1, y1) = t.cursor_pos(area).unwrap();
    let (x2, y2) = t.cursor_pos_with_state(area, bad_state).unwrap();
    assert_eq!((x2, y2), (x1, y1));

    // Case 2: Cursor below the current window — y should be clamped to the
    // bottom row (area.height - 1) after adjusting effective scroll.
    let mut t = ta_with("one two three four five six");
    // Force wrapping to many visual lines.
    let wrap_width = 4;
    let _ = t.desired_height(wrap_width);
    // Put cursor somewhere near the end so it's definitely below the first window.
    t.set_cursor(t.text().len().saturating_sub(2));
    let small_area = Rect::new(0, 0, wrap_width, 2);
    let state = TextAreaState { scroll: 0 };
    let (_x, y) = t.cursor_pos_with_state(small_area, state).unwrap();
    assert_eq!(y, small_area.y + small_area.height - 1);

    // Case 3: Cursor above the current window — y should be top row (0)
    // when the provided scroll is too large.
    let mut t = ta_with("alpha beta gamma delta epsilon zeta");
    let wrap_width = 5;
    let lines = t.desired_height(wrap_width);
    // Place cursor near start so an excessive scroll moves it to top row.
    t.set_cursor(1);
    let area = Rect::new(0, 0, wrap_width, 3);
    let state = TextAreaState {
        scroll: lines.saturating_mul(2),
    };
    let (_x, y) = t.cursor_pos_with_state(area, state).unwrap();
    assert_eq!(y, area.y);
}

#[test]
fn wrapped_navigation_across_visual_lines() {
    let mut t = ta_with("abcdefghij");
    // Force wrapping at width 4: lines -> ["abcd", "efgh", "ij"]
    let _ = t.desired_height(4);

    // From the very start, moving down should go to the start of the next wrapped line (index 4)
    t.set_cursor(0);
    t.move_cursor_down();
    assert_eq!(t.cursor(), 4);

    // Cursor at boundary index 4 should be displayed at start of second wrapped line
    t.set_cursor(4);
    let area = Rect::new(0, 0, 4, 10);
    let (x, y) = t.cursor_pos(area).unwrap();
    assert_eq!((x, y), (0, 1));

    // With state and small height, cursor should be visible at row 0, col 0
    let small_area = Rect::new(0, 0, 4, 1);
    let state = TextAreaState::default();
    let (x, y) = t.cursor_pos_with_state(small_area, state).unwrap();
    assert_eq!((x, y), (0, 0));

    // Place cursor in the middle of the second wrapped line ("efgh"), at 'g'
    t.set_cursor(6);
    // Move up should go to same column on previous wrapped line -> index 2 ('c')
    t.move_cursor_up();
    assert_eq!(t.cursor(), 2);

    // Move down should return to same position on the next wrapped line -> back to index 6 ('g')
    t.move_cursor_down();
    assert_eq!(t.cursor(), 6);

    // Move down again should go to third wrapped line. Target col is 2, but the line has len 2 -> clamp to end
    t.move_cursor_down();
    assert_eq!(t.cursor(), t.text().len());
}

#[test]
fn cursor_pos_with_state_after_movements() {
    let mut t = ta_with("abcdefghij");
    // Wrap width 4 -> visual lines: abcd | efgh | ij
    let _ = t.desired_height(4);
    let area = Rect::new(0, 0, 4, 2);
    let mut state = TextAreaState::default();
    let mut buf = Buffer::empty(area);

    // Start at beginning
    t.set_cursor(0);
    ratatui::widgets::StatefulWidgetRef::render_ref(&(&t), area, &mut buf, &mut state);
    let (x, y) = t.cursor_pos_with_state(area, state).unwrap();
    assert_eq!((x, y), (0, 0));

    // Move down to second visual line; should be at bottom row (row 1) within 2-line viewport
    t.move_cursor_down();
    ratatui::widgets::StatefulWidgetRef::render_ref(&(&t), area, &mut buf, &mut state);
    let (x, y) = t.cursor_pos_with_state(area, state).unwrap();
    assert_eq!((x, y), (0, 1));

    // Move down to third visual line; viewport scrolls and keeps cursor on bottom row
    t.move_cursor_down();
    ratatui::widgets::StatefulWidgetRef::render_ref(&(&t), area, &mut buf, &mut state);
    let (x, y) = t.cursor_pos_with_state(area, state).unwrap();
    assert_eq!((x, y), (0, 1));

    // Move up to second visual line; with current scroll, it appears on top row
    t.move_cursor_up();
    ratatui::widgets::StatefulWidgetRef::render_ref(&(&t), area, &mut buf, &mut state);
    let (x, y) = t.cursor_pos_with_state(area, state).unwrap();
    assert_eq!((x, y), (0, 0));

    // Column preservation across moves: set to col 2 on first line, move down
    t.set_cursor(2);
    ratatui::widgets::StatefulWidgetRef::render_ref(&(&t), area, &mut buf, &mut state);
    let (x0, y0) = t.cursor_pos_with_state(area, state).unwrap();
    assert_eq!((x0, y0), (2, 0));
    t.move_cursor_down();
    ratatui::widgets::StatefulWidgetRef::render_ref(&(&t), area, &mut buf, &mut state);
    let (x1, y1) = t.cursor_pos_with_state(area, state).unwrap();
    assert_eq!((x1, y1), (2, 1));
}
