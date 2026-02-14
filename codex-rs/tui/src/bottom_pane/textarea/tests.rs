use super::*;
// crossterm types are intentionally not imported here to avoid unused warnings
use rand::prelude::*;

fn rand_grapheme(rng: &mut rand::rngs::StdRng) -> String {
    let r: u8 = rng.random_range(0..100);
    match r {
        0..=4 => "\n".to_string(),
        5..=12 => " ".to_string(),
        13..=35 => (rng.random_range(b'a'..=b'z') as char).to_string(),
        36..=45 => (rng.random_range(b'A'..=b'Z') as char).to_string(),
        46..=52 => (rng.random_range(b'0'..=b'9') as char).to_string(),
        53..=65 => {
            // Some emoji (wide graphemes)
            let choices = ["👍", "😊", "🐍", "🚀", "🧪", "🌟"];
            choices[rng.random_range(0..choices.len())].to_string()
        }
        66..=75 => {
            // CJK wide characters
            let choices = ["漢", "字", "測", "試", "你", "好", "界", "编", "码"];
            choices[rng.random_range(0..choices.len())].to_string()
        }
        76..=85 => {
            // Combining mark sequences
            let base = ["e", "a", "o", "n", "u"][rng.random_range(0..5)];
            let marks = ["\u{0301}", "\u{0308}", "\u{0302}", "\u{0303}"];
            format!("{base}{}", marks[rng.random_range(0..marks.len())])
        }
        86..=92 => {
            // Some non-latin single codepoints (Greek, Cyrillic, Hebrew)
            let choices = ["Ω", "β", "Ж", "ю", "ש", "م", "ह"];
            choices[rng.random_range(0..choices.len())].to_string()
        }
        _ => {
            // ZWJ sequences (single graphemes but multi-codepoint)
            let choices = [
                "👩\u{200D}💻", // woman technologist
                "👨\u{200D}💻", // man technologist
                "🏳️\u{200D}🌈", // rainbow flag
            ];
            choices[rng.random_range(0..choices.len())].to_string()
        }
    }
}

fn ta_with(text: &str) -> TextArea {
    let mut t = TextArea::new();
    t.insert_str(text);
    t
}

#[test]
fn insert_and_replace_update_cursor_and_text() {
    // insert helpers
    let mut t = ta_with("hello");
    t.set_cursor(5);
    t.insert_str("!");
    assert_eq!(t.text(), "hello!");
    assert_eq!(t.cursor(), 6);

    t.insert_str_at(0, "X");
    assert_eq!(t.text(), "Xhello!");
    assert_eq!(t.cursor(), 7);

    // Insert after the cursor should not move it
    t.set_cursor(1);
    let end = t.text().len();
    t.insert_str_at(end, "Y");
    assert_eq!(t.text(), "Xhello!Y");
    assert_eq!(t.cursor(), 1);

    // replace_range cases
    // 1) cursor before range
    let mut t = ta_with("abcd");
    t.set_cursor(1);
    t.replace_range(2..3, "Z");
    assert_eq!(t.text(), "abZd");
    assert_eq!(t.cursor(), 1);

    // 2) cursor inside range
    let mut t = ta_with("abcd");
    t.set_cursor(2);
    t.replace_range(1..3, "Q");
    assert_eq!(t.text(), "aQd");
    assert_eq!(t.cursor(), 2);

    // 3) cursor after range with shifted by diff
    let mut t = ta_with("abcd");
    t.set_cursor(4);
    t.replace_range(0..1, "AA");
    assert_eq!(t.text(), "AAbcd");
    assert_eq!(t.cursor(), 5);
}

#[test]
fn delete_backward_and_forward_edges() {
    let mut t = ta_with("abc");
    t.set_cursor(1);
    t.delete_backward(1);
    assert_eq!(t.text(), "bc");
    assert_eq!(t.cursor(), 0);

    // deleting backward at start is a no-op
    t.set_cursor(0);
    t.delete_backward(1);
    assert_eq!(t.text(), "bc");
    assert_eq!(t.cursor(), 0);

    // forward delete removes next grapheme
    t.set_cursor(1);
    t.delete_forward(1);
    assert_eq!(t.text(), "b");
    assert_eq!(t.cursor(), 1);

    // forward delete at end is a no-op
    t.set_cursor(t.text().len());
    t.delete_forward(1);
    assert_eq!(t.text(), "b");
}

