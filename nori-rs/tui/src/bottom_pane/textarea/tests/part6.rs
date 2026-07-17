use super::*;

#[test]
fn vim_delete_operator_applies_word_motion() {
    let mut t = vim_normal("hello world");
    t.set_cursor(0);

    t.input(key('d'));
    t.input(key('w'));

    pretty_assertions::assert_eq!(t.text(), "world");
    pretty_assertions::assert_eq!(t.cursor(), 0);
}

#[test]
fn vim_linewise_yank_pastes_below_current_line() {
    let mut t = vim_normal("abc\n123\nxyz");
    t.set_cursor(1);

    t.input(key('y'));
    t.input(key('y'));
    t.input(key('p'));

    pretty_assertions::assert_eq!(t.text(), "abc\nabc\n123\nxyz");
    pretty_assertions::assert_eq!(t.cursor(), "abc\n".len());
}

#[test]
fn vim_change_inner_word_enters_insert_and_preserves_nori_undo() {
    let mut t = vim_normal("hello world");
    t.set_cursor("hello ".len());

    t.input(key('c'));
    t.input(key('i'));
    t.input(key('w'));

    pretty_assertions::assert_eq!(t.text(), "hello ");
    pretty_assertions::assert_eq!(t.cursor(), "hello ".len());
    pretty_assertions::assert_eq!(t.vim_mode_state(), VimModeState::Insert);

    t.input(key('N'));
    t.input(esc_key());
    t.input(key('u'));

    pretty_assertions::assert_eq!(t.text(), "hello world");
    pretty_assertions::assert_eq!(t.vim_mode_state(), VimModeState::Normal);
}

#[test]
fn vim_change_word_uses_ce_semantics_and_preserves_whitespace() {
    let mut t = vim_normal("hello world");
    t.set_cursor(1);

    t.input(key('c'));
    t.input(key('w'));

    pretty_assertions::assert_eq!(t.text(), "h world");
    pretty_assertions::assert_eq!(t.cursor(), 1);
    pretty_assertions::assert_eq!(t.vim_mode_state(), VimModeState::Insert);

    t.input(key('i'));
    t.input(esc_key());
    t.input(key('u'));
    pretty_assertions::assert_eq!(t.text(), "hello world");
}

#[test]
fn vim_change_to_line_end_enters_insert() {
    let mut t = vim_normal("hello world");
    t.set_cursor(5);

    t.input(key('c'));
    t.input(shift_key('$'));

    pretty_assertions::assert_eq!(t.text(), "hello");
    pretty_assertions::assert_eq!(t.vim_mode_state(), VimModeState::Insert);
}

#[test]
fn vim_change_line_matches_shift_s() {
    let mut t = vim_normal("hello\nworld\nfoo");
    t.set_cursor(8);

    t.input(key('c'));
    t.input(key('c'));

    pretty_assertions::assert_eq!(t.text(), "hello\n\nfoo");
    pretty_assertions::assert_eq!(t.cursor(), 6);
    pretty_assertions::assert_eq!(t.vim_mode_state(), VimModeState::Insert);
}

#[test]
fn vim_delimiter_text_objects_select_innermost_pair() {
    let mut t = vim_normal("a(b(c)d)e");
    t.set_cursor("a(b(".len());

    t.input(key('c'));
    t.input(key('i'));
    t.input(key('('));

    pretty_assertions::assert_eq!(t.text(), "a(b()d)e");
    pretty_assertions::assert_eq!(t.vim_mode_state(), VimModeState::Insert);
}

#[test]
fn vim_around_quote_text_object_includes_delimiters() {
    let mut t = vim_normal(r#"say "hello world" now"#);
    t.set_cursor(r#"say "hello"#.len());

    t.input(key('d'));
    t.input(key('a'));
    t.input(key('"'));

    pretty_assertions::assert_eq!(t.text(), "say  now");
    pretty_assertions::assert_eq!(t.vim_mode_state(), VimModeState::Normal);
}

#[test]
fn vim_invalid_operator_motion_is_consumed_without_editing() {
    let mut t = vim_normal("hello");
    t.set_cursor(0);

    t.input(key('d'));
    t.input(key('z'));

    pretty_assertions::assert_eq!(t.text(), "hello");
    pretty_assertions::assert_eq!(t.cursor(), 0);
    pretty_assertions::assert_eq!(t.vim_mode_state(), VimModeState::Normal);
}

#[test]
fn vim_modified_escape_does_not_leave_insert_mode() {
    let mut t = ta_with("hello");
    t.set_vim_mode_enabled(true);
    t.set_cursor(3);

    t.input(KeyEvent::new(KeyCode::Esc, KeyModifiers::ALT));

    pretty_assertions::assert_eq!(t.vim_mode_state(), VimModeState::Insert);
    pretty_assertions::assert_eq!(t.cursor(), 3);
}

#[test]
fn vim_escape_from_insert_respects_atomic_element_boundary() {
    let mut t = TextArea::new();
    t.insert_str("a");
    t.insert_element("<element>");
    t.set_vim_mode_enabled(true);

    t.input(esc_key());

    pretty_assertions::assert_eq!(t.vim_mode_state(), VimModeState::Normal);
    pretty_assertions::assert_eq!(t.cursor(), 1);
}
