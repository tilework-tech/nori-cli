//! Snapshot tests for selection components

use insta::assert_snapshot;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use tui_components::render::Renderable;
use tui_components::selection::selection_option_row;

fn render_to_string(renderable: &dyn Renderable, width: u16) -> String {
    let height = renderable.desired_height(width);
    let area = Rect::new(0, 0, width, height);
    let mut buf = Buffer::empty(area);
    renderable.render(area, &mut buf);

    let lines: Vec<String> = (0..area.height)
        .map(|row| {
            let mut line = String::new();
            for col in 0..area.width {
                let symbol = buf[(area.x + col, area.y + row)].symbol();
                if symbol.is_empty() {
                    line.push(' ');
                } else {
                    line.push_str(symbol);
                }
            }
            line
        })
        .collect();
    lines.join("\n")
}

#[test]
fn selection_row_unselected() {
    let row = selection_option_row(0, "First Option".to_string(), false);
    let output = render_to_string(row.as_ref(), 40);
    assert_snapshot!("selection_row_unselected", output);
}

#[test]
fn selection_row_selected() {
    let row = selection_option_row(0, "First Option".to_string(), true);
    let output = render_to_string(row.as_ref(), 40);
    assert_snapshot!("selection_row_selected", output);
}

#[test]
fn selection_row_double_digit_index() {
    let row = selection_option_row(9, "Tenth Option".to_string(), false);
    let output = render_to_string(row.as_ref(), 40);
    assert_snapshot!("selection_row_double_digit_index", output);
}

#[test]
fn selection_row_triple_digit_index() {
    let row = selection_option_row(99, "Hundredth Option".to_string(), true);
    let output = render_to_string(row.as_ref(), 40);
    assert_snapshot!("selection_row_triple_digit_index", output);
}

#[test]
fn selection_row_long_label_wraps() {
    let row = selection_option_row(
        0,
        "This is a very long option label that should wrap to multiple lines when rendered".to_string(),
        false,
    );
    let output = render_to_string(row.as_ref(), 40);
    assert_snapshot!("selection_row_long_label_wraps", output);
}

#[test]
fn selection_row_unicode_content() {
    let row = selection_option_row(0, "Option with emoji 🚀 and unicode ✓".to_string(), true);
    let output = render_to_string(row.as_ref(), 50);
    assert_snapshot!("selection_row_unicode_content", output);
}
