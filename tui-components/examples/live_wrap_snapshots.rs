use std::time::Duration;

use color_eyre::Result;
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::{Color, Style},
    widgets::Paragraph,
};
use tui_components::live_wrap::{Row, RowBuilder};

use insta::assert_snapshot;
use tui_components::live_wrap::take_prefix_by_width;

// Interactive example application
struct App {
    examples: Vec<LiveWrapExample>,
}

struct LiveWrapExample {
    label: String,
    rows: Vec<Row>,
}

impl LiveWrapExample {
    fn new(label: impl Into<String>, rows: Vec<Row>) -> Self {
        Self {
            label: label.into(),
            rows,
        }
    }
}

impl App {
    fn new() -> Self {
        let mut examples = Vec::new();

        // ASCII text wrapping
        let mut rb = RowBuilder::new(10);
        rb.push_fragment("hello whirl this is a test");
        examples.push(LiveWrapExample::new("ASCII Wrapping", rb.rows().to_vec()));

        // Emoji and CJK wrapping
        let mut rb = RowBuilder::new(6);
        rb.push_fragment("😀😀 你好");
        examples.push(LiveWrapExample::new("Unicode Wrapping", rb.rows().to_vec()));

        // Newline handling
        let mut rb = RowBuilder::new(10);
        rb.push_fragment("hello\nworld");
        examples.push(LiveWrapExample::new("Newline Breaks", rb.display_rows()));

        // Explicit line breaks
        let mut rb = RowBuilder::new(20);
        rb.push_fragment("first line");
        rb.end_line();
        rb.push_fragment("second line");
        examples.push(LiveWrapExample::new("Explicit Breaks", rb.display_rows()));

        // Multiple newlines
        let mut rb = RowBuilder::new(20);
        rb.push_fragment("line1\n\nline3");
        examples.push(LiveWrapExample::new("Multiple Newlines", rb.display_rows()));

        // Long word wrapping
        let mut rb = RowBuilder::new(5);
        rb.push_fragment("supercalifragilisticexpialidocious");
        examples.push(LiveWrapExample::new(
            "Long Word Wrapping",
            rb.display_rows(),
        ));

        // Width change demonstration
        let mut rb = RowBuilder::new(10);
        rb.push_fragment("abcdefghijKLMN");
        let original_rows = rb.rows().to_vec();
        rb.set_width(5);
        let resized_rows = rb.rows().to_vec();
        let mut combined_rows = original_rows;
        combined_rows.push(Row {
            text: "--- Width changed to 5 ---".to_string(),
            explicit_break: true,
        });
        combined_rows.extend(resized_rows);
        examples.push(LiveWrapExample::new("Dynamic Width Change", combined_rows));

        Self { examples }
    }

    fn run(&mut self, terminal: &mut ratatui::DefaultTerminal) -> Result<()> {
        loop {
            if event::poll(Duration::from_millis(100))? {
                match event::read()? {
                    Event::Key(key) => {
                        if key.code == KeyCode::Esc
                            || (key.code == KeyCode::Char('c')
                                && key.modifiers.contains(KeyModifiers::CONTROL))
                        {
                            break;
                        }
                    }
                    Event::Resize(_, _) => {
                        terminal.draw(|frame| self.draw(frame))?;
                    }
                    _ => {}
                }
            }
            terminal.draw(|frame| self.draw(frame))?;
        }
        Ok(())
    }

    fn draw(&self, frame: &mut Frame) {
        let num_examples = self.examples.len();
        let constraints = vec![Constraint::Length(4); num_examples];

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints(constraints)
            .split(frame.area());

        for (i, example) in self.examples.iter().enumerate() {
            let area = chunks[i];

            // Label
            let label_area = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(1), Constraint::Min(0)])
                .split(area)[0];

            let label = Paragraph::new(format!("[ {} ]", example.label))
                .style(Style::default().fg(Color::Yellow));
            frame.render_widget(label, label_area);

            // Content
            let content_area = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(1), Constraint::Min(0)])
                .split(area)[1];

            let content = render_rows(&example.rows);
            let paragraph = Paragraph::new(content).style(Style::default().fg(Color::White));
            frame.render_widget(paragraph, content_area);
        }
    }
}

