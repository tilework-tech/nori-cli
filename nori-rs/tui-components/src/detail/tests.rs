use super::*;
use insta::assert_snapshot;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::style::Color;

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
