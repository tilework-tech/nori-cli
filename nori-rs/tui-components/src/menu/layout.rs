use std::borrow::Cow;

use textwrap::Options;
use unicode_width::UnicodeWidthChar;
use unicode_width::UnicodeWidthStr;

pub(super) fn wrap_lines(text: &str, width: u16, maximum: u16) -> Vec<String> {
    if width == 0 || maximum == 0 {
        return Vec::new();
    }
    let wrapped = textwrap::wrap(
        text,
        Options::new(usize::from(width))
            .break_words(true)
            .word_splitter(textwrap::WordSplitter::NoHyphenation),
    );
    let truncated = wrapped.len() > usize::from(maximum);
    let mut lines = wrapped
        .into_iter()
        .take(usize::from(maximum))
        .map(Cow::into_owned)
        .collect::<Vec<_>>();
    if truncated && let Some(last) = lines.last_mut() {
        *last = truncate_with_ellipsis(last, width);
    }
    lines
}

pub(super) fn truncate(text: &str, width: u16) -> String {
    if UnicodeWidthStr::width(text) <= usize::from(width) {
        return text.to_string();
    }
    truncate_with_ellipsis(text, width)
}

fn truncate_with_ellipsis(text: &str, width: u16) -> String {
    if width == 0 {
        return String::new();
    }
    let target = usize::from(width.saturating_sub(1));
    let mut rendered = String::new();
    let mut rendered_width = 0;
    for character in text.chars() {
        let character_width = character.width().unwrap_or(0);
        if rendered_width + character_width > target {
            break;
        }
        rendered.push(character);
        rendered_width += character_width;
    }
    rendered.push('…');
    rendered
}
