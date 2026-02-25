use ratatui::text::{Line, Span, Text};
use unicode_width::UnicodeWidthStr;

/// Wrap text to fit within the specified width, preserving styling from spans.
/// Returns a vector of Lines, each fitting within the width constraint.
pub fn wrap_text_to_width(text: &Text, width: usize) -> Vec<Line<'static>> {
    let mut wrapped_lines = Vec::new();

    // If width is too small, just return the original line clones
    if width < 10 {
        for line in &text.lines {
            let owned_spans: Vec<Span> = line
                .spans
                .iter()
                .map(|s| Span::styled(s.content.to_string(), s.style))
                .collect();
            wrapped_lines.push(Line::from(owned_spans));
        }
        return wrapped_lines;
    }

    for line in &text.lines {
        let mut current_line_spans = Vec::new();
        let mut current_width = 0;

        for span in &line.spans {
            let span_text = span.content.as_ref();

            // Handle standalone space spans (don't split them)
            if span_text.trim().is_empty() && !span_text.is_empty() {
                current_line_spans.push(Span::styled(span_text.to_string(), span.style));
                current_width += span_text.width();
                continue;
            }

            // Split span text by words to wrap properly
            let words: Vec<&str> = span_text.split_whitespace().collect();

            for (i, word) in words.iter().enumerate() {
                let word_width = word.width();
                let space_width = if i < words.len() - 1 { 1 } else { 0 };

                // If this word would exceed width, start new line
                if current_width + word_width > width && current_width > 0 {
                    wrapped_lines.push(Line::from(current_line_spans.clone()));
                    current_line_spans.clear();
                    current_width = 0;
                }

                // If word itself is too long (longer than width), split it character by character
                if word_width > width {
                    let mut remaining = word.to_string();
                    while !remaining.is_empty() {
                        let mut chunk = String::new();
                        let mut chunk_width = 0;

                        for ch in remaining.chars() {
                            let ch_width = ch.to_string().width();
                            if chunk_width + ch_width > width && !chunk.is_empty() {
                                break;
                            }
                            chunk.push(ch);
                            chunk_width += ch_width;
                        }

                        if !chunk.is_empty() {
                            if current_width > 0 {
                                wrapped_lines.push(Line::from(current_line_spans.clone()));
                                current_line_spans.clear();
                            }
                            current_line_spans.push(Span::styled(chunk.clone(), span.style));
                            wrapped_lines.push(Line::from(current_line_spans.clone()));
                            current_line_spans.clear();
                            current_width = 0;

                            remaining = remaining[chunk.len()..].to_string();
                        } else {
                            break;
                        }
                    }
                } else {
                    // Add the word normally
                    let word_str = if space_width > 0 {
                        format!("{word} ")
                    } else {
                        word.to_string()
                    };

                    current_line_spans.push(Span::styled(word_str, span.style));
                    current_width += word_width + space_width;
                }
            }
        }

        // Add remaining spans as a line
        if !current_line_spans.is_empty() {
            wrapped_lines.push(Line::from(current_line_spans));
        } else if wrapped_lines.is_empty() {
            // Empty line - preserve it
            wrapped_lines.push(Line::from(""));
        }
    }

    // If no lines were created, return at least one empty line
    if wrapped_lines.is_empty() {
        wrapped_lines.push(Line::from(""));
    }

    wrapped_lines
}
