//! Interactive specimen over production `/context` rendering and fixture data.

#[path = "../../tui-components/examples/support/mod.rs"]
mod support;

use anyhow::Result;
use crossterm::event::Event;
use crossterm::event::KeyCode;
use crossterm::event::KeyEventKind;
use nori_tui::storybook::context_specimen_lines;
use nori_tui_components::KeyHint;
use nori_tui_components::KeyHints;
use ratatui::layout::Constraint;
use ratatui::layout::Layout;
use ratatui::style::Color;
use ratatui::style::Modifier;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;
use support::StorybookTerminal;

fn main() -> Result<()> {
    if std::env::args().any(|arg| arg == "--json") {
        let mut specimens = Vec::new();
        for width in [100, 60, 32] {
            for used in [Some(340_412), Some(950_000), Some(0), None] {
                let lines = context_specimen_lines(used, width);
                let lines: Vec<_> = lines.iter().map(|line| line.spans.iter().map(|span| {
                    let style = line.style.patch(span.style);
                    serde_json::json!({
                        "text": span.content,
                        "tone": match style.fg { Some(Color::Green) => "green", Some(Color::DarkGray) => "muted", _ => "text" },
                        "bold": style.add_modifier.contains(Modifier::BOLD),
                    })
                }).collect::<Vec<_>>()).collect();
                specimens.push(serde_json::json!({"width": width, "used": used, "lines": lines}));
            }
        }
        println!("{}", serde_json::to_string(&specimens)?);
        return Ok(());
    }
    let mut terminal = StorybookTerminal::enter()?;
    let mut used = Some(340_412);
    loop {
        terminal.terminal.draw(|frame| {
            let sections =
                Layout::vertical([Constraint::Min(1), Constraint::Length(2)]).split(frame.area());
            Paragraph::new(context_specimen_lines(used, sections[0].width))
                .render(sections[0], frame.buffer_mut());
            KeyHints::new([
                KeyHint::new("u", "cycle fixture usage"),
                KeyHint::new("q / esc", "close"),
            ])
            .theme(terminal.theme)
            .render(sections[1], frame.buffer_mut());
        })?;
        if let Some(Event::Key(key)) = terminal.next_event()?
            && key.kind == KeyEventKind::Press
        {
            match key.code {
                KeyCode::Char('q') | KeyCode::Esc => break,
                KeyCode::Char('u') => {
                    used = match used {
                        Some(340_412) => Some(950_000),
                        Some(950_000) => Some(0),
                        Some(_) => None,
                        None => Some(340_412),
                    }
                }
                _ => {}
            }
        }
    }
    Ok(())
}
