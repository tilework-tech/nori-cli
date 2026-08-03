mod support;

use anyhow::Result;
use codex_tui_components::EmptyState;
use codex_tui_components::KeyHint;
use codex_tui_components::KeyHints;
use codex_tui_components::MessageLevel;
use codex_tui_components::SemanticMessage;
use codex_tui_components::Theme;
use crossterm::event::Event;
use crossterm::event::KeyCode;
use crossterm::event::KeyEventKind;
use ratatui::layout::Constraint;
use ratatui::layout::Layout;
use ratatui::layout::Margin;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Block;
use ratatui::widgets::Paragraph;

use support::StorybookTerminal;

fn main() -> Result<()> {
    let mut terminal = StorybookTerminal::enter()?;
    let theme = Theme::default();
    loop {
        terminal.terminal.draw(|frame| {
            let outer = Block::default()
                .style(theme.surface);
            frame.render_widget(outer, frame.area());
            let inner = frame.area().inner(Margin {
                horizontal: 2,
                vertical: 1,
            });
            let chunks = Layout::vertical([
                Constraint::Length(2),
                Constraint::Length(3),
                Constraint::Length(9),
                Constraint::Length(4),
                Constraint::Min(1),
                Constraint::Length(1),
            ])
            .split(inner);
            frame.render_widget(
                Paragraph::new(Line::styled("Component storybook", theme.title)),
                chunks[0],
            );
            frame.render_widget(
                Paragraph::new(vec![
                    Line::styled("Semantic tokens", theme.title),
                    Line::from(vec![
                        Span::styled("accent  ", theme.accent),
                        Span::styled("success  ", theme.success),
                        Span::styled("warning  ", theme.warning),
                        Span::styled("error  ", theme.error),
                        Span::styled("muted", theme.muted),
                    ]),
                ]),
                chunks[1],
            );
            let messages = [
                SemanticMessage::new(MessageLevel::Info, "Connected to the agent"),
                SemanticMessage::new(MessageLevel::Success, "Snapshot suite passed"),
                SemanticMessage::new(MessageLevel::Warning, "Two sessions are still running")
                    .detail("Open the picker to inspect them."),
                SemanticMessage::new(MessageLevel::Error, "Could not resume the session")
                    .detail("The agent no longer reports that session id."),
            ];
            for (index, message) in messages.into_iter().enumerate() {
                frame.render_widget(
                    message,
                    ratatui::layout::Rect::new(
                        chunks[2].x,
                        chunks[2].y + index as u16 * 2,
                        chunks[2].width,
                        2,
                    ),
                );
            }
            frame.render_widget(
                EmptyState::new("No matching sessions")
                    .detail("Try a title, project path, or session id."),
                chunks[3],
            );
            frame.render_widget(
                Paragraph::new("These primitives are intentionally small. Applications compose them around caller-owned state and translate their own events."),
                chunks[4],
            );
            frame.render_widget(
                KeyHints::new([KeyHint::new("q / esc", "close storybook")]),
                chunks[5],
            );
        })?;
        let Some(Event::Key(key)) = terminal.next_event()? else {
            continue;
        };
        if key.kind == KeyEventKind::Press && matches!(key.code, KeyCode::Esc | KeyCode::Char('q'))
        {
            break;
        }
    }
    Ok(())
}