fn render_rows(rows: &[Row]) -> String {
    rows.iter()
        .map(|r| {
            if r.explicit_break {
                format!("{}⏎", r.text)
            } else {
                r.text.clone()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
fn test_rows_do_not_exceed_width_ascii() {
    let mut rb = RowBuilder::new(10);
    rb.push_fragment("hello whirl this is a test");
    let rows = rb.rows().to_vec();
    assert_snapshot!(render_rows(&rows), @r###"
    hello whir
    l this is
    "###);
}

#[test]
fn test_rows_do_not_exceed_width_emoji_cjk() {
    // 😀 is width 2; 你/好 are width 2.
    let mut rb = RowBuilder::new(6);
    rb.push_fragment("😀😀 你好");
    let rows = rb.rows().to_vec();
    assert_snapshot!(render_rows(&rows), @"😀😀 ");
}

#[test]
fn test_fragmentation_invariance_long_token() {
    let s = "ABCDEFGHIJKLMNOPQRSTUVWXYZ"; // 26 chars
    let mut rb_all = RowBuilder::new(7);
    rb_all.push_fragment(s);
    let all_rows = rb_all.rows().to_vec();

    let mut rb_chunks = RowBuilder::new(7);
    for i in (0..s.len()).step_by(3) {
        let end = (i + 3).min(s.len());
        rb_chunks.push_fragment(&s[i..end]);
    }
    let chunk_rows = rb_chunks.rows().to_vec();

    assert_eq!(all_rows, chunk_rows);
    assert_snapshot!(render_rows(&all_rows), @r###"
    ABCDEFG
    HIJKLMN
    OPQRSTU
    "###);
}

#[test]
fn test_newline_splits_rows() {
    let mut rb = RowBuilder::new(10);
    rb.push_fragment("hello\nworld");
    let rows = rb.display_rows();
    assert_snapshot!(render_rows(&rows), @r###"
    hello⏎
    world
    "###);
}

#[test]
fn test_rewrap_on_width_change() {
    let mut rb = RowBuilder::new(10);
    rb.push_fragment("abcdefghijKLMN");
    let rows_before = rb.rows().to_vec();
    assert_snapshot!(render_rows(&rows_before), @"abcdefghij");

    rb.set_width(5);
    let rows_after = rb.rows().to_vec();
    assert_snapshot!(render_rows(&rows_after), @r###"
    abcde
    fghij
    "###);
}

#[test]
fn test_end_line_marks_explicit_break() {
    let mut rb = RowBuilder::new(20);
    rb.push_fragment("first line");
    rb.end_line();
    rb.push_fragment("second line");
    let rows = rb.display_rows();
    assert_snapshot!(render_rows(&rows), @r###"
    first line⏎
    second line
    "###);
}

#[test]
fn test_drain_rows_clears_buffer() {
    let mut rb = RowBuilder::new(10);
    rb.push_fragment("line one\n");
    rb.push_fragment("line two\n");

    let drained = rb.drain_rows();
    assert_eq!(drained.len(), 2);

    let remaining = rb.rows();
    assert_eq!(remaining.len(), 0);
}

#[test]
fn test_display_rows_includes_partial() {
    let mut rb = RowBuilder::new(20);
    rb.push_fragment("complete\n");
    rb.push_fragment("partial");

    let display = rb.display_rows();
    assert_eq!(display.len(), 2);
    assert_snapshot!(render_rows(&display), @r###"
    complete⏎
    partial
    "###);
}

#[test]
fn test_drain_commit_ready_keeps_recent() {
    let mut rb = RowBuilder::new(10);
    for i in 0..10 {
        rb.push_fragment(&format!("line {}\n", i));
    }

    let old_rows = rb.drain_commit_ready(5);
    assert_eq!(old_rows.len(), 5);

    let remaining = rb.rows();
    assert_eq!(remaining.len(), 5);
    assert_snapshot!(render_rows(remaining), @r###"
    line 5⏎
    line 6⏎
    line 7⏎
    line 8⏎
    line 9⏎
    "###);
}

#[test]
fn test_take_prefix_by_width_ascii() {
    let (prefix, suffix, width) = take_prefix_by_width("hello world", 5);
    assert_eq!(prefix, "hello");
    assert_eq!(suffix, " world");
    assert_eq!(width, 5);
}

#[test]
fn test_take_prefix_by_width_unicode() {
    let (prefix, suffix, width) = take_prefix_by_width("😀😀😀", 4);
    assert_eq!(prefix, "😀😀");
    assert_eq!(suffix, "😀");
    assert_eq!(width, 4);
}

#[test]
fn test_take_prefix_by_width_exact_fit() {
    let (prefix, suffix, width) = take_prefix_by_width("abcde", 5);
    assert_eq!(prefix, "abcde");
    assert_eq!(suffix, "");
    assert_eq!(width, 5);
}

#[test]
fn test_take_prefix_by_width_empty() {
    let (prefix, suffix, width) = take_prefix_by_width("", 10);
    assert_eq!(prefix, "");
    assert_eq!(suffix, "");
    assert_eq!(width, 0);
}

#[test]
fn test_take_prefix_by_width_zero_max() {
    let (prefix, suffix, width) = take_prefix_by_width("hello", 0);
    assert_eq!(prefix, "");
    assert_eq!(suffix, "hello");
    assert_eq!(width, 0);
}

#[test]
fn test_multiple_newlines() {
    let mut rb = RowBuilder::new(20);
    rb.push_fragment("line1\n\nline3");
    let rows = rb.display_rows();
    assert_snapshot!(render_rows(&rows), @r###"
    line1⏎
    ⏎
    line3
    "###);
}

#[test]
fn test_long_word_wrapping() {
    let mut rb = RowBuilder::new(5);
    rb.push_fragment("supercalifragilisticexpialidocious");
    let rows = rb.display_rows();
    assert_snapshot!(render_rows(&rows), @r###"
    super
    calif
    ragil
    istic
    expia
    lidoc
    ious
    "###);
}

fn main() -> Result<()> {
    color_eyre::install()?;
    let mut terminal = ratatui::init();
    let mut app = App::new();
    let result = app.run(&mut terminal);
    ratatui::restore();
    result
}
