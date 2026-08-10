use super::*;
use insta::assert_snapshot;
use pretty_assertions::assert_eq;
use ratatui::Terminal;
use ratatui::backend::TestBackend;

fn snapshot(markdown: &str, width: u16, height: u16) -> String {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal
        .draw(|frame| {
            frame.render_widget(Markdown::new(markdown).width(width), frame.area());
        })
        .expect("draw markdown");
    terminal.backend().to_string()
}

#[test]
fn common_markdown_snapshot() {
    let source = r#"# Component library

This renderer supports **strong**, *emphasis*, `inline code`, and [links](https://nori.dev).

> Components render from state and return typed outcomes.

1. Build the picker
2. Add adaptive tables
   - preserve Unicode width
   - handle narrow terminals

```rust
let picker = Picker::new(&state);
```
"#;
    assert_snapshot!(snapshot(source, 66, 22));
}

#[test]
fn table_grid_snapshot() {
    let source = r#"## Active sessions

| Session | Project | Updated | Turn status |
| :-- | :-- | --: | :-- |
| Fix parser recovery | nori-cli | 2m | Working |
| Improve Markdown tables | external-codex | 18m | Waiting for input |
| Handroll picker migration | sessions/handroll | 1h | Ready |
"#;
    assert_snapshot!(snapshot(source, 86, 16));
}

#[test]
fn narrow_table_uses_stacked_records_snapshot() {
    let source = r#"| Session | Project | Turn status |
| :-- | :-- | :-- |
| Improve Markdown tables | external-codex | Waiting for user input |
| Handroll picker migration | sessions/handroll | Implementing the event loop |
"#;
    assert_snapshot!(snapshot(source, 38, 20));
}

#[test]
fn table_handles_unicode_and_long_tokens_snapshot() {
    let source = r#"| Name | Path | State |
| :-- | :-- | :--: |
| 日本語のセッション | /workspace/really-long-project-directory/src/parser.rs | ✓ |
| Résumé UI | session-019fc87740127ac08ef3a91ace638f94 | queued |
"#;
    assert_snapshot!(snapshot(source, 58, 18));
}

#[test]
fn prose_after_table_is_not_swallowed_snapshot() {
    let source = r#"| Component | State |
| :-- | :-- |
| Picker | Ready |
This sentence immediately follows the table without a blank line.
"#;
    assert_snapshot!(snapshot(source, 64, 10));
}

#[test]
fn streaming_buffer_matches_complete_render() {
    let mut streaming = StreamingMarkdown::new();
    streaming.push_str("## Streaming\n\n| State | Value |\n");
    streaming.push_str("| :-- | --: |\n| complete | 3 |\n");

    assert_eq!(
        streaming.markdown().width(42).render_text(),
        Markdown::new(streaming.source()).width(42).render_text(),
    );
    assert_snapshot!(snapshot(streaming.source(), 42, 10));
}
