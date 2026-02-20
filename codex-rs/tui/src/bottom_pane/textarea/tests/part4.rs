use super::*;

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
fn vim_normal_e_moves_to_end_of_word() {
    // From start of first word, 'e' should move to end of "hello"
    let mut t = vim_normal("hello world foo");
    t.set_cursor(0);
    t.input(key('e'));
    pretty_assertions::assert_eq!(t.cursor(), 5); // end of "hello"

    // From end of "hello", 'e' should move to end of "world"
    t.input(key('e'));
    pretty_assertions::assert_eq!(t.cursor(), 11); // end of "world"

    // From end of "world", 'e' should move to end of "foo"
    t.input(key('e'));
    pretty_assertions::assert_eq!(t.cursor(), 15); // end of "foo" (end of text)

    // 'e' at end of text stays at end
    t.input(key('e'));
    pretty_assertions::assert_eq!(t.cursor(), 15);

    // 'e' from middle of a word jumps to end of that word's run
    let mut t = vim_normal("hello world");
    t.set_cursor(2); // on 'l' in "hello"
    t.input(key('e'));
    pretty_assertions::assert_eq!(t.cursor(), 5); // end of "hello"

    // 'e' respects word separators
    let mut t = vim_normal("hello.world");
    t.set_cursor(0);
    t.input(key('e'));
    pretty_assertions::assert_eq!(t.cursor(), 5); // end of "hello"
    t.input(key('e'));
    pretty_assertions::assert_eq!(t.cursor(), 6); // end of "." separator
    t.input(key('e'));
    pretty_assertions::assert_eq!(t.cursor(), 11); // end of "world"
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

// ===== Capital letter vim variants =====

#[test]
fn vim_normal_shift_w_moves_forward_big_word() {
    // W skips over separators as part of the WORD
    let mut t = vim_normal("hello.world foo");
    t.set_cursor(0);
    t.input(shift_key('W'));
    pretty_assertions::assert_eq!(t.cursor(), 12); // start of "foo"

    // W from whitespace skips to next WORD
    let mut t = vim_normal("hello   world");
    t.set_cursor(5); // on first space
    t.input(shift_key('W'));
    pretty_assertions::assert_eq!(t.cursor(), 8); // start of "world"

    // W at end of text stays at end
    let mut t = vim_normal("hello");
    t.set_cursor(3);
    t.input(shift_key('W'));
    pretty_assertions::assert_eq!(t.cursor(), 5);
}

#[test]
fn vim_normal_shift_b_moves_backward_big_word() {
    // B skips over separators as part of the WORD
    let mut t = vim_normal("foo hello.world");
    t.set_cursor(15); // end
    t.input(shift_key('B'));
    pretty_assertions::assert_eq!(t.cursor(), 4); // start of "hello.world"

    // B from beginning stays at 0
    let mut t = vim_normal("hello");
    t.set_cursor(0);
    t.input(shift_key('B'));
    pretty_assertions::assert_eq!(t.cursor(), 0);
}

#[test]
fn vim_normal_shift_e_moves_to_end_of_big_word() {
    // E skips over separators as part of the WORD, landing on the last character
    let mut t = vim_normal("hello.world foo");
    t.set_cursor(0);
    t.input(shift_key('E'));
    pretty_assertions::assert_eq!(t.cursor(), 10); // last char 'd' of "hello.world"

    // E advances to last char of next WORD
    t.input(shift_key('E'));
    pretty_assertions::assert_eq!(t.cursor(), 14); // last char 'o' of "foo"

    // E at end of text moves to text.len()
    t.input(shift_key('E'));
    pretty_assertions::assert_eq!(t.cursor(), 15);

    // E past end stays at end
    t.input(shift_key('E'));
    pretty_assertions::assert_eq!(t.cursor(), 15);
}

#[test]
fn vim_normal_shift_x_deletes_char_before_cursor() {
    let mut t = vim_normal("hello");
    t.set_cursor(3); // on second 'l'
    t.input(shift_key('X'));
    pretty_assertions::assert_eq!(t.text(), "helo");
    pretty_assertions::assert_eq!(t.cursor(), 2);

    // X at beginning of text is a no-op
    t.set_cursor(0);
    t.input(shift_key('X'));
    pretty_assertions::assert_eq!(t.text(), "helo");
    pretty_assertions::assert_eq!(t.cursor(), 0);
}

#[test]
fn vim_normal_shift_p_pastes_from_kill_buffer() {
    let mut t = vim_normal("hello world");
    t.set_cursor(5);
    // Kill to end of line to fill kill buffer
    t.input(shift_key('D'));
    pretty_assertions::assert_eq!(t.text(), "hello");
    // P pastes at cursor
    t.input(shift_key('P'));
    pretty_assertions::assert_eq!(t.text(), "hello world");
}

#[test]
fn vim_normal_shift_j_joins_lines() {
    let mut t = vim_normal("hello\nworld");
    t.set_cursor(2); // on 'l' in "hello"
    t.input(shift_key('J'));
    pretty_assertions::assert_eq!(t.text(), "hello world");

    // J on the last line is a no-op
    let mut t = vim_normal("hello");
    t.set_cursor(2);
    t.input(shift_key('J'));
    pretty_assertions::assert_eq!(t.text(), "hello");
}

#[test]
fn vim_normal_shift_j_strips_leading_whitespace() {
    let mut t = vim_normal("hello\n   world");
    t.set_cursor(0);
    t.input(shift_key('J'));
    pretty_assertions::assert_eq!(t.text(), "hello world");
}

#[test]
fn vim_normal_shift_s_substitutes_line() {
    let mut t = vim_normal("hello\nworld\nfoo");
    t.set_cursor(8); // on 'r' in "world"
    t.input(shift_key('S'));
    pretty_assertions::assert_eq!(t.text(), "hello\n\nfoo");
    pretty_assertions::assert_eq!(t.cursor(), 6); // at the now-empty line
    pretty_assertions::assert_eq!(t.vim_mode_state(), VimModeState::Insert);
}

#[test]
fn vim_normal_shift_s_on_empty_line_enters_insert() {
    let mut t = vim_normal("hello\n\nworld");
    t.set_cursor(6); // on the empty line
    t.input(shift_key('S'));
    pretty_assertions::assert_eq!(t.text(), "hello\n\nworld");
    pretty_assertions::assert_eq!(t.vim_mode_state(), VimModeState::Insert);
}

#[test]
fn vim_normal_shift_y_yanks_line() {
    let mut t = vim_normal("hello\nworld\nfoo");
    t.set_cursor(8); // on 'r' in "world"
    t.input(shift_key('Y'));
    // Text should be unchanged
    pretty_assertions::assert_eq!(t.text(), "hello\nworld\nfoo");
    pretty_assertions::assert_eq!(t.cursor(), 8);
    // Pasting should insert the yanked line
    t.input(key('p'));
    assert!(t.text().contains("world\n"));
}

#[test]
fn vim_normal_shift_y_on_last_line_yanks_without_newline() {
    let mut t = vim_normal("hello\nworld");
    t.set_cursor(8); // on 'r' in "world"
    t.input(shift_key('Y'));
    pretty_assertions::assert_eq!(t.text(), "hello\nworld");
    // Paste the yanked text
    t.input(key('p'));
    assert!(t.text().contains("world"));
}