#[test]
fn delete_backward_word_and_kill_line_variants() {
    // delete backward word at end removes the whole previous word
    let mut t = ta_with("hello   world  ");
    t.set_cursor(t.text().len());
    t.delete_backward_word();
    assert_eq!(t.text(), "hello   ");
    assert_eq!(t.cursor(), 8);

    // From inside a word, delete from word start to cursor
    let mut t = ta_with("foo bar");
    t.set_cursor(6); // inside "bar" (after 'a')
    t.delete_backward_word();
    assert_eq!(t.text(), "foo r");
    assert_eq!(t.cursor(), 4);

    // From end, delete the last word only
    let mut t = ta_with("foo bar");
    t.set_cursor(t.text().len());
    t.delete_backward_word();
    assert_eq!(t.text(), "foo ");
    assert_eq!(t.cursor(), 4);

    // kill_to_end_of_line when not at EOL
    let mut t = ta_with("abc\ndef");
    t.set_cursor(1); // on first line, middle
    t.kill_to_end_of_line();
    assert_eq!(t.text(), "a\ndef");
    assert_eq!(t.cursor(), 1);

    // kill_to_end_of_line when at EOL deletes newline
    let mut t = ta_with("abc\ndef");
    t.set_cursor(3); // EOL of first line
    t.kill_to_end_of_line();
    assert_eq!(t.text(), "abcdef");
    assert_eq!(t.cursor(), 3);

    // kill_to_beginning_of_line from middle of line
    let mut t = ta_with("abc\ndef");
    t.set_cursor(5); // on second line, after 'e'
    t.kill_to_beginning_of_line();
    assert_eq!(t.text(), "abc\nef");

    // kill_to_beginning_of_line at beginning of non-first line removes the previous newline
    let mut t = ta_with("abc\ndef");
    t.set_cursor(4); // beginning of second line
    t.kill_to_beginning_of_line();
    assert_eq!(t.text(), "abcdef");
    assert_eq!(t.cursor(), 3);
}

#[test]
fn delete_forward_word_variants() {
    let mut t = ta_with("hello   world ");
    t.set_cursor(0);
    t.delete_forward_word();
    assert_eq!(t.text(), "   world ");
    assert_eq!(t.cursor(), 0);

    let mut t = ta_with("hello   world ");
    t.set_cursor(1);
    t.delete_forward_word();
    assert_eq!(t.text(), "h   world ");
    assert_eq!(t.cursor(), 1);

    let mut t = ta_with("hello   world");
    t.set_cursor(t.text().len());
    t.delete_forward_word();
    assert_eq!(t.text(), "hello   world");
    assert_eq!(t.cursor(), t.text().len());

    let mut t = ta_with("foo   \nbar");
    t.set_cursor(3);
    t.delete_forward_word();
    assert_eq!(t.text(), "foo");
    assert_eq!(t.cursor(), 3);

    let mut t = ta_with("foo\nbar");
    t.set_cursor(3);
    t.delete_forward_word();
    assert_eq!(t.text(), "foo");
    assert_eq!(t.cursor(), 3);

    let mut t = ta_with("hello   world ");
    t.set_cursor(t.text().len() + 10);
    t.delete_forward_word();
    assert_eq!(t.text(), "hello   world ");
    assert_eq!(t.cursor(), t.text().len());
}

#[test]
fn delete_forward_word_handles_atomic_elements() {
    let mut t = TextArea::new();
    t.insert_element("<element>");
    t.insert_str(" tail");

    t.set_cursor(0);
    t.delete_forward_word();
    assert_eq!(t.text(), " tail");
    assert_eq!(t.cursor(), 0);

    let mut t = TextArea::new();
    t.insert_str("   ");
    t.insert_element("<element>");
    t.insert_str(" tail");

    t.set_cursor(0);
    t.delete_forward_word();
    assert_eq!(t.text(), " tail");
    assert_eq!(t.cursor(), 0);

    let mut t = TextArea::new();
    t.insert_str("prefix ");
    t.insert_element("<element>");
    t.insert_str(" tail");

    // cursor in the middle of the element, delete_forward_word deletes the element
    let elem_range = t.elements[0].range.clone();
    t.cursor_pos = elem_range.start + (elem_range.len() / 2);
    t.delete_forward_word();
    assert_eq!(t.text(), "prefix  tail");
    assert_eq!(t.cursor(), elem_range.start);
}

#[test]
fn delete_backward_word_respects_word_separators() {
    let mut t = ta_with("path/to/file");
    t.set_cursor(t.text().len());
    t.delete_backward_word();
    assert_eq!(t.text(), "path/to/");
    assert_eq!(t.cursor(), t.text().len());

    t.delete_backward_word();
    assert_eq!(t.text(), "path/to");
    assert_eq!(t.cursor(), t.text().len());

    let mut t = ta_with("foo/ ");
    t.set_cursor(t.text().len());
    t.delete_backward_word();
    assert_eq!(t.text(), "foo");
    assert_eq!(t.cursor(), 3);

    let mut t = ta_with("foo /");
    t.set_cursor(t.text().len());
    t.delete_backward_word();
    assert_eq!(t.text(), "foo ");
    assert_eq!(t.cursor(), 4);
}

#[test]
fn delete_forward_word_respects_word_separators() {
    let mut t = ta_with("path/to/file");
    t.set_cursor(0);
    t.delete_forward_word();
    assert_eq!(t.text(), "/to/file");
    assert_eq!(t.cursor(), 0);

    t.delete_forward_word();
    assert_eq!(t.text(), "to/file");
    assert_eq!(t.cursor(), 0);

    let mut t = ta_with("/ foo");
    t.set_cursor(0);
    t.delete_forward_word();
    assert_eq!(t.text(), " foo");
    assert_eq!(t.cursor(), 0);

    let mut t = ta_with(" /foo");
    t.set_cursor(0);
    t.delete_forward_word();
    assert_eq!(t.text(), "foo");
    assert_eq!(t.cursor(), 0);
}

