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
