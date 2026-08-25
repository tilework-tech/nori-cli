use super::*;
use insta::assert_snapshot;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::style::Color;
use ratatui::style::Style;
use ratatui::text::Span;
use ratatui::widgets::Paragraph;

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

fn configured_snapshot(
    width: u16,
    density: DetailDensity,
    layout: DetailLayout,
    row_pattern: DetailRowPattern,
) -> String {
    let entries = entries();
    let pane = || {
        DetailPane::new(&entries)
            .heading("Session details")
            .density(density)
            .layout(layout)
            .row_pattern(row_pattern)
    };
    let backend = TestBackend::new(width, pane().required_height(width));
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal
        .draw(|frame| frame.render_widget(pane(), frame.area()))
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
fn detail_pane_zebra_snapshot() {
    assert_snapshot!(configured_snapshot(
        42,
        DetailDensity::Compact,
        DetailLayout::Columns,
        DetailRowPattern::Zebra,
    ));
}

#[test]
fn detail_pane_normal_density_snapshot() {
    assert_snapshot!(configured_snapshot(
        42,
        DetailDensity::Normal,
        DetailLayout::Columns,
        DetailRowPattern::Plain,
    ));
}

#[test]
fn detail_pane_responsive_stacked_snapshot() {
    assert_snapshot!(configured_snapshot(
        22,
        DetailDensity::Compact,
        DetailLayout::Responsive { stack_below: 30 },
        DetailRowPattern::Plain,
    ));
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

#[test]
fn detail_pane_zebra_fills_the_surface_for_every_wrapped_line() {
    let entries = [
        DetailEntry::key_value("First", "aaaaaa bbbbbb").wrap(true),
        DetailEntry::key_value("Second", "two"),
    ];
    let backend = TestBackend::new(23, 3);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    let theme = Theme {
        detail_surface: Style::new().bg(Color::Blue),
        row: Style::new().bg(Color::Red),
        row_alt: Style::new().bg(Color::Green),
        ..Theme::default()
    };
    terminal
        .draw(|frame| {
            frame.render_widget(
                DetailPane::new(&entries)
                    .theme(theme)
                    .row_pattern(DetailRowPattern::Zebra),
                frame.area(),
            );
        })
        .expect("draw pane");
    let buffer = terminal.backend().buffer();

    for row in [0, 1] {
        assert_eq!(buffer[(1, row)].bg, Color::Red);
        assert_eq!(buffer[(21, row)].bg, Color::Red);
    }
    assert_eq!(buffer[(2, 0)].bg, Color::Red);
    assert_eq!(buffer[(1, 2)].bg, Color::Green);
    assert_eq!(buffer[(0, 0)].bg, Color::Reset);
}

#[test]
fn detail_pane_zebra_restarts_after_rules() {
    let entries = [
        DetailEntry::key_value("First", "one"),
        DetailEntry::key_value("Second", "two"),
        DetailEntry::key_value("Third", "three"),
        DetailEntry::Rule,
        DetailEntry::key_value("After", "four"),
    ];
    let backend = TestBackend::new(23, 5);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    let theme = Theme {
        detail_surface: Style::new().bg(Color::Blue),
        row: Style::new().bg(Color::Red),
        row_alt: Style::new().bg(Color::Green),
        ..Theme::default()
    };
    terminal
        .draw(|frame| {
            frame.render_widget(
                DetailPane::new(&entries)
                    .theme(theme)
                    .row_pattern(DetailRowPattern::Zebra),
                frame.area(),
            );
        })
        .expect("draw pane");
    let buffer = terminal.backend().buffer();

    assert_eq!(buffer[(1, 0)].bg, Color::Red);
    assert_eq!(buffer[(1, 1)].bg, Color::Green);
    assert_eq!(buffer[(1, 2)].bg, Color::Red);
    assert_eq!(buffer[(1, 3)].bg, Color::Blue);
    assert_eq!(buffer[(1, 4)].bg, Color::Red);
}

#[test]
fn detail_pane_density_controls_spacing_without_doubling_rules() {
    let entries = [
        DetailEntry::key_value("A", "one"),
        DetailEntry::key_value("B", "two"),
        DetailEntry::Rule,
        DetailEntry::key_value("C", "three"),
        DetailEntry::key_value("D", "four"),
    ];
    let render = |density| {
        let backend = TestBackend::new(30, 7);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        terminal
            .draw(|frame| {
                frame.render_widget(DetailPane::new(&entries).density(density), frame.area());
            })
            .expect("draw pane");
        terminal.backend().buffer().clone()
    };

    let compact = render(DetailDensity::Compact);
    assert_eq!(compact[(2, 0)].symbol(), "A");
    assert_eq!(compact[(2, 1)].symbol(), "B");
    assert_eq!(compact[(2, 3)].symbol(), "C");
    assert_eq!(compact[(2, 4)].symbol(), "D");

    let normal = render(DetailDensity::Normal);
    assert_eq!(normal[(2, 0)].symbol(), "A");
    assert_eq!(normal[(2, 2)].symbol(), "B");
    assert_eq!(normal[(2, 4)].symbol(), "C");
    assert_eq!(normal[(2, 6)].symbol(), "D");
    for row in [1, 3, 5] {
        assert!(
            (0..30).all(|x| normal[(x, row)].symbol() == " "),
            "normal density spacing and grouping rows should stay blank"
        );
    }
}

#[test]
fn detail_pane_responsive_layout_stacks_keys_above_inset_values() {
    let entries = [
        DetailEntry::key_value("Alpha", "one"),
        DetailEntry::key_value("Beta", "two"),
    ];
    let render = |width| {
        let backend = TestBackend::new(width, 4);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        terminal
            .draw(|frame| {
                frame.render_widget(
                    DetailPane::new(&entries).layout(DetailLayout::Responsive { stack_below: 30 }),
                    frame.area(),
                );
            })
            .expect("draw pane");
        terminal.backend().buffer().clone()
    };

    let narrow = render(29);
    assert_eq!(narrow[(2, 0)].symbol(), "A");
    assert_eq!(narrow[(4, 1)].symbol(), "o");
    assert_eq!(narrow[(2, 2)].symbol(), "B");
    assert_eq!(narrow[(4, 3)].symbol(), "t");

    let wide = render(30);
    assert_eq!(wide[(2, 0)].symbol(), "A");
    assert_eq!(wide[(9, 0)].symbol(), "o");
    assert_eq!(wide[(2, 1)].symbol(), "B");
    assert_eq!(wide[(9, 1)].symbol(), "t");
}

#[test]
fn detail_pane_required_height_matches_responsive_density_and_wrapping() {
    let entries = [
        DetailEntry::key_value("A", "first"),
        DetailEntry::key_value("Wrapped", "aaaaaa bbbbbb").wrap(true),
        DetailEntry::Rule,
        DetailEntry::key_value("After", "last"),
    ];
    let pane = || {
        DetailPane::new(&entries)
            .heading("Details")
            .density(DetailDensity::Normal)
            .layout(DetailLayout::Responsive { stack_below: 20 })
    };

    assert_eq!(pane().required_height(16), 11);
    assert_eq!(pane().required_height(40), 7);

    let backend = TestBackend::new(16, pane().required_height(16));
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal
        .draw(|frame| frame.render_widget(pane(), frame.area()))
        .expect("draw pane");
    let buffer = terminal.backend().buffer();
    assert_eq!(buffer[(2, 9)].symbol(), "A");
    assert_eq!(buffer[(4, 10)].symbol(), "l");

    let backend = TestBackend::new(40, pane().required_height(40));
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal
        .draw(|frame| frame.render_widget(pane(), frame.area()))
        .expect("draw pane");
    let buffer = terminal.backend().buffer();
    assert_eq!(buffer[(2, 6)].symbol(), "A");
    assert_eq!(buffer[(11, 6)].symbol(), "l");
}

#[test]
fn detail_pane_requires_positive_content_width() {
    let entries = [DetailEntry::key_value("Agent", "Codex")];

    assert_eq!(DetailPane::new(&entries).required_height(4), 0);
    assert_eq!(DetailPane::new(&entries).required_height(5), 1);
}

#[test]
fn detail_pane_stacked_labels_do_not_escape_the_caller_rectangle() {
    let entries = [DetailEntry::key_value("界".repeat(10), "value")];
    let backend = TestBackend::new(20, 2);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal
        .draw(|frame| {
            frame.render_widget(Paragraph::new("...................."), frame.area());
            frame.render_widget(
                DetailPane::new(&entries).layout(DetailLayout::Stacked),
                ratatui::layout::Rect::new(2, 0, 10, 2),
            );
        })
        .expect("draw pane");
    let buffer = terminal.backend().buffer();

    assert_eq!(buffer[(10, 0)].symbol(), ".");
    assert_eq!(buffer[(11, 0)].symbol(), ".");
    assert_eq!(buffer[(12, 0)].symbol(), ".");
}

#[test]
fn detail_pane_required_height_saturates_for_oversized_wrapped_values() {
    let entries =
        [DetailEntry::key_value("Payload", "x".repeat(usize::from(u16::MAX) + 1)).wrap(true)];

    assert_eq!(
        DetailPane::new(&entries)
            .layout(DetailLayout::Stacked)
            .required_height(7),
        u16::MAX
    );
}

#[test]
fn detail_pane_zebra_background_overrides_semantic_and_span_backgrounds() {
    let value = Line::from(Span::styled("Codex", Style::new().bg(Color::Yellow)));
    let entries = [DetailEntry::key_value("Agent", value)
        .tone(DetailTone::Info)
        .wrap(true)];
    let theme = Theme {
        detail_surface: Style::new().bg(Color::Blue),
        row: Style::new().bg(Color::Red),
        muted: Style::new().fg(Color::White).bg(Color::Green),
        info: Style::new().fg(Color::White).bg(Color::Magenta),
        ..Theme::default()
    };
    let backend = TestBackend::new(20, 1);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal
        .draw(|frame| {
            frame.render_widget(
                DetailPane::new(&entries)
                    .theme(theme)
                    .row_pattern(DetailRowPattern::Zebra),
                frame.area(),
            );
        })
        .expect("draw pane");
    let buffer = terminal.backend().buffer();

    assert_eq!(buffer[(2, 0)].bg, Color::Red);
    assert_eq!(buffer[(9, 0)].bg, Color::Red);
}