#[test]
fn yank_restores_last_kill() {
    let mut t = ta_with("hello");
    t.set_cursor(0);
    t.kill_to_end_of_line();
    assert_eq!(t.text(), "");
    assert_eq!(t.cursor(), 0);

    t.yank();
    assert_eq!(t.text(), "hello");
    assert_eq!(t.cursor(), 5);

    let mut t = ta_with("hello world");
    t.set_cursor(t.text().len());
    t.delete_backward_word();
    assert_eq!(t.text(), "hello ");
    assert_eq!(t.cursor(), 6);

    t.yank();
    assert_eq!(t.text(), "hello world");
    assert_eq!(t.cursor(), 11);

    let mut t = ta_with("hello");
    t.set_cursor(5);
    t.kill_to_beginning_of_line();
    assert_eq!(t.text(), "");
    assert_eq!(t.cursor(), 0);

    t.yank();
    assert_eq!(t.text(), "hello");
    assert_eq!(t.cursor(), 5);
}

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

#[test]
fn wrapped_navigation_with_newlines_and_spaces() {
    // Include spaces and an explicit newline to exercise boundaries
    let mut t = ta_with("word1  word2\nword3");
    // Width 6 will wrap "word1  " and then "word2" before the newline
    let _ = t.desired_height(6);

    // Put cursor on the second wrapped line before the newline, at column 1 of "word2"
    let start_word2 = t.text().find("word2").unwrap();
    t.set_cursor(start_word2 + 1);

    // Up should go to first wrapped line, column 1 -> index 1
    t.move_cursor_up();
    assert_eq!(t.cursor(), 1);

    // Down should return to the same visual column on "word2"
    t.move_cursor_down();
    assert_eq!(t.cursor(), start_word2 + 1);

    // Down again should cross the logical newline to the next visual line ("word3"), clamped to its length if needed
    t.move_cursor_down();
    let start_word3 = t.text().find("word3").unwrap();
    assert!(t.cursor() >= start_word3 && t.cursor() <= start_word3 + "word3".len());
}

#[test]
fn wrapped_navigation_with_wide_graphemes() {
    // Four thumbs up, each of display width 2, with width 3 to force wrapping inside grapheme boundaries
    let mut t = ta_with("👍👍👍👍");
    let _ = t.desired_height(3);

    // Put cursor after the second emoji (which should be on first wrapped line)
    t.set_cursor("👍👍".len());

    // Move down should go to the start of the next wrapped line (same column preserved but clamped)
    t.move_cursor_down();
    // We expect to land somewhere within the third emoji or at the start of it
    let pos_after_down = t.cursor();
    assert!(pos_after_down >= "👍👍".len());

    // Moving up should take us back to the original position
    t.move_cursor_up();
    assert_eq!(t.cursor(), "👍👍".len());
}

