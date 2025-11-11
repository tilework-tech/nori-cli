use nori_cli::ui::calculate_textarea_height;
use tui_textarea::TextArea;

#[test]
fn test_single_line_returns_minimum_height() {
    let mut textarea = TextArea::default();
    textarea.insert_str("hello");

    let height = calculate_textarea_height(&textarea, 80);

    // MIN_HEIGHT (3) + BORDER_HEIGHT (2) = 5
    assert_eq!(height, 5);
}

#[test]
fn test_multiline_returns_correct_height() {
    let mut textarea = TextArea::default();
    textarea.insert_str("line 1");
    textarea.insert_newline();
    textarea.insert_str("line 2");
    textarea.insert_newline();
    textarea.insert_str("line 3");

    let height = calculate_textarea_height(&textarea, 80);

    // 3 lines + BORDER_HEIGHT (2) = 5
    assert_eq!(height, 5);
}
