use textwrap;

/// Wraps text to fit within the specified width, breaking at word boundaries.
///
/// This function processes each line independently, preserving existing newlines
/// and wrapping long lines that exceed the specified width. The wrapping occurs
/// at word boundaries when possible, but will break long words if necessary.
///
/// # Arguments
///
/// * `text` - The text to wrap
/// * `width` - The maximum width in characters (using unicode width, not bytes)
///
/// # Returns
///
/// A String with newlines inserted to wrap the text to the specified width.
///
/// # Examples
///
/// ```
/// use nori_cli::text_wrapping::wrap_text_for_width;
///
/// let text = "This is a long line that needs wrapping";
/// let wrapped = wrap_text_for_width(text, 20);
/// assert!(wrapped.lines().count() > 1);
/// ```
pub fn wrap_text_for_width(text: &str, width: usize) -> String {
    if text.is_empty() {
        return String::new();
    }

    if width == 0 {
        return text.to_string();
    }

    let mut result = String::new();

    for line in text.lines() {
        let wrapped_line = textwrap::fill(line, width);
        result.push_str(&wrapped_line);
        result.push('\n');
    }

    // Remove trailing newline that textwrap adds
    if result.ends_with('\n') {
        result.pop();
    }

    result
}
