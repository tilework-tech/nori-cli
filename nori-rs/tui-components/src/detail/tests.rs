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
    assert_eq!(buffer[(2, 0)].symbol(), "A");
    assert_eq!(buffer[(14, 0)].symbol(), "C");
    assert_eq!(buffer[(14, 0)].fg, Color::White);
}

#[test]
fn detail_pane_left_aligns_two_columns_without_rule_glyphs() {
    let entries = [
        DetailEntry::key_value("A", "first"),
        DetailEntry::key_value("Longer", "second"),
        DetailEntry::Rule,
        DetailEntry::key_value("After", "third"),
    ];
    let backend = TestBackend::new(30, 4);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal
        .draw(|frame| frame.render_widget(DetailPane::new(&entries), frame.area()))
        .expect("draw pane");
    let buffer = terminal.backend().buffer();

    assert_eq!(buffer[(2, 0)].symbol(), "A");
    assert_eq!(buffer[(2, 1)].symbol(), "L");
    assert_eq!(buffer[(10, 0)].symbol(), "f");
    assert_eq!(buffer[(10, 1)].symbol(), "s");
    assert_eq!(buffer[(8, 0)].symbol(), " ");
    assert_eq!(buffer[(9, 0)].symbol(), " ");
    assert_eq!(buffer[(2, 3)].symbol(), "A");
    assert_eq!(buffer[(10, 3)].symbol(), "t");
    assert!(
        (0..30).all(|x| buffer[(x, 2)].symbol() == " "),
        "DetailEntry::Rule should reserve one blank grouping row"
    );
}

#[test]
fn detail_pane_measures_wrapped_values_with_renderer_semantics() {
    let entries = [
        DetailEntry::key_value("Wrapped", "aaaaaa bbbbbb cccccc").wrap(true),
        DetailEntry::key_value("After", "visible"),
    ];
    let backend = TestBackend::new(23, 4);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal
        .draw(|frame| frame.render_widget(DetailPane::new(&entries), frame.area()))
        .expect("draw pane");
    let buffer = terminal.backend().buffer();

    assert_eq!(buffer[(11, 0)].symbol(), "a");
    assert_eq!(buffer[(11, 1)].symbol(), "b");
    assert_eq!(buffer[(11, 2)].symbol(), "c");
    assert_eq!(buffer[(2, 3)].symbol(), "A");
    assert_eq!(buffer[(11, 3)].symbol(), "v");
}

#[test]
fn detail_pane_shades_one_column_inset_with_or_without_heading() {
    let entries = [DetailEntry::key_value("Agent", "Codex")];
    let render = |heading: Option<&'static str>| {
        let backend = TestBackend::new(30, 5);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        let theme = Theme {
            detail_surface: Style::new().bg(Color::Blue),
            ..Theme::default()
        };
        terminal
            .draw(|frame| {
                let pane = DetailPane::new(&entries).theme(theme);
                frame.render_widget(
                    if let Some(heading) = heading {
                        pane.heading(heading)
                    } else {
                        pane
                    },
                    frame.area(),
                );
            })
            .expect("draw pane");
        terminal.backend().buffer().clone()
    };
    let with_heading = render(Some("Details"));
    for row in [0, 4] {
        assert_eq!(with_heading[(0, row)].bg, Color::Reset);
        assert_eq!(with_heading[(1, row)].bg, Color::Blue);
        assert_eq!(with_heading[(28, row)].bg, Color::Blue);
        assert_eq!(with_heading[(29, row)].bg, Color::Reset);
    }
    assert_eq!(with_heading[(1, 0)].symbol(), " ");
    assert_eq!(with_heading[(2, 0)].symbol(), "D");

    let without_heading = render(None);
    for row in [0, 4] {
        assert_eq!(without_heading[(0, row)].bg, Color::Reset);
        assert_eq!(without_heading[(1, row)].bg, Color::Blue);
        assert_eq!(without_heading[(28, row)].bg, Color::Blue);
        assert_eq!(without_heading[(29, row)].bg, Color::Reset);
    }
    assert_eq!(without_heading[(2, 0)].symbol(), "A");
}
