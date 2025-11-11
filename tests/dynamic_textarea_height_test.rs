use nori_cli::ui::calculate_textarea_height;
use tui_textarea::TextArea;

#[test]
fn test_empty_textarea_height() {
    let textarea = TextArea::default();
    let available_width = 80;

    let height = calculate_textarea_height(&textarea, available_width);

    // Empty textarea should have minimum height: 1 content line + 2 borders = 3
    assert_eq!(height, 3);
}

#[test]
fn test_single_short_line_height() {
    let mut textarea = TextArea::default();
    textarea.insert_str("Hello");
    let available_width = 80;

    let height = calculate_textarea_height(&textarea, available_width);

    // Single short line: 1 content line + 2 borders = 3
    assert_eq!(height, 3);
}

#[test]
fn test_single_wrapping_line_height() {
    let mut textarea = TextArea::default();
    // Create a line that's 100 characters long
    textarea.insert_str(&"a".repeat(100));
    let available_width = 50; // Available width after borders

    let height = calculate_textarea_height(&textarea, available_width);

    // 100 chars at width 48 (50 - 2 for borders) = 3 visual rows
    // 3 content rows + 2 borders = 5
    assert_eq!(height, 5);
}

#[test]
fn test_multiple_lines_height() {
    let mut textarea = TextArea::default();
    textarea.insert_str("Line 1\nLine 2\nLine 3");
    let available_width = 80;

    let height = calculate_textarea_height(&textarea, available_width);

    // 3 content lines + 2 borders = 5
    assert_eq!(height, 5);
}

#[test]
fn test_no_maximum_height() {
    let mut textarea = TextArea::default();
    // Create 20 lines
    for i in 0..20 {
        if i > 0 {
            textarea.insert_char('\n');
        }
        textarea.insert_str(&format!("Line {}", i));
    }
    let available_width = 80;

    let height = calculate_textarea_height(&textarea, available_width);

    // 20 content lines + 2 borders = 22 (no clamping)
    assert_eq!(height, 22);
}

#[test]
fn test_unicode_width_handling() {
    let mut textarea = TextArea::default();
    // Emoji are typically 2 units wide
    textarea.insert_str("Hello 👋 World");
    let available_width = 80;

    let height = calculate_textarea_height(&textarea, available_width);

    // Should handle unicode width correctly - still fits in one line
    assert_eq!(height, 3);
}

#[test]
fn test_very_small_width() {
    let mut textarea = TextArea::default();
    textarea.insert_str("Hello");
    let available_width = 2; // Smaller than border width

    let height = calculate_textarea_height(&textarea, available_width);

    // Should handle gracefully, return at least minimum height
    assert!(height >= 3);
}