#[test]
fn fuzz_textarea_randomized() {
    // Deterministic seed for reproducibility
    // Seed the RNG based on the current day in Pacific Time (PST/PDT). This
    // keeps the fuzz test deterministic within a day while still varying
    // day-to-day to improve coverage.
    let pst_today_seed: u64 = (chrono::Utc::now() - chrono::Duration::hours(8))
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .unwrap()
        .and_utc()
        .timestamp() as u64;
    let mut rng = rand::rngs::StdRng::seed_from_u64(pst_today_seed);

    for _case in 0..500 {
        let mut ta = TextArea::new();
        let mut state = TextAreaState::default();
        // Track element payloads we insert. Payloads use characters '[' and ']' which
        // are not produced by rand_grapheme(), avoiding accidental collisions.
        let mut elem_texts: Vec<String> = Vec::new();
        let mut next_elem_id: usize = 0;
        // Start with a random base string
        let base_len = rng.random_range(0..30);
        let mut base = String::new();
        for _ in 0..base_len {
            base.push_str(&rand_grapheme(&mut rng));
        }
        ta.set_text(&base);
        // Choose a valid char boundary for initial cursor
        let mut boundaries: Vec<usize> = vec![0];
        boundaries.extend(ta.text().char_indices().map(|(i, _)| i).skip(1));
        boundaries.push(ta.text().len());
        let init = boundaries[rng.random_range(0..boundaries.len())];
        ta.set_cursor(init);

        let mut width: u16 = rng.random_range(1..=12);
        let mut height: u16 = rng.random_range(1..=4);

        for _step in 0..60 {
            // Mostly stable width/height, occasionally change
            if rng.random_bool(0.1) {
                width = rng.random_range(1..=12);
            }
            if rng.random_bool(0.1) {
                height = rng.random_range(1..=4);
            }

            // Pick an operation
            match rng.random_range(0..18) {
                0 => {
                    // insert small random string at cursor
                    let len = rng.random_range(0..6);
                    let mut s = String::new();
                    for _ in 0..len {
                        s.push_str(&rand_grapheme(&mut rng));
                    }
                    ta.insert_str(&s);
                }
                1 => {
                    // replace_range with small random slice
                    let mut b: Vec<usize> = vec![0];
                    b.extend(ta.text().char_indices().map(|(i, _)| i).skip(1));
                    b.push(ta.text().len());
                    let i1 = rng.random_range(0..b.len());
                    let i2 = rng.random_range(0..b.len());
                    let (start, end) = if b[i1] <= b[i2] {
                        (b[i1], b[i2])
                    } else {
                        (b[i2], b[i1])
                    };
                    let insert_len = rng.random_range(0..=4);
                    let mut s = String::new();
                    for _ in 0..insert_len {
                        s.push_str(&rand_grapheme(&mut rng));
                    }
                    let before = ta.text().len();
                    // If the chosen range intersects an element, replace_range will expand to
                    // element boundaries, so the naive size delta assertion does not hold.
                    let intersects_element = elem_texts.iter().any(|payload| {
                        if let Some(pstart) = ta.text().find(payload) {
                            let pend = pstart + payload.len();
                            pstart < end && pend > start
                        } else {
                            false
                        }
                    });
                    ta.replace_range(start..end, &s);
                    if !intersects_element {
                        let after = ta.text().len();
                        assert_eq!(
                            after as isize,
                            before as isize + (s.len() as isize) - ((end - start) as isize)
                        );
                    }
                }
                2 => ta.delete_backward(rng.random_range(0..=3)),
                3 => ta.delete_forward(rng.random_range(0..=3)),
                4 => ta.delete_backward_word(),
                5 => ta.kill_to_beginning_of_line(),
                6 => ta.kill_to_end_of_line(),
                7 => ta.move_cursor_left(),
                8 => ta.move_cursor_right(),
                9 => ta.move_cursor_up(),
                10 => ta.move_cursor_down(),
                11 => ta.move_cursor_to_beginning_of_line(true),
                12 => ta.move_cursor_to_end_of_line(true),
                13 => {
                    // Insert an element with a unique sentinel payload
                    let payload =
                        format!("[[EL#{}:{}]]", next_elem_id, rng.random_range(1000..9999));
                    next_elem_id += 1;
                    ta.insert_element(&payload);
                    elem_texts.push(payload);
                }
                14 => {
                    // Try inserting inside an existing element (should clamp to boundary)
                    if let Some(payload) = elem_texts.choose(&mut rng).cloned()
                        && let Some(start) = ta.text().find(&payload)
                    {
                        let end = start + payload.len();
                        if end - start > 2 {
                            let pos = rng.random_range(start + 1..end - 1);
                            let ins = rand_grapheme(&mut rng);
                            ta.insert_str_at(pos, &ins);
                        }
                    }
                }
                15 => {
                    // Replace a range that intersects an element -> whole element should be replaced
                    if let Some(payload) = elem_texts.choose(&mut rng).cloned()
                        && let Some(start) = ta.text().find(&payload)
                    {
                        let end = start + payload.len();
                        // Create an intersecting range [start-δ, end-δ2)
                        let mut s = start.saturating_sub(rng.random_range(0..=2));
                        let mut e = (end + rng.random_range(0..=2)).min(ta.text().len());
                        // Align to char boundaries to satisfy String::replace_range contract
                        let txt = ta.text();
                        while s > 0 && !txt.is_char_boundary(s) {
                            s -= 1;
                        }
                        while e < txt.len() && !txt.is_char_boundary(e) {
                            e += 1;
                        }
                        if s < e {
                            // Small replacement text
                            let mut srep = String::new();
                            for _ in 0..rng.random_range(0..=2) {
                                srep.push_str(&rand_grapheme(&mut rng));
                            }
                            ta.replace_range(s..e, &srep);
                        }
                    }
                }
                16 => {
                    // Try setting the cursor to a position inside an element; it should clamp out
                    if let Some(payload) = elem_texts.choose(&mut rng).cloned()
                        && let Some(start) = ta.text().find(&payload)
                    {
                        let end = start + payload.len();
                        if end - start > 2 {
                            let pos = rng.random_range(start + 1..end - 1);
                            ta.set_cursor(pos);
                        }
                    }
                }
                _ => {
                    // Jump to word boundaries
                    if rng.random_bool(0.5) {
                        let p = ta.beginning_of_previous_word();
                        ta.set_cursor(p);
                    } else {
                        let p = ta.end_of_next_word();
                        ta.set_cursor(p);
                    }
                }
            }

            // Sanity invariants
            assert!(ta.cursor() <= ta.text().len());

            // Element invariants
            for payload in &elem_texts {
                if let Some(start) = ta.text().find(payload) {
                    let end = start + payload.len();
                    // 1) Text inside elements matches the initially set payload
                    assert_eq!(&ta.text()[start..end], payload);
                    // 2) Cursor is never strictly inside an element
                    let c = ta.cursor();
                    assert!(
                        c <= start || c >= end,
                        "cursor inside element: {start}..{end} at {c}"
                    );
                }
            }

            // Render and compute cursor positions; ensure they are in-bounds and do not panic
            let area = Rect::new(0, 0, width, height);
            // Stateless render into an area tall enough for all wrapped lines
            let total_lines = ta.desired_height(width);
            let full_area = Rect::new(0, 0, width, total_lines.max(1));
            let mut buf = Buffer::empty(full_area);
            ratatui::widgets::WidgetRef::render_ref(&(&ta), full_area, &mut buf);

            // cursor_pos: x must be within width when present
            let _ = ta.cursor_pos(area);

            // cursor_pos_with_state: always within viewport rows
            let (_x, _y) = ta
                .cursor_pos_with_state(area, state)
                .unwrap_or((area.x, area.y));

            // Stateful render should not panic, and updates scroll
            let mut sbuf = Buffer::empty(area);
            ratatui::widgets::StatefulWidgetRef::render_ref(&(&ta), area, &mut sbuf, &mut state);

            // After wrapping, desired height equals the number of lines we would render without scroll
            let total_lines = total_lines as usize;
            // state.scroll must not exceed total_lines when content fits within area height
            if (height as usize) >= total_lines {
                assert_eq!(state.scroll, 0);
            }
        }
    }
}

