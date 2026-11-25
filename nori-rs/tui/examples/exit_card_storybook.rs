//! Exit card design specimen.
//!
//! Renders the design-only exit card (`nori_tui::storybook::exit_card_lines`)
//! over synthetic fixtures. The storybook owns the chrome: width, tool list,
//! fields, shading, and color.
//!
//! Flags set the starting state so captures are repeatable:
//! `--width N`, `--tools few|many|long|none`,
//! `--fields full|minimal|not-resumable|no-session`,
//! `--shade off|terminal|dark|light`, `--no-color`.

#[path = "../../tui-components/examples/support/mod.rs"]
mod support;

use anyhow::Result;
use anyhow::bail;
use crossterm::event::Event;
use crossterm::event::KeyCode;
use crossterm::event::KeyEventKind;
use nori_tui::storybook::ExitCardPaint;
use nori_tui::storybook::ExitFieldsFixture;
use nori_tui::storybook::ExitToolsFixture;
use nori_tui::storybook::exit_card_fixture;
use nori_tui::storybook::exit_card_lines;
use nori_tui_components::KeyHint;
use nori_tui_components::KeyHints;
use ratatui::layout::Constraint;
use ratatui::layout::Layout;
use ratatui::layout::Margin;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Block;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;

use support::StorybookTerminal;

const WIDTHS: [u16; 11] = [120, 100, 80, 64, 48, 40, 32, 27, 24, 16, 10];
const DARK: (u8, u8, u8) = (24, 26, 27);
const LIGHT: (u8, u8, u8) = (248, 247, 244);
const LIGHT_TEXT: (u8, u8, u8) = (30, 32, 33);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Shade {
    Off,
    Terminal,
    Dark,
    Light,
}

impl Shade {
    fn next(self) -> Self {
        match self {
            Self::Off => Self::Terminal,
            Self::Terminal => Self::Dark,
            Self::Dark => Self::Light,
            Self::Light => Self::Off,
        }
    }

    fn label(self, terminal_bg: Option<(u8, u8, u8)>) -> String {
        match (self, terminal_bg) {
            (Self::Off, _) => "off".into(),
            (Self::Terminal, Some((r, g, b))) => format!("from terminal #{r:02x}{g:02x}{b:02x}"),
            (Self::Terminal, None) => "from terminal (unknown, so none)".into(),
            (Self::Dark, _) => "dark terminal preview".into(),
            (Self::Light, _) => "light terminal preview".into(),
        }
    }
}

struct State {
    width: usize,
    tools: ExitToolsFixture,
    fields: ExitFieldsFixture,
    shade: Shade,
    color: bool,
}

fn parse_args() -> Result<State> {
    let mut state = State {
        width: 2,
        tools: ExitToolsFixture::Few,
        fields: ExitFieldsFixture::Full,
        shade: Shade::Off,
        color: true,
    };
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = || args.next().unwrap_or_default();
        match arg.as_str() {
            "--width" => {
                let width: u16 = value().parse()?;
                let Some(index) = WIDTHS.iter().position(|w| *w == width) else {
                    bail!("--width must be one of {WIDTHS:?}");
                };
                state.width = index;
            }
            "--tools" => {
                state.tools = match value().as_str() {
                    "few" => ExitToolsFixture::Few,
                    "many" => ExitToolsFixture::Many,
                    "long" => ExitToolsFixture::LongNames,
                    "none" => ExitToolsFixture::None,
                    other => bail!("unknown --tools {other}"),
                }
            }
            "--fields" => {
                state.fields = match value().as_str() {
                    "full" => ExitFieldsFixture::Full,
                    "minimal" => ExitFieldsFixture::Minimal,
                    "not-resumable" => ExitFieldsFixture::NotResumable,
                    "no-session" => ExitFieldsFixture::NoSession,
                    other => bail!("unknown --fields {other}"),
                }
            }
            "--shade" => {
                state.shade = match value().as_str() {
                    "off" => Shade::Off,
                    "terminal" => Shade::Terminal,
                    "dark" => Shade::Dark,
                    "light" => Shade::Light,
                    other => bail!("unknown --shade {other}"),
                }
            }
            "--no-color" => state.color = false,
            other => bail!("unknown argument {other}"),
        }
    }
    Ok(state)
}

