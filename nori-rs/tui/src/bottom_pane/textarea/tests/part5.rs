use super::*;

// ===== Undo/Redo basic behavior =====

#[test]
fn undo_after_insert_reverts_text() {
    let mut t = ta_with("hello");
    t.input(key(' '));
    t.input(key('w'));
    t.input(key('o'));
    t.input(key('r'));
    t.input(key('l'));
    t.input(key('d'));
    pretty_assertions::assert_eq!(t.text(), "hello world");
    t.undo();
    // Each char is a separate undo entry (non-vim mode, no grouping)
    // After one undo we should revert the last char insertion
    pretty_assertions::assert_eq!(t.text(), "hello worl");
}

#[test]
fn redo_after_undo_restores_text() {
    let mut t = ta_with("hello");
    t.input(key(' '));
    pretty_assertions::assert_eq!(t.text(), "hello ");
    t.undo();
    pretty_assertions::assert_eq!(t.text(), "hello");
    t.redo();
    pretty_assertions::assert_eq!(t.text(), "hello ");
}

#[test]
fn multiple_undo_steps() {
    let mut t = ta_with("");
    t.input(key('a'));
    t.input(key('b'));
    t.input(key('c'));
    pretty_assertions::assert_eq!(t.text(), "abc");
    t.undo();
    pretty_assertions::assert_eq!(t.text(), "ab");
    t.undo();
    pretty_assertions::assert_eq!(t.text(), "a");
    t.undo();
    pretty_assertions::assert_eq!(t.text(), "");
}

#[test]
fn redo_cleared_on_new_edit() {
    let mut t = ta_with("");
    t.input(key('a'));
    t.input(key('b'));
    t.undo();
    pretty_assertions::assert_eq!(t.text(), "a");
    // Now make a new edit — redo stack should be cleared
    t.input(key('z'));
    pretty_assertions::assert_eq!(t.text(), "az");
    t.redo();
    // Redo should be a no-op since the redo stack was cleared
    pretty_assertions::assert_eq!(t.text(), "az");
}

#[test]
fn undo_on_empty_stack_is_noop() {
    // Use set_text to initialize, which clears undo/redo stacks
    let mut t = TextArea::new();
    t.set_text("hello");
    let text_before = t.text().to_string();
    let cursor_before = t.cursor();
    t.undo();
    pretty_assertions::assert_eq!(t.text(), text_before);
    pretty_assertions::assert_eq!(t.cursor(), cursor_before);
}

#[test]
fn redo_on_empty_stack_is_noop() {
    let mut t = TextArea::new();
    t.set_text("hello");
    let text_before = t.text().to_string();
    let cursor_before = t.cursor();
    t.redo();
    pretty_assertions::assert_eq!(t.text(), text_before);
    pretty_assertions::assert_eq!(t.cursor(), cursor_before);
}

#[test]
fn set_text_clears_undo_redo_stacks() {
    let mut t = ta_with("");
    t.input(key('a'));
    t.input(key('b'));
    t.input(key('c'));
    // Undo once so there's something in both stacks
    t.undo();
    pretty_assertions::assert_eq!(t.text(), "ab");
    // Now set_text replaces everything
    t.set_text("new text");
    // Undo should be a no-op
    t.undo();
    pretty_assertions::assert_eq!(t.text(), "new text");
    // Redo should also be a no-op
    t.redo();
    pretty_assertions::assert_eq!(t.text(), "new text");
}

#[test]
fn undo_restores_cursor_position() {
    let mut t = ta_with("hello");
    let cursor_before = t.cursor(); // 5
    t.input(key('!'));
    pretty_assertions::assert_eq!(t.cursor(), 6);
    t.undo();
    pretty_assertions::assert_eq!(t.cursor(), cursor_before);
}

// ===== Vim insert-session grouping =====

#[test]
fn vim_undo_groups_insert_session() {
    // In vim, everything typed during one insert session should be one undo group
    let mut t = vim_normal("hello");
    t.set_cursor(5);
    // Enter insert mode with 'a' (append after cursor)
    t.input(key('a'));
    pretty_assertions::assert_eq!(t.vim_mode_state(), VimModeState::Insert);
    // Type " world"
    t.input(key(' '));
    t.input(key('w'));
    t.input(key('o'));
    t.input(key('r'));
    t.input(key('l'));
    t.input(key('d'));
    pretty_assertions::assert_eq!(t.text(), "hello world");
    // Return to normal mode
    t.input(esc_key());
    pretty_assertions::assert_eq!(t.vim_mode_state(), VimModeState::Normal);
    // 'u' should undo the entire insert session
    t.input(key('u'));
    pretty_assertions::assert_eq!(t.text(), "hello");
}

#[test]
fn vim_undo_normal_mode_edit_is_single_entry() {
    // Normal mode commands like 'x' should each be a separate undo entry
    let mut t = vim_normal("hello");
    t.set_cursor(1);
    t.input(key('x')); // delete 'e'
    pretty_assertions::assert_eq!(t.text(), "hllo");
    t.input(key('x')); // delete first 'l'
    pretty_assertions::assert_eq!(t.text(), "hlo");
    // First undo reverts the second 'x'
    t.input(key('u'));
    pretty_assertions::assert_eq!(t.text(), "hllo");
    // Second undo reverts the first 'x'
    t.input(key('u'));
    pretty_assertions::assert_eq!(t.text(), "hello");
}