// ===== Configurable hotkey tests =====

#[test]
fn test_configurable_ctrl_a_moves_to_line_start() {
    use codex_acp::config::HotkeyConfig;
    let mut t = ta_with("hello");
    t.set_hotkey_config(HotkeyConfig::default());
    // Cursor is at end (5) after insert
    pretty_assertions::assert_eq!(t.cursor(), 5);
    // Ctrl+A should move to beginning of line
    t.input(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL));
    pretty_assertions::assert_eq!(t.cursor(), 0);
}

#[test]
fn test_configurable_ctrl_e_moves_to_line_end() {
    use codex_acp::config::HotkeyConfig;
    let mut t = ta_with("hello");
    t.set_hotkey_config(HotkeyConfig::default());
    t.set_cursor(0);
    // Ctrl+E should move to end of line
    t.input(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::CONTROL));
    pretty_assertions::assert_eq!(t.cursor(), 5);
}

#[test]
fn test_rebound_move_backward_char() {
    use codex_acp::config::HotkeyBinding;
    use codex_acp::config::HotkeyConfig;
    let config = HotkeyConfig {
        move_backward_char: HotkeyBinding::from_str("alt+x"),
        ..HotkeyConfig::default()
    };
    let mut t = ta_with("abcd");
    t.set_hotkey_config(config);
    // Cursor at end (4)
    pretty_assertions::assert_eq!(t.cursor(), 4);

    // Alt+X (the rebound key) should move cursor left
    t.input(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::ALT));
    pretty_assertions::assert_eq!(t.cursor(), 3);

    // Ctrl+B should no longer move cursor (it's no longer bound to move backward)
    t.input(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::CONTROL));
    pretty_assertions::assert_eq!(t.cursor(), 3);
}

#[test]
fn test_unbound_editing_action_falls_through() {
    use codex_acp::config::HotkeyBinding;
    use codex_acp::config::HotkeyConfig;
    let config = HotkeyConfig {
        kill_to_end_of_line: HotkeyBinding::none(),
        ..HotkeyConfig::default()
    };
    let mut t = ta_with("hello");
    t.set_hotkey_config(config);
    t.set_cursor(2);
    // Ctrl+K should not kill because the action is unbound
    t.input(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL));
    pretty_assertions::assert_eq!(t.text(), "hello");
    pretty_assertions::assert_eq!(t.cursor(), 2);
}

#[test]
fn test_configurable_kill_and_yank() {
    use codex_acp::config::HotkeyConfig;
    let mut t = ta_with("hello world");
    t.set_hotkey_config(HotkeyConfig::default());
    t.set_cursor(5);
    // Ctrl+K should kill to end of line
    t.input(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL));
    pretty_assertions::assert_eq!(t.text(), "hello");
    pretty_assertions::assert_eq!(t.cursor(), 5);
    // Ctrl+Y should yank back
    t.input(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::CONTROL));
    pretty_assertions::assert_eq!(t.text(), "hello world");
    pretty_assertions::assert_eq!(t.cursor(), 11);
}

#[test]
fn test_configurable_word_movement() {
    use codex_acp::config::HotkeyConfig;
    let mut t = ta_with("foo bar baz");
    t.set_hotkey_config(HotkeyConfig::default());
    t.set_cursor(0);
    // Alt+F should move forward past "foo"
    t.input(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::ALT));
    pretty_assertions::assert_eq!(t.cursor(), 3);
    // Alt+B should move backward to start of "foo"
    t.input(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::ALT));
    pretty_assertions::assert_eq!(t.cursor(), 0);
}

// ===== Vim mode tests =====

#[test]
fn vim_mode_disabled_inserts_hjkl_as_characters() {
    // When vim mode is disabled (default), h/j/k/l should insert characters
    let mut t = ta_with("");
    t.set_vim_mode_enabled(false);
    t.input(KeyEvent::new(KeyCode::Char('h'), KeyModifiers::NONE));
    t.input(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
    t.input(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE));
    t.input(KeyEvent::new(KeyCode::Char('l'), KeyModifiers::NONE));
    pretty_assertions::assert_eq!(t.text(), "hjkl");
}

