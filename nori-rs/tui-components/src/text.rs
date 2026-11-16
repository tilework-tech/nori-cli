//! Width-aware text helpers shared by components and their callers.

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// Fit `text` in `cells` display cells by cutting its middle, keeping the
/// start and the end so names that share a long prefix stay apart. The start
/// gets the larger half; graphemes are never split.
pub fn truncate_middle(text: &str, cells: usize) -> String {
    if text.width() <= cells {
        return text.to_string();
    }
    if cells == 0 {
        return String::new();
    }
    let room = cells - 1;
    let graphemes: Vec<&str> = text.graphemes(true).collect();
    let mut head = String::new();
    for grapheme in &graphemes {
        if head.width() + grapheme.width() > room.div_ceil(2) {
            break;
        }
        head.push_str(grapheme);
    }
    let mut tail_width = 0;
    let mut tail: Vec<&str> = Vec::new();
    for grapheme in graphemes.iter().rev() {
        if head.width() + tail_width + grapheme.width() > room {
            break;
        }
        tail_width += grapheme.width();
        tail.push(grapheme);
    }
    tail.reverse();
    format!("{}…{}", head.trim_end(), tail.concat().trim_start())
}

#[cfg(test)]
mod tests {
    use super::truncate_middle;
    use pretty_assertions::assert_eq;
    use unicode_width::UnicodeWidthStr;

    #[test]
    fn text_that_fits_is_unchanged() {
        assert_eq!(truncate_middle("parser", 6), "parser");
    }

    #[test]
    fn tiny_widths_still_fit() {
        assert_eq!(truncate_middle("parser", 0), "");
        assert_eq!(truncate_middle("parser", 1), "…");
        assert_eq!(truncate_middle("parser", 2), "p…");
    }

    #[test]
    fn the_start_and_end_survive_with_the_start_getting_more() {
        assert_eq!(truncate_middle("review the parser:x", 9), "revi…er:x");
    }

    #[test]
    fn wide_and_joined_characters_are_never_split() {
        for (text, cells) in [
            ("日本語のパーサー", 7),
            ("ab👩‍💻cd👩‍💻ef", 6),
            ("cafe\u{301} au lait", 6),
        ] {
            let cut = truncate_middle(text, cells);
            assert!(cut.width() <= cells, "{cut:?} fits {cells}");
            assert!(!cut.contains('\u{200d}') || cut.contains("👩‍💻"), "{cut:?}");
            assert!(cut.contains('…'), "{cut:?}");
        }
    }
}
