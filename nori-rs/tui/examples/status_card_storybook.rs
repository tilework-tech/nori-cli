//! Status card specimen.
//!
//! The card is rendered by the production status view (`nori_tui::storybook`),
//! so what shows here is exactly what a session shows. The storybook owns only
//! the chrome around it: a caption and the toggle between the compact welcome
//! block and the full `/status` card.

#[path = "../../tui-components/examples/support/mod.rs"]
mod support;

use anyhow::Result;
use crossterm::event::Event;
use crossterm::event::KeyCode;
use crossterm::event::KeyEventKind;
use nori_tui::storybook::StatusSpecimen;
use nori_tui::storybook::status_specimen_lines;
use nori_tui_components::KeyHint;
use nori_tui_components::KeyHints;
use ratatui::layout::Constraint;
use ratatui::layout::Layout;
use ratatui::layout::Margin;
use ratatui::text::Line;
use ratatui::widgets::Block;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;

use support::StorybookTerminal;

fn main() -> Result<()> {
    let mut terminal = StorybookTerminal::enter()?;
    let theme = terminal.theme;
    let mut specimen = StatusSpecimen::default();

    loop {
        terminal.terminal.draw(|frame| {
            let area = frame.area();
            Block::default()
                .style(theme.surface)
                .render(area, frame.buffer_mut());
            let inner = area.inner(Margin {
                horizontal: 1,
                vertical: 1,
            });
            let sections = Layout::vertical([
                Constraint::Length(3),
                Constraint::Min(1),
                Constraint::Length(1),
            ])
            .split(inner);

            Paragraph::new(vec![
                Line::styled("Status card specimen", theme.title),
                Line::styled(format!("Content: {}", specimen.label()), theme.muted),
                Line::styled("Rendered by the production status view", theme.muted),
            ])
            .render(sections[0], frame.buffer_mut());

            let card = sections[1];
            let width = card.width.min(100);
            Paragraph::new(status_specimen_lines(specimen, width)).render(
                ratatui::layout::Rect::new(card.x, card.y, width, card.height),
                frame.buffer_mut(),
            );

            KeyHints::new([
                KeyHint::new("v", "compact/full"),
                KeyHint::new("q / esc", "close"),
            ])
            .theme(theme)
            .render(sections[2], frame.buffer_mut());
        })?;

        let Some(Event::Key(key)) = terminal.next_event()? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        match key.code {
            KeyCode::Char('v') => specimen = specimen.next(),
            KeyCode::Esc | KeyCode::Char('q') => break,
            _ => {}
        }
    }
    Ok(())
}