#[test]
fn vim_mode_enabled_starts_in_insert_mode() {
    // When vim mode is enabled, it should start in insert mode
    let mut t = ta_with("");
    t.set_vim_mode_enabled(true);
    pretty_assertions::assert_eq!(t.vim_mode_state(), VimModeState::Insert);
    // In insert mode, h/j/k/l should still insert characters
    t.input(KeyEvent::new(KeyCode::Char('h'), KeyModifiers::NONE));
    pretty_assertions::assert_eq!(t.text(), "h");
}

#[test]
fn vim_mode_escape_enters_normal_mode() {
    let mut t = ta_with("hello");
    t.set_vim_mode_enabled(true);
    pretty_assertions::assert_eq!(t.vim_mode_state(), VimModeState::Insert);

    // Escape should switch to normal mode
    t.enter_vim_normal_mode();
    pretty_assertions::assert_eq!(t.vim_mode_state(), VimModeState::Normal);
}

#[test]
fn vim_mode_normal_h_moves_cursor_left() {
    let mut t = ta_with("hello");
    t.set_vim_mode_enabled(true);
    t.set_cursor(3);
    t.enter_vim_normal_mode();

    // 'h' in normal mode should move cursor left
    t.input(KeyEvent::new(KeyCode::Char('h'), KeyModifiers::NONE));
    pretty_assertions::assert_eq!(t.cursor(), 2);
    // Text should NOT change (no insertion)
    pretty_assertions::assert_eq!(t.text(), "hello");
}

#[test]
fn vim_mode_normal_l_moves_cursor_right() {
    let mut t = ta_with("hello");
    t.set_vim_mode_enabled(true);
    t.set_cursor(2);
    t.enter_vim_normal_mode();

    // 'l' in normal mode should move cursor right
    t.input(KeyEvent::new(KeyCode::Char('l'), KeyModifiers::NONE));
    pretty_assertions::assert_eq!(t.cursor(), 3);
    pretty_assertions::assert_eq!(t.text(), "hello");
}

#[test]
fn vim_mode_normal_j_moves_cursor_down() {
    let mut t = ta_with("line1\nline2\nline3");
    t.set_vim_mode_enabled(true);
    t.set_cursor(2); // on line1
    t.enter_vim_normal_mode();

    // 'j' in normal mode should move cursor down
    t.input(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
    // Should be on line2 now
    assert!(t.cursor() > 5, "cursor should be on line2");
    pretty_assertions::assert_eq!(t.text(), "line1\nline2\nline3");
}

#[test]
fn vim_mode_normal_k_moves_cursor_up() {
    let mut t = ta_with("line1\nline2\nline3");
    t.set_vim_mode_enabled(true);
    t.set_cursor(8); // on line2
    t.enter_vim_normal_mode();

    // 'k' in normal mode should move cursor up
    t.input(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE));
    // Should be on line1 now
    assert!(t.cursor() < 6, "cursor should be on line1");
    pretty_assertions::assert_eq!(t.text(), "line1\nline2\nline3");
}

#[test]
fn vim_mode_normal_i_returns_to_insert_mode() {
    let mut t = ta_with("hello");
    t.set_vim_mode_enabled(true);
    t.enter_vim_normal_mode();
    pretty_assertions::assert_eq!(t.vim_mode_state(), VimModeState::Normal);

    // 'i' should return to insert mode
    t.input(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE));
    pretty_assertions::assert_eq!(t.vim_mode_state(), VimModeState::Insert);

    // Now 'h' should insert a character (cursor is at end, so "h" appends)
    t.input(KeyEvent::new(KeyCode::Char('h'), KeyModifiers::NONE));
    pretty_assertions::assert_eq!(t.text(), "helloh");
}

#[test]
fn vim_mode_disabled_ignores_enter_normal_mode() {
    let mut t = ta_with("hello");
    t.set_vim_mode_enabled(false);

    // enter_vim_normal_mode should be a no-op when vim mode is disabled
    t.enter_vim_normal_mode();
    pretty_assertions::assert_eq!(t.vim_mode_state(), VimModeState::Insert);
}

#[test]
fn vim_mode_toggle_resets_to_insert_mode() {
    let mut t = ta_with("hello");
    t.set_vim_mode_enabled(true);
    t.enter_vim_normal_mode();
    pretty_assertions::assert_eq!(t.vim_mode_state(), VimModeState::Normal);

    // Disabling vim mode should reset to insert mode
    t.set_vim_mode_enabled(false);
    pretty_assertions::assert_eq!(t.vim_mode_state(), VimModeState::Insert);
}

// ===== Helper to create a vim-enabled textarea in Normal mode =====
fn vim_normal(text: &str) -> TextArea {
    let mut t = ta_with(text);
    t.set_vim_mode_enabled(true);
    t.enter_vim_normal_mode();
    t
}

