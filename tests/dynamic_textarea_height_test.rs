use tui_components::textarea::{TextArea, TextAreaConfig};

#[test]
fn test_single_line_returns_minimum_height() {
    let mut textarea = TextArea::new(TextAreaConfig::default());
    textarea.insert_str("hello");

    let height = textarea.desired_height(80);

    // MIN_HEIGHT (1) + padding (0) = 1
    assert_eq!(height, 1);
}

#[test]
fn test_multiline_returns_correct_height() {
    let mut textarea = TextArea::new(TextAreaConfig::default());
    textarea.insert_str("line 1");
    textarea.insert_str("\n");
    textarea.insert_str("line 2");
    textarea.insert_str("\n");
    textarea.insert_str("line 3");

    let height = textarea.desired_height(80);

    // 3 lines + padding (0) = 3
    assert_eq!(height, 3);
}

#[test]
fn test_multiline_bordered_returns_correct_height() {
    let textarea = TextArea::new(TextAreaConfig::default().with_padding(4, 5, 6, 7));
    let mut textarea = textarea;
    textarea.insert_str("line 1");
    textarea.insert_str("\n");
    textarea.insert_str("line 2");
    textarea.insert_str("\n");
    textarea.insert_str("line 3");

    let content_height = textarea.desired_height(80);
    let config = textarea.config();
    let total_height = content_height + config.padding_top + config.padding_bottom;

    // 3 lines content + padding_top (4) + padding_bottom (5) = 12
    assert_eq!(total_height, 12);
}

#[test]
fn test_long_line_accounts_for_wrapping() {
    let mut textarea = TextArea::new(TextAreaConfig::default());
    // Create a 250-character line (will wrap at width 80)
    // 250 / 80 = 4 wrapped lines (rounded up from 3.125)
    textarea.insert_str(&"a".repeat(250));

    let height = textarea.desired_height(80);

    // 4 wrapped lines + padding (0) = 4
    assert_eq!(
        height, 4,
        "Expected height 4 for wrapped line, got {height}"
    );
}

#[test]
fn test_desired_height_returns_actual_line_count() {
    let mut textarea = TextArea::new(TextAreaConfig::default());
    // Create 20 lines
    for i in 0..20 {
        if i > 0 {
            textarea.insert_str("\n");
        }
        textarea.insert_str(&format!("line {i}"));
    }

    let height = textarea.desired_height(80);

    // TextArea.desired_height() returns actual line count (no built-in max)
    // UI code should apply max height constraint if needed
    assert_eq!(height, 20, "Expected height of 20 lines, got {height}");
}
