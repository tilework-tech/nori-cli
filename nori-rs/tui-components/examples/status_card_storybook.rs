mod support;

use anyhow::Result;
use crossterm::event::Event;
use crossterm::event::KeyCode;
use crossterm::event::KeyEventKind;
use nori_tui_components::DetailDensity;
use nori_tui_components::DetailEntry;
use nori_tui_components::DetailLabelStyle;
use nori_tui_components::DetailLayout;
use nori_tui_components::DetailPane;
use nori_tui_components::KeyHint;
use nori_tui_components::KeyHints;
use nori_tui_components::Theme;
use ratatui::layout::Constraint;
use ratatui::layout::Layout;
use ratatui::layout::Margin;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::style::Modifier;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Block;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;

use support::StorybookTerminal;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum SurfaceMode {
    Auto,
    Derived,
    Ansi,
    #[default]
    None,
}

impl SurfaceMode {
    fn next(self) -> Self {
        match self {
            Self::Auto => Self::Derived,
            Self::Derived => Self::Ansi,
            Self::Ansi => Self::None,
            Self::None => Self::Auto,
        }
    }

    fn label(self, derived_available: bool) -> &'static str {
        match self {
            Self::Auto if derived_available => "Auto (derived)",
            Self::Auto => "Auto (ANSI fallback)",
            Self::Derived if derived_available => "Derived",
            Self::Derived => "Derived (unavailable)",
            Self::Ansi => "ANSI black",
            Self::None => "None",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum AccentMode {
    #[default]
    Marker,
    None,
    Title,
    Labels,
}

impl AccentMode {
    fn next(self) -> Self {
        match self {
            Self::Marker => Self::None,
            Self::None => Self::Title,
            Self::Title => Self::Labels,
            Self::Labels => Self::Marker,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Marker => "Prompt marker",
            Self::None => "None",
            Self::Title => "Title",
            Self::Labels => "Labels",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum ContentMode {
    #[default]
    Summary,
    Full,
}

impl ContentMode {
    fn next(self) -> Self {
        match self {
            Self::Summary => Self::Full,
            Self::Full => Self::Summary,
        }
    }
}

fn main() -> Result<()> {
    let mut terminal = StorybookTerminal::enter()?;
    let terminal_theme = terminal.theme;
    let mut surface_mode = SurfaceMode::default();
    let mut accent_mode = AccentMode::default();
    let mut content_mode = ContentMode::default();
    let mut label_style = DetailLabelStyle::Plain;
    let mut density = DetailDensity::Compact;

    loop {
        terminal.terminal.draw(|frame| {
            let area = frame.area();
            Block::default()
                .style(terminal_theme.surface)
                .render(area, frame.buffer_mut());
            let inner = area.inner(Margin {
                horizontal: 1,
                vertical: 1,
            });
            let sections = Layout::vertical([
                Constraint::Length(3),
                Constraint::Min(1),
                Constraint::Length(2),
            ])
            .split(inner);
            let derived_available = terminal_theme.detail_surface.bg.is_some();
            Paragraph::new(vec![
                Line::styled("Status card specimen", terminal_theme.title),
                Line::styled(
                    format!(
                        "Background: {} · Labels: {} · Green: {}",
                        surface_mode.label(derived_available),
                        match label_style {
                            DetailLabelStyle::Plain => "plain",
                            DetailLabelStyle::Colon => "colons",
                        },
                        accent_mode.label(),
                    ),
                    terminal_theme.muted,
                ),
                Line::styled(
                    format!("Content: {content_mode:?} · Density: {density:?}"),
                    terminal_theme.muted,
                ),
            ])
            .render(sections[0], frame.buffer_mut());

            let mut card_theme = status_theme(terminal_theme, surface_mode);
            if accent_mode == AccentMode::Labels {
                card_theme.muted = card_theme.pointer;
            }
            card_theme.provider_claude = Style::new().fg(Color::Rgb(255, 158, 100));
            let entries = status_entries(content_mode, card_theme);
            let pane = DetailPane::new(&entries)
                .heading(status_heading(card_theme, accent_mode))
                .theme(card_theme)
                .label_style(label_style)
                .density(density)
                .layout(DetailLayout::Responsive { stack_below: 42 });
            let height = pane.required_height(sections[1].width);
            pane.render(
                Rect::new(
                    sections[1].x,
                    sections[1].y,
                    sections[1].width.min(100),
                    height.min(sections[1].height),
                ),
                frame.buffer_mut(),
            );

            let hints =
                Layout::vertical([Constraint::Length(1), Constraint::Length(1)]).split(sections[2]);
            KeyHints::new([
                KeyHint::new("b", "background"),
                KeyHint::new("c", "colons"),
                KeyHint::new("g", "green accent"),
            ])
            .theme(terminal_theme)
            .render(hints[0], frame.buffer_mut());
            KeyHints::new([
                KeyHint::new("d", "density"),
                KeyHint::new("v", "summary/full"),
                KeyHint::new("q / esc", "close"),
            ])
            .theme(terminal_theme)
            .render(hints[1], frame.buffer_mut());
        })?;

        let Some(Event::Key(key)) = terminal.next_event()? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        match key.code {
            KeyCode::Char('b') => surface_mode = surface_mode.next(),
            KeyCode::Char('c') => {
                label_style = match label_style {
                    DetailLabelStyle::Plain => DetailLabelStyle::Colon,
                    DetailLabelStyle::Colon => DetailLabelStyle::Plain,
                };
            }
            KeyCode::Char('g') => accent_mode = accent_mode.next(),
            KeyCode::Char('d') => {
                density = match density {
                    DetailDensity::Compact => DetailDensity::Normal,
                    DetailDensity::Normal => DetailDensity::Compact,
                };
            }
            KeyCode::Char('v') => content_mode = content_mode.next(),
            KeyCode::Esc | KeyCode::Char('q') => break,
            _ => {}
        }
    }
    Ok(())
}

fn status_theme(terminal_theme: Theme, surface_mode: SurfaceMode) -> Theme {
    let derived_available = terminal_theme.detail_surface.bg.is_some();
    match surface_mode {
        SurfaceMode::Auto if derived_available => terminal_theme,
        SurfaceMode::Derived => terminal_theme,
        SurfaceMode::Auto | SurfaceMode::Ansi => Theme {
            text: Style::new().fg(Color::White),
            muted: Style::new().fg(Color::DarkGray),
            title: Style::new().fg(Color::White).add_modifier(Modifier::BOLD),
            detail_surface: Style::new().bg(Color::Black),
            ..terminal_theme
        },
        SurfaceMode::None => Theme {
            detail_surface: Style::new(),
            row: Style::new(),
            row_alt: Style::new(),
            ..terminal_theme
        },
    }
}

fn status_heading(theme: Theme, accent_mode: AccentMode) -> Line<'static> {
    let version = Span::styled(" v0.1.0", theme.muted);
    match accent_mode {
        AccentMode::Marker => Line::from(vec![
            Span::styled("› ", theme.pointer),
            Span::styled("Nori CLI", theme.title),
            version,
        ]),
        AccentMode::None | AccentMode::Labels => {
            Line::from(vec![Span::styled("Nori CLI", theme.title), version])
        }
        AccentMode::Title => Line::from(vec![
            Span::styled("Nori CLI", theme.pointer.add_modifier(Modifier::BOLD)),
            version,
        ]),
    }
}

fn status_entries(content_mode: ContentMode, theme: Theme) -> Vec<DetailEntry> {
    match content_mode {
        ContentMode::Summary => vec![
            DetailEntry::key_value("System", "~/org/workspace/cli · Agent approvals · clifford"),
            DetailEntry::key_value(
                "Agent",
                Line::from(vec![
                    Span::styled("Claude", theme.provider_claude),
                    Span::raw(" · Opus 5 · xhigh · fast"),
                ]),
            ),
        ],
        ContentMode::Full => vec![
            DetailEntry::key_value("Directory", "~/org/workspace/cli"),
            DetailEntry::key_value("Session", "Fix terminal hierarchy"),
            DetailEntry::key_value("Approvals", "Agent"),
            DetailEntry::key_value("Skillset", "clifford"),
            DetailEntry::Rule,
            DetailEntry::key_value(
                "Agent",
                Line::from(Span::styled("Claude", theme.provider_claude)),
            ),
            DetailEntry::key_value("Model", "Opus 5"),
            DetailEntry::key_value("Reasoning", "xhigh"),
            DetailEntry::key_value("Speed", "fast"),
            DetailEntry::Rule,
            DetailEntry::key_value("Git", "⎇ main · +120 -8"),
            DetailEntry::key_value("Context", "44% used · 120K / 272K"),
            DetailEntry::key_value("Instructions", "~/org/AGENTS.md          ~1,830 tokens"),
            DetailEntry::key_value("", "./AGENTS.md               ~620 tokens"),
            DetailEntry::key_value("", "2 files · ~2,450 tokens"),
        ],
    }
}