fn key(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
}

fn shift_key(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::SHIFT)
}

fn esc_key() -> KeyEvent {
    KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)
}

// ===== Insert mode entry variants =====

#[test]
fn vim_normal_a_enters_insert_after_cursor() {
    let mut t = vim_normal("hello");
    t.set_cursor(2); // on 'l'
    t.input(key('a'));
    pretty_assertions::assert_eq!(t.vim_mode_state(), VimModeState::Insert);
    pretty_assertions::assert_eq!(t.cursor(), 3); // moved right one
}

#[test]
fn vim_normal_shift_a_enters_insert_at_eol() {
    let mut t = vim_normal("hello\nworld");
    t.set_cursor(2); // on 'l' in "hello"
    t.input(shift_key('A'));
    pretty_assertions::assert_eq!(t.vim_mode_state(), VimModeState::Insert);
    pretty_assertions::assert_eq!(t.cursor(), 5); // end of "hello"
}

#[test]
fn vim_normal_shift_i_enters_insert_at_bol() {
    let mut t = vim_normal("hello\nworld");
    t.set_cursor(8); // on 'r' in "world"
    t.input(shift_key('I'));
    pretty_assertions::assert_eq!(t.vim_mode_state(), VimModeState::Insert);
    pretty_assertions::assert_eq!(t.cursor(), 6); // beginning of "world"
}

#[test]
fn vim_normal_o_opens_line_below() {
    let mut t = vim_normal("hello\nworld");
    t.set_cursor(2); // on 'l' in "hello"
    t.input(key('o'));
    pretty_assertions::assert_eq!(t.vim_mode_state(), VimModeState::Insert);
    pretty_assertions::assert_eq!(t.text(), "hello\n\nworld");
    pretty_assertions::assert_eq!(t.cursor(), 6); // on the new blank line
}

#[test]
fn vim_normal_shift_o_opens_line_above() {
    let mut t = vim_normal("hello\nworld");
    t.set_cursor(8); // on 'r' in "world"
    t.input(shift_key('O'));
    pretty_assertions::assert_eq!(t.vim_mode_state(), VimModeState::Insert);
    pretty_assertions::assert_eq!(t.text(), "hello\n\nworld");
    pretty_assertions::assert_eq!(t.cursor(), 6); // on the new blank line
}

// ===== Navigation =====

#[test]
fn vim_normal_w_moves_forward_word() {
    let mut t = vim_normal("hello world foo");
    t.set_cursor(0);
    t.input(key('w'));
    pretty_assertions::assert_eq!(t.cursor(), 6); // start of "world"

    // w from middle of whitespace should land on next word start
    let mut t = vim_normal("hello   world");
    t.set_cursor(5); // on first space
    t.input(key('w'));
    pretty_assertions::assert_eq!(t.cursor(), 8); // start of "world"
}

#[test]
fn vim_normal_b_moves_backward_word() {
    let mut t = vim_normal("hello world");
    t.set_cursor(8); // on 'r' in "world"
    t.input(key('b'));
    pretty_assertions::assert_eq!(t.cursor(), 6); // start of "world"
}

#[test]
fn vim_normal_0_moves_to_bol() {
    let mut t = vim_normal("hello\nworld");
    t.set_cursor(8); // on 'r' in "world"
    t.input(key('0'));
    pretty_assertions::assert_eq!(t.cursor(), 6); // beginning of "world" line
}

#[test]
fn vim_normal_dollar_moves_to_eol() {
    let mut t = vim_normal("hello\nworld");
    t.set_cursor(6); // beginning of "world"
    t.input(shift_key('$'));
    pretty_assertions::assert_eq!(t.cursor(), 11); // end of "world"
}

#[test]
fn vim_normal_caret_moves_to_first_nonwhitespace() {
    let mut t = vim_normal("   hello");
    t.set_cursor(6); // on 'l'
    t.input(shift_key('^'));
    pretty_assertions::assert_eq!(t.cursor(), 3); // first non-whitespace 'h'
}

#[test]
fn vim_normal_caret_on_all_whitespace_line_goes_to_eol() {
    let mut t = vim_normal("   \nhello");
    t.set_cursor(1); // in the whitespace line
    t.input(shift_key('^'));
    pretty_assertions::assert_eq!(t.cursor(), 3); // end of whitespace line (before \n)
}

#[test]
fn vim_normal_shift_g_moves_to_end_of_text() {
    let mut t = vim_normal("hello\nworld");
    t.set_cursor(0);
    t.input(shift_key('G'));
    pretty_assertions::assert_eq!(t.cursor(), 11); // end of text
}

#[test]
fn vim_normal_gg_moves_to_beginning_of_text() {
    let mut t = vim_normal("hello\nworld");
    t.set_cursor(8); // on 'r'
    t.input(key('g'));
    t.input(key('g'));
    pretty_assertions::assert_eq!(t.cursor(), 0);
}

