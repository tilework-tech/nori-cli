use super::*;
use insta::assert_snapshot;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::style::Color;
use ratatui::style::Style;

fn entries() -> Vec<DetailEntry> {
    vec![
        DetailEntry::key_value("Agent", "Codex").tone(DetailTone::Provider(ProviderKind::Codex)),
        DetailEntry::key_value("Created", "2026-08-15 12:00"),
        DetailEntry::Rule,
        DetailEntry::muted(
            "Latest prompt",
            "Investigate the reusable component boundary without changing Handroll.",
        ),
        DetailEntry::muted(
            "Latest response",
            "The component stays stateless and consumer-owned.",
        ),
    ]
}

fn snapshot(width: u16, height: u16) -> String {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal
        .draw(|frame| {
            frame.render_widget(
                DetailPane::new(&entries()).heading("Session details"),
                frame.area(),
            )
        })
        .expect("draw pane");
    terminal.backend().to_string()
}

#[test]
fn detail_pane_wide_snapshot() {
    assert_snapshot!(snapshot(42, 12));
}

#[test]
fn detail_pane_narrow_snapshot() {
    assert_snapshot!(snapshot(22, 12));
}

#[test]
fn detail_pane_respects_fixed_gutter_and_provider_tone() {
    let backend = TestBackend::new(40, 4);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    let theme = Theme::default();
    terminal
        .draw(|frame| {
            frame.render_widget(
                DetailPane::new(&entries())
                    .theme(theme)
                    .label_width(LabelWidth::Fixed(10)),
                frame.area(),
            )
        })
        .expect("draw pane");
    let buffer = terminal.backend().buffer();
    assert_eq!(buffer[(11, 0)].symbol(), "│");
    assert_eq!(buffer[(13, 0)].fg, Color::Cyan);
}

#[test]
fn detail_pane_background_options_only_shade_their_intended_layer() {
    let cases = [
        (
            DetailBackground::Transparent,
            Color::Reset,
            Color::Reset,
            Color::Reset,
        ),
        (
            DetailBackground::Pane,
            Color::Blue,
            Color::Blue,
            Color::Blue,
        ),
        (
            DetailBackground::Heading,
            Color::Blue,
            Color::Reset,
            Color::Reset,
        ),
        (
            DetailBackground::LabelGutter,
            Color::Reset,
            Color::Blue,
            Color::Reset,
        ),
        (
            DetailBackground::Rows,
            Color::Reset,
            Color::Blue,
            Color::Blue,
        ),
    ];
    let entries = [DetailEntry::key_value("Agent", "Codex")];
    for (background, heading_bg, label_bg, value_bg) in cases {
        let backend = TestBackend::new(30, 5);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        let theme = Theme {
            detail_surface: Style::new().bg(Color::Blue),
            ..Theme::default()
        };
        terminal
            .draw(|frame| {
                frame.render_widget(
                    DetailPane::new(&entries)
                        .heading("Details")
                        .theme(theme)
                        .background(background),
                    frame.area(),
                )
            })
            .expect("draw pane");
        let buffer = terminal.backend().buffer();
        assert_eq!(buffer[(0, 0)].bg, heading_bg, "{background:?} heading");
        assert_eq!(buffer[(0, 2)].bg, label_bg, "{background:?} label");
        assert_eq!(buffer[(10, 2)].bg, value_bg, "{background:?} value");
        assert_eq!(
            buffer[(29, 4)].bg,
            if background == DetailBackground::Pane {
                Color::Blue
            } else {
                Color::Reset
            },
            "{background:?} unused area"
        );
    }
}
