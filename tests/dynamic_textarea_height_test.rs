use nori_cli::ui::calculate_textarea_height;
use tui_components::textarea::{TextArea, TextAreaConfig};

#[test]
fn test_single_line_returns_minimum_height() {
    let mut textarea = TextArea::new(TextAreaConfig::default());
    textarea.insert_str("hello");

    let height = calculate_textarea_height(&textarea, 80);

    // MIN_HEIGHT (1) + BORDER_HEIGHT (0) = 1
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

    let height = calculate_textarea_height(&textarea, 80);

    // 3 lines + BORDER_HEIGHT (0) = 3
    assert_eq!(height, 3);
}

#[test]
fn test_multiline_bordered_returns_correct_height() {
    let mut textarea = TextArea::new(TextAreaConfig::default().with_padding(4, 5, 6, 7));
    textarea.insert_str("line 1");
    textarea.insert_str("\n");
    textarea.insert_str("line 2");
    textarea.insert_str("\n");
    textarea.insert_str("line 3");

    let height = calculate_textarea_height(&textarea, 80);

    // 3 lines + BORDER_HEIGHT (4 and 5) = 12
    assert_eq!(height, 12);
}

#[test]
fn test_long_line_accounts_for_wrapping() {
    let mut textarea = TextArea::new(TextAreaConfig::default());
    // Create a 250-character line (will wrap at width 80)
    // 250 / 80 = 4 wrapped lines (rounded up from 3.125)
    textarea.insert_str(&"a".repeat(250));

    let height = calculate_textarea_height(&textarea, 80);

    // 4 wrapped lines + BORDER_HEIGHT (0) = 4
    assert_eq!(
        height, 4,
        "Expected height 6 for wrapped line, got {height}"
    );
}

#[test]
fn test_height_respects_maximum_bound() {
    let mut textarea = TextArea::new(TextAreaConfig::default());
    // Create 20 lines
    for i in 0..20 {
        if i > 0 {
            textarea.insert_str("\n");
        }
        textarea.insert_str(&format!("line {i}"));
    }

    let height = calculate_textarea_height(&textarea, 80);

    // MAX_HEIGHT (10) + BORDER_HEIGHT (0) = 12
    assert_eq!(height, 10, "Expected maximum height of 12, got {height}");
}