#[test]
fn vim_normal_g_then_other_cancels_pending() {
    let mut t = vim_normal("hello\nworld");
    t.set_cursor(8);
    t.input(key('g'));
    t.input(key('x')); // not 'g', should cancel and do nothing
    pretty_assertions::assert_eq!(t.cursor(), 8); // unchanged
    pretty_assertions::assert_eq!(t.text(), "hello\nworld"); // unchanged
}

// ===== Editing =====

#[test]
fn vim_normal_x_deletes_char_under_cursor() {
    let mut t = vim_normal("hello");
    t.set_cursor(1); // on 'e'
    t.input(key('x'));
    pretty_assertions::assert_eq!(t.text(), "hllo");
    pretty_assertions::assert_eq!(t.cursor(), 1);
}

#[test]
fn vim_normal_shift_d_deletes_to_eol() {
    let mut t = vim_normal("hello\nworld");
    t.set_cursor(2); // on 'l' in "hello"
    t.input(shift_key('D'));
    pretty_assertions::assert_eq!(t.text(), "he\nworld");
    pretty_assertions::assert_eq!(t.cursor(), 2);
}

#[test]
fn vim_normal_shift_c_deletes_to_eol_and_enters_insert() {
    let mut t = vim_normal("hello\nworld");
    t.set_cursor(2); // on 'l' in "hello"
    t.input(shift_key('C'));
    pretty_assertions::assert_eq!(t.text(), "he\nworld");
    pretty_assertions::assert_eq!(t.cursor(), 2);
    pretty_assertions::assert_eq!(t.vim_mode_state(), VimModeState::Insert);
}

#[test]
fn vim_normal_dd_deletes_current_line() {
    let mut t = vim_normal("hello\nworld\nfoo");
    t.set_cursor(8); // on 'r' in "world"
    t.input(key('d'));
    t.input(key('d'));
    pretty_assertions::assert_eq!(t.text(), "hello\nfoo");
    pretty_assertions::assert_eq!(t.cursor(), 6); // beginning of "foo"
}

#[test]
fn vim_normal_dd_deletes_only_line() {
    let mut t = vim_normal("hello");
    t.set_cursor(2);
    t.input(key('d'));
    t.input(key('d'));
    pretty_assertions::assert_eq!(t.text(), "");
    pretty_assertions::assert_eq!(t.cursor(), 0);
}

#[test]
fn vim_normal_dd_deletes_last_line() {
    let mut t = vim_normal("hello\nworld");
    t.set_cursor(8); // on 'r' in "world"
    t.input(key('d'));
    t.input(key('d'));
    pretty_assertions::assert_eq!(t.text(), "hello");
}

#[test]
fn vim_normal_d_then_other_cancels_pending() {
    let mut t = vim_normal("hello");
    t.set_cursor(2);
    t.input(key('d'));
    t.input(key('x')); // not 'd', should cancel
    pretty_assertions::assert_eq!(t.text(), "hello"); // unchanged
    pretty_assertions::assert_eq!(t.cursor(), 2);
}

#[test]
fn vim_normal_p_pastes_from_kill_buffer() {
    let mut t = vim_normal("hello world");
    t.set_cursor(5); // on ' '
    // First kill to end of line to fill the kill buffer
    t.input(shift_key('D'));
    pretty_assertions::assert_eq!(t.text(), "hello");
    // Now paste it back
    t.input(key('p'));
    pretty_assertions::assert_eq!(t.text(), "hello world");
}

#[test]
fn vim_normal_dd_then_p_roundtrip() {
    let mut t = vim_normal("hello\nworld\nfoo");
    t.set_cursor(8); // on 'r' in "world"
    t.input(key('d'));
    t.input(key('d'));
    pretty_assertions::assert_eq!(t.text(), "hello\nfoo");
    // Paste the deleted line back
    t.input(key('p'));
    // The kill buffer should contain "world\n" (line + trailing newline)
    assert!(t.text().contains("world"));
}

// ===== Pending key edge cases =====

#[test]
fn vim_normal_pending_resets_on_esc() {
    let mut t = vim_normal("hello\nworld");
    t.set_cursor(8);
    t.input(key('g'));
    t.input(esc_key()); // Escape should cancel pending, NOT change mode
    pretty_assertions::assert_eq!(t.cursor(), 8); // unchanged
    pretty_assertions::assert_eq!(t.vim_mode_state(), VimModeState::Normal); // still Normal
}

// ===== Arrow key navigation in Normal mode =====

#[test]
fn vim_normal_arrow_up_moves_cursor_up() {
    let mut t = vim_normal("hello\nworld");
    t.set_cursor(8); // on 'r' in "world"
    t.input(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
    pretty_assertions::assert_eq!(t.cursor(), 2); // 'l' in "hello" (same column)
    pretty_assertions::assert_eq!(t.text(), "hello\nworld");
}

#[test]
fn vim_normal_arrow_down_moves_cursor_down() {
    let mut t = vim_normal("hello\nworld");
    t.set_cursor(2); // on 'l' in "hello"
    t.input(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    pretty_assertions::assert_eq!(t.cursor(), 8); // 'r' in "world" (same column)
    pretty_assertions::assert_eq!(t.text(), "hello\nworld");
}
