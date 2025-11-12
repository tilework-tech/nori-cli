use codex_tui_components::render::{
    ColumnRenderable, Insets, InsetRenderable, RectExt, Renderable, RenderableExt, RowRenderable,
};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};

fn render_to_string(renderable: &dyn Renderable, width: u16, height: u16) -> String {
    let mut buf = Buffer::empty(Rect::new(0, 0, width, height));
    renderable.render(Rect::new(0, 0, width, height), &mut buf);
    let mut lines = Vec::new();
    for y in 0..height {
        let mut line = String::new();
        for x in 0..width {
            line.push_str(&buf.cell((x, y)).unwrap().symbol());
        }
        // Trim trailing spaces from each line
        lines.push(line.trim_end().to_string());
    }
    lines.join("\n")
}

#[test]
fn test_string_renderable() {
    let renderable = "Hello, world!";
    let output = render_to_string(&renderable, 20, 1);
    insta::assert_snapshot!(output, @"Hello, world!");
}

#[test]
fn test_column_renderable() {
    let mut column = ColumnRenderable::new();
    column.push("Line 1");
    column.push("Line 2");
    column.push("Line 3");

    assert_eq!(column.desired_height(20), 3);

    let output = render_to_string(&column, 20, 3);
    insta::assert_snapshot!(output, @r"
    Line 1
    Line 2
    Line 3
    ");
}

#[test]
fn test_row_renderable() {
    let mut row = RowRenderable::new();
    row.push(5, "Left");
    row.push(10, "Middle");
    row.push(5, "Right");

    let output = render_to_string(&row, 20, 1);
    insta::assert_snapshot!(output, @"Left Middle    Right");
}

#[test]
fn test_inset_renderable() {
    let content = InsetRenderable::new("Padded", Insets::vh(1, 2));

    assert_eq!(content.desired_height(20), 3); // 1 top + 1 line + 1 bottom

    let output = render_to_string(&content, 10, 3);
    insta::assert_snapshot!(output, @r"

      Padded

    ");
}

#[test]
fn test_rect_inset() {
    let rect = Rect::new(0, 0, 20, 10);
    let inset_rect = rect.inset(Insets::vh(1, 2));

    assert_eq!(inset_rect.x, 2);
    assert_eq!(inset_rect.y, 1);
    assert_eq!(inset_rect.width, 16); // 20 - 2*2
    assert_eq!(inset_rect.height, 8); // 10 - 2*1
}

#[test]
fn test_rect_inset_saturating() {
    let rect = Rect::new(0, 0, 5, 3);
    let inset_rect = rect.inset(Insets::tlbr(10, 10, 10, 10));

    // Should saturate to 0, not underflow
    assert_eq!(inset_rect.width, 0);
    assert_eq!(inset_rect.height, 0);
}

#[test]
fn test_renderable_ext_inset() {
    let padded = "Content".inset(Insets::vh(0, 1));
    let output = render_to_string(&*padded, 10, 1);
    insta::assert_snapshot!(output, @" Content");
}

#[test]
fn test_nested_columns() {
    let mut inner = ColumnRenderable::new();
    inner.push("Inner 1");
    inner.push("Inner 2");

    let mut outer = ColumnRenderable::new();
    outer.push("Outer top");
    outer.push(inner);
    outer.push("Outer bottom");

    assert_eq!(outer.desired_height(20), 4); // 1 + 2 + 1

    let output = render_to_string(&outer, 20, 4);
    insta::assert_snapshot!(output, @r"
    Outer top
    Inner 1
    Inner 2
    Outer bottom
    ");
}

#[test]
fn test_option_renderable_some() {
    let renderable: Option<&str> = Some("Present");
    let output = render_to_string(&renderable, 10, 1);
    insta::assert_snapshot!(output, @"Present");
}

#[test]
fn test_option_renderable_none() {
    let renderable: Option<&str> = None;
    assert_eq!(renderable.desired_height(10), 0);
    let output = render_to_string(&renderable, 10, 1);
    insta::assert_snapshot!(output, @"");
}

#[test]
fn test_line_renderable() {
    let line = Line::from(vec![
        Span::raw("Hello "),
        Span::raw("world"),
    ]);
    let output = render_to_string(&line, 15, 1);
    insta::assert_snapshot!(output, @"Hello world");
}

#[test]
fn test_complex_layout() {
    // Create a layout with header, content columns, and footer
    let mut main_column = ColumnRenderable::new();

    // Header
    main_column.push("=== Header ===".inset(Insets::vh(0, 1)));

    // Content row with two columns
    let mut content_row = RowRenderable::new();

    let mut left_col = ColumnRenderable::new();
    left_col.push("Left 1");
    left_col.push("Left 2");

    let mut right_col = ColumnRenderable::new();
    right_col.push("Right 1");
    right_col.push("Right 2");

    content_row.push(10, left_col);
    content_row.push(10, right_col);

    main_column.push(content_row);

    // Footer
    main_column.push("=== Footer ===".inset(Insets::vh(0, 1)));

    let output = render_to_string(&main_column, 22, 4);
    insta::assert_snapshot!(output, @r"
     === Header ===
    Left 1    Right 1
    Left 2    Right 2
     === Footer ===
    ");
}

#[cfg(feature = "syntax-highlighting")]
mod highlight_tests {
    use codex_tui_components::render::highlight::highlight_bash_to_lines;

    #[test]
    fn test_highlight_simple_command() {
        let script = "echo hello";
        let lines = highlight_bash_to_lines(script);

        assert_eq!(lines.len(), 1);
        assert!(!lines[0].spans.is_empty());

        // Check that the text is preserved
        let reconstructed: String = lines[0]
            .spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect();
        assert_eq!(reconstructed, "echo hello");
    }

    #[test]
    fn test_highlight_multiline() {
        let script = "echo line1\necho line2";
        let lines = highlight_bash_to_lines(script);

        assert_eq!(lines.len(), 2);

        let line1: String = lines[0]
            .spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect();
        let line2: String = lines[1]
            .spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect();

        assert_eq!(line1, "echo line1");
        assert_eq!(line2, "echo line2");
    }
}
