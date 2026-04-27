use super::*;
// crossterm types are intentionally not imported here to avoid unused warnings
use rand::prelude::*;

fn rand_grapheme(rng: &mut rand::rngs::StdRng) -> String {
    let r: u8 = rng.random_range(0..100);
    match r {
        0..=4 => "\n".to_string(),
        5..=12 => " ".to_string(),
        13..=35 => (rng.random_range(b'a'..=b'z') as char).to_string(),
        36..=45 => (rng.random_range(b'A'..=b'Z') as char).to_string(),
        46..=52 => (rng.random_range(b'0'..=b'9') as char).to_string(),
        53..=65 => {
            // Some emoji (wide graphemes)
            let choices = ["👍", "😊", "🐍", "🚀", "🧪", "🌟"];
            choices[rng.random_range(0..choices.len())].to_string()
        }
        66..=75 => {
            // CJK wide characters
            let choices = ["漢", "字", "測", "試", "你", "好", "界", "编", "码"];
            choices[rng.random_range(0..choices.len())].to_string()
        }
        76..=85 => {
            // Combining mark sequences
            let base = ["e", "a", "o", "n", "u"][rng.random_range(0..5)];
            let marks = ["\u{0301}", "\u{0308}", "\u{0302}", "\u{0303}"];
            format!("{base}{}", marks[rng.random_range(0..marks.len())])
        }
        86..=92 => {
            // Some non-latin single codepoints (Greek, Cyrillic, Hebrew)
            let choices = ["Ω", "β", "Ж", "ю", "ש", "م", "ह"];
            choices[rng.random_range(0..choices.len())].to_string()
        }
        _ => {
            // ZWJ sequences (single graphemes but multi-codepoint)
            let choices = [
                "👩\u{200D}💻", // woman technologist
                "👨\u{200D}💻", // man technologist
                "🏳️\u{200D}🌈", // rainbow flag
            ];
            choices[rng.random_range(0..choices.len())].to_string()
        }
    }
}

fn ta_with(text: &str) -> TextArea {
    let mut t = TextArea::new();
    t.insert_str(text);
    t
}

// ===== Helper to create a vim-enabled textarea in Normal mode =====
fn vim_normal(text: &str) -> TextArea {
    let mut t = ta_with(text);
    t.set_vim_mode_enabled(true);
    t.enter_vim_normal_mode();
    t
}

fn key(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
}

fn shift_key(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::SHIFT)
}

fn esc_key() -> KeyEvent {
    KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)
}

mod part1;
mod part2;
mod part3;
mod part4;
mod part5;
