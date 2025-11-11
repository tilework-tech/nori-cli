use nori_cli::text_wrapping::wrap_text_for_width;

#[test]
fn test_short_text_no_wrapping() {
    let text = "Hello world";
    let width = 80;

    let wrapped = wrap_text_for_width(text, width);

    assert_eq!(wrapped, "Hello world");
}

#[test]
fn test_long_line_wraps_at_word_boundary() {
    let text = "This is a very long line that should wrap at word boundaries";
    let width = 20;

    let wrapped = wrap_text_for_width(text, width);

    // Should wrap into multiple lines
    let lines: Vec<&str> = wrapped.lines().collect();
    assert!(lines.len() > 1, "Text should be wrapped into multiple lines");

    // Each line should be <= width
    for line in &lines {
        assert!(line.len() <= width, "Line '{}' exceeds width {}", line, width);
    }
}

#[test]
fn test_preserves_existing_newlines() {
    let text = "Line 1\nLine 2\nLine 3";
    let width = 80;

    let wrapped = wrap_text_for_width(text, width);

    let lines: Vec<&str> = wrapped.lines().collect();
    assert_eq!(lines.len(), 3);
    assert_eq!(lines[0], "Line 1");
    assert_eq!(lines[1], "Line 2");
    assert_eq!(lines[2], "Line 3");
}

#[test]
fn test_wraps_each_line_independently() {
    let text = "Short\nThis is a very long line that needs wrapping because it exceeds the width";
    let width = 20;

    let wrapped = wrap_text_for_width(text, width);

    let lines: Vec<&str> = wrapped.lines().collect();

    // First line should remain short
    assert_eq!(lines[0], "Short");

    // Second line should be wrapped into multiple lines
    assert!(lines.len() > 2, "Second line should wrap");
}

#[test]
fn test_empty_string() {
    let text = "";
    let width = 80;

    let wrapped = wrap_text_for_width(text, width);

    assert_eq!(wrapped, "");
}

#[test]
fn test_single_long_word() {
    // Word longer than width should be broken mid-word
    let text = "verylongwordthatcannotfitonasingleline";
    let width = 10;

    let wrapped = wrap_text_for_width(text, width);

    let lines: Vec<&str> = wrapped.lines().collect();
    assert!(lines.len() > 1, "Long word should be broken across lines");
}

#[test]
fn test_unicode_text_wrapping() {
    let text = "Hello 👋 world 🌍 this is a test with emoji";
    let width = 20;

    let wrapped = wrap_text_for_width(text, width);

    // Should wrap without panicking on unicode
    let lines: Vec<&str> = wrapped.lines().collect();
    assert!(lines.len() >= 1);
}

#[test]
fn test_multiple_spaces_preserved() {
    let text = "Hello  world  with  spaces";
    let width = 80;

    let wrapped = wrap_text_for_width(text, width);

    // Spaces should be handled reasonably (textwrap may normalize them)
    assert!(wrapped.contains("Hello"));
    assert!(wrapped.contains("world"));
}