#[test]
fn vim_ctrl_r_in_normal_mode_triggers_redo() {
    let mut t = vim_normal("hello");
    t.set_cursor(1);
    t.input(key('x')); // delete 'e' -> "hllo"
    t.input(key('u')); // undo -> "hello"
    pretty_assertions::assert_eq!(t.text(), "hello");
    // Ctrl-R in normal mode should redo
    t.input(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL));
    pretty_assertions::assert_eq!(t.text(), "hllo");
}

#[test]
fn vim_undo_dd_restores_deleted_line() {
    let mut t = vim_normal("hello\nworld\nfoo");
    t.set_cursor(8);
    t.input(key('d'));
    t.input(key('d'));
    pretty_assertions::assert_eq!(t.text(), "hello\nfoo");
    t.input(key('u'));
    pretty_assertions::assert_eq!(t.text(), "hello\nworld\nfoo");
}

#[test]
fn vim_undo_shift_d_restores_killed_text() {
    let mut t = vim_normal("hello world");
    t.set_cursor(5);
    t.input(shift_key('D')); // kill to end of line
    pretty_assertions::assert_eq!(t.text(), "hello");
    t.input(key('u'));
    pretty_assertions::assert_eq!(t.text(), "hello world");
}

#[test]
fn vim_undo_shift_c_restores_text_and_returns_to_normal() {
    let mut t = vim_normal("hello world");
    t.set_cursor(5);
    t.input(shift_key('C')); // kill to end + enter insert
    pretty_assertions::assert_eq!(t.text(), "hello");
    pretty_assertions::assert_eq!(t.vim_mode_state(), VimModeState::Insert);
    // Type some replacement text
    t.input(key('!'));
    pretty_assertions::assert_eq!(t.text(), "hello!");
    // Return to normal
    t.input(esc_key());
    // Undo should revert both the kill and the typed text (one insert session)
    t.input(key('u'));
    pretty_assertions::assert_eq!(t.text(), "hello world");
}

#[test]
fn vim_undo_o_reverts_new_line_and_typed_text() {
    let mut t = vim_normal("hello\nworld");
    t.set_cursor(2);
    t.input(key('o')); // open line below, enter insert
    t.input(key('f'));
    t.input(key('o'));
    t.input(key('o'));
    pretty_assertions::assert_eq!(t.text(), "hello\nfoo\nworld");
    t.input(esc_key());
    // Undo should revert the entire insert session including the 'o' new line
    t.input(key('u'));
    pretty_assertions::assert_eq!(t.text(), "hello\nworld");
}

#[test]
fn vim_undo_shift_s_reverts_substitution_and_typed_text() {
    let mut t = vim_normal("hello\nworld\nfoo");
    t.set_cursor(8);
    t.input(shift_key('S')); // substitute line
    t.input(key('b'));
    t.input(key('a'));
    t.input(key('r'));
    pretty_assertions::assert_eq!(t.text(), "hello\nbar\nfoo");
    t.input(esc_key());
    t.input(key('u'));
    pretty_assertions::assert_eq!(t.text(), "hello\nworld\nfoo");
}

#[test]
fn vim_undo_join_lines() {
    let mut t = vim_normal("hello\nworld");
    t.set_cursor(2);
    t.input(shift_key('J'));
    pretty_assertions::assert_eq!(t.text(), "hello world");
    t.input(key('u'));
    pretty_assertions::assert_eq!(t.text(), "hello\nworld");
}

#[test]
fn vim_undo_paste() {
    let mut t = vim_normal("hello world");
    t.set_cursor(5);
    t.input(shift_key('D')); // kill " world"
    pretty_assertions::assert_eq!(t.text(), "hello");
    t.input(key('p')); // paste " world" back
    pretty_assertions::assert_eq!(t.text(), "hello world");
    // Undo the paste
    t.input(key('u'));
    pretty_assertions::assert_eq!(t.text(), "hello");
    // Undo the kill
    t.input(key('u'));
    pretty_assertions::assert_eq!(t.text(), "hello world");
}

#[test]
fn vim_multiple_insert_sessions_each_undoable() {
    let mut t = vim_normal("");
    // First insert session: type "hello"
    t.input(key('i'));
    t.input(key('h'));
    t.input(key('e'));
    t.input(key('l'));
    t.input(key('l'));
    t.input(key('o'));
    t.input(esc_key());
    pretty_assertions::assert_eq!(t.text(), "hello");
    // Second insert session: append " world"
    t.input(shift_key('A'));
    t.input(key(' '));
    t.input(key('w'));
    t.input(key('o'));
    t.input(key('r'));
    t.input(key('l'));
    t.input(key('d'));
    t.input(esc_key());
    pretty_assertions::assert_eq!(t.text(), "hello world");
    // Undo second session
    t.input(key('u'));
    pretty_assertions::assert_eq!(t.text(), "hello");
    // Undo first session
    t.input(key('u'));
    pretty_assertions::assert_eq!(t.text(), "");
}

#[test]
fn vim_redo_after_undo_insert_session() {
    let mut t = vim_normal("");
    t.input(key('i'));
    t.input(key('h'));
    t.input(key('i'));
    t.input(esc_key());
    pretty_assertions::assert_eq!(t.text(), "hi");
    t.input(key('u'));
    pretty_assertions::assert_eq!(t.text(), "");
    // Ctrl-R to redo
    t.input(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL));
    pretty_assertions::assert_eq!(t.text(), "hi");
}
