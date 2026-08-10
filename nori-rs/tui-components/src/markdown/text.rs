use ratatui::text::Line;
use ratatui::text::Span;
use unicode_width::UnicodeWidthChar;
use unicode_width::UnicodeWidthStr;

pub(super) fn wrap_line(line: Line<'static>, width: u16, prefix: &str) -> Vec<Line<'static>> {
    let width = width.max(1) as usize;
    let prefix_width = prefix.width();
    let content_width = width.saturating_sub(prefix_width).max(1);
    let mut result = Vec::new();
    let mut current = Vec::new();
    let mut current_width = 0;

    for span in line.spans {
        let style = span.style;
        for token in tokens(&span.content) {
            let token_width = token.width();
            if !current.is_empty()
                && !token.trim().is_empty()
                && current_width + token_width > content_width
            {
                result.push(prefixed_line(prefix, std::mem::take(&mut current)));
                current_width = 0;
            }
            if token_width <= content_width {
                current.push(Span::styled(token, style));
                current_width += token_width;
                continue;
            }
            for character in token.chars() {
                let character_width = character.width().unwrap_or(0);
                if current_width + character_width > content_width && !current.is_empty() {
                    result.push(prefixed_line(prefix, std::mem::take(&mut current)));
                    current_width = 0;
                }
                current.push(Span::styled(character.to_string(), style));
                current_width += character_width;
            }
        }
    }
    if !current.is_empty() {
        result.push(prefixed_line(prefix, current));
    }
    if result.is_empty() {
        result.push(Line::from(prefix.to_string()));
    }
    result
}

fn tokens(value: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut whitespace = None;
    for character in value.chars() {
        let is_whitespace = character.is_whitespace();
        if whitespace.is_some_and(|previous| previous != is_whitespace) && !current.is_empty() {
            tokens.push(std::mem::take(&mut current));
        }
        whitespace = Some(is_whitespace);
        current.push(character);
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

fn prefixed_line(prefix: &str, mut spans: Vec<Span<'static>>) -> Line<'static> {
    if prefix.is_empty() {
        return Line::from(spans);
    }
    let mut prefixed = vec![Span::raw(prefix.to_string())];
    prefixed.append(&mut spans);
    Line::from(prefixed)
}