#[allow(clippy::disallowed_methods)]
fn rgb((r, g, b): (u8, u8, u8)) -> Color {
    Color::Rgb(r, g, b)
}

fn main() -> Result<()> {
    let mut state = parse_args()?;
    let mut terminal = StorybookTerminal::enter()?;
    let theme = terminal.theme;
    let terminal_bg = terminal_background();

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
                Constraint::Length(5),
                Constraint::Min(1),
                Constraint::Length(1),
            ])
            .split(inner);

            let width = WIDTHS[state.width].min(sections[1].width);
            let color = if state.color { "on" } else { "off (NO_COLOR)" };
            Paragraph::new(vec![
                Line::styled("Exit card specimen · design only", theme.title),
                Line::styled(
                    format!(
                        "Width {width} · Tools: {} · Fields: {}",
                        state.tools.label(),
                        state.fields.label()
                    ),
                    theme.muted,
                ),
                Line::styled(
                    format!(
                        "Shading: {} · Color: {color}",
                        state.shade.label(terminal_bg)
                    ),
                    theme.muted,
                ),
                Line::default(),
                ruler(width, theme.muted),
            ])
            .render(sections[0], frame.buffer_mut());

            let (shade_from, canvas) = match state.shade {
                Shade::Off => (None, None),
                Shade::Terminal => (terminal_bg, None),
                Shade::Dark => (Some(DARK), Some(Style::new().bg(rgb(DARK)))),
                Shade::Light => (
                    Some(LIGHT),
                    Some(Style::new().bg(rgb(LIGHT)).fg(rgb(LIGHT_TEXT))),
                ),
            };
            let paint = ExitCardPaint {
                shade_from: shade_from.filter(|_| state.color),
                color: state.color,
            };
            let card = Rect::new(sections[1].x, sections[1].y, width, sections[1].height);
            if let Some(canvas) = canvas {
                let wide = Rect::new(card.x, card.y, sections[1].width, card.height);
                Block::default()
                    .style(canvas)
                    .render(wide, frame.buffer_mut());
            }
            let model = exit_card_fixture(state.tools, state.fields);
            Paragraph::new(exit_card_lines(&model, width, paint)).render(card, frame.buffer_mut());

            KeyHints::new([
                KeyHint::new("← →", "width"),
                KeyHint::new("t", "tools"),
                KeyHint::new("f", "fields"),
                KeyHint::new("s", "shading"),
                KeyHint::new("c", "color"),
                KeyHint::new("q", "close"),
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
            KeyCode::Left => state.width = (state.width + 1).min(WIDTHS.len() - 1),
            KeyCode::Right => state.width = state.width.saturating_sub(1),
            KeyCode::Char('t') => state.tools = state.tools.next(),
            KeyCode::Char('f') => state.fields = state.fields.next(),
            KeyCode::Char('s') => state.shade = state.shade.next(),
            KeyCode::Char('c') => state.color = !state.color,
            KeyCode::Esc | KeyCode::Char('q') => break,
            _ => {}
        }
    }
    Ok(())
}

/// A dotted row exactly `width` cells wide, so the card's edge is visible.
fn ruler(width: u16, style: Style) -> Line<'static> {
    let width = usize::from(width);
    let label = format!(" {width}");
    let dots = width.saturating_sub(label.len());
    if dots == 0 {
        return Line::from(Span::styled("·".repeat(width), style));
    }
    Line::from(Span::styled(format!("{}{label}", "·".repeat(dots)), style))
}

#[cfg(unix)]
fn terminal_background() -> Option<(u8, u8, u8)> {
    use crossterm::style::query_background_color;

    match query_background_color().ok().flatten()? {
        crossterm::style::Color::Rgb { r, g, b } => Some((r, g, b)),
        _ => None,
    }
}

#[cfg(not(unix))]
fn terminal_background() -> Option<(u8, u8, u8)> {
    None
}
