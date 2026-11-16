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
