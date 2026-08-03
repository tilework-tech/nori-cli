mod support;

use anyhow::Result;
use codex_tui_components::KeyHint;
use codex_tui_components::KeyHints;
use codex_tui_components::Markdown;
use crossterm::event::Event;
use crossterm::event::KeyCode;
use crossterm::event::KeyEventKind;
use ratatui::layout::Constraint;
use ratatui::layout::Layout;
use ratatui::widgets::Block;
use ratatui::widgets::Borders;
use ratatui::widgets::Paragraph;

use support::StorybookTerminal;

const DOCUMENTS: [&str; 3] = [
    r#"# Markdown storybook

The shared renderer supports **strong text**, *emphasis*, ~~strikethrough~~,
`inline code`, and [links](https://nori.dev).

> The component owns rendering, while this example owns the event loop.

1. Resize the terminal
2. Press `2` for adaptive tables
3. Press `3` for streaming output

```rust
let text = Markdown::new(source).width(area.width).render_text();
```
"#,
    r#"# Adaptive tables

| Session | Project | Updated | Current turn status |
| :-- | :-- | --: | :-- |
| Fix parser recovery | nori-cli | 2m | Working |
| Improve Markdown tables | external-codex | 18m | Waiting for user input |
| Handroll picker migration | sessions/handroll | 1h | Implementing the event loop |

Narrow the terminal until the grid becomes stacked records.
"#,
    r#"# Streaming Markdown

Consumers can append chunks to `StreamingMarkdown` and render the complete
buffer on each frame. Incomplete syntax remains safe; later chunks refine it.

- Empty, partial, and complete buffers use one API.
- Width is supplied at render time.
- The consumer decides when to draw.

| Chunk | Result |
| :-- | :-- |
| `**work` | Safe partial text |
| ` complete**` | Strong completed text |
"#,
];

fn main() -> Result<()> {
    let mut terminal = StorybookTerminal::enter()?;
    let mut document = 0;
    let mut scroll = 0;
    loop {
        terminal.terminal.draw(|frame| {
            let chunks =
                Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).split(frame.area());
            let block = Block::default()
                .borders(Borders::ALL)
                .title(format!(" Markdown storybook · page {} of 3 ", document + 1));
            let inner = block.inner(chunks[0]);
            frame.render_widget(block, chunks[0]);
            let text = Markdown::new(DOCUMENTS[document])
                .width(inner.width)
                .render_text();
            frame.render_widget(Paragraph::new(text).scroll((scroll, 0)), inner);
            frame.render_widget(
                KeyHints::new([
                    KeyHint::new("1-3", "page"),
                    KeyHint::new("↑↓", "scroll"),
                    KeyHint::new("q", "close"),
                ]),
                chunks[1],
            );
        })?;
        let Some(Event::Key(key)) = terminal.next_event()? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => break,
            KeyCode::Char('1') => {
                document = 0;
                scroll = 0;
            }
            KeyCode::Char('2') => {
                document = 1;
                scroll = 0;
            }
            KeyCode::Char('3') => {
                document = 2;
                scroll = 0;
            }
            KeyCode::Up => scroll = scroll.saturating_sub(1),
            KeyCode::Down => scroll = scroll.saturating_add(1),
            _ => {}
        }
    }
    Ok(())
}
