use super::*;

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
