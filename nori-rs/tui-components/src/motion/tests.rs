#![allow(clippy::disallowed_methods)] // Verify the explicit RGB art palette.

use super::*;
use crate::Theme;
use pretty_assertions::assert_eq;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::style::Color;
use ratatui::style::Modifier;
use ratatui::style::Style;
use unicode_width::UnicodeWidthStr;

fn render(state: &MotionState, area: Rect, ascii: bool) -> Buffer {
    let mut buffer = Buffer::empty(area);
    state
        .background()
        .palette(MotionPalette::nori())
        .ascii(ascii)
        .render(area, &mut buffer);
    buffer
}

#[test]
fn every_scene_at_wide_and_narrow_sizes() {
    for scene in MotionScene::ALL {
        let mut state = MotionState::new(scene);
        state.advance(Duration::from_secs(12));
        for (width, height) in [(80, 24), (32, 12)] {
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal
                .draw(|frame| frame.render_widget(state.background(), frame.area()))
                .unwrap();
            insta::assert_snapshot!(
                format!("{}_{}x{}", scene.label().to_lowercase(), width, height),
                terminal.backend().to_string()
            );
        }
    }
}

#[test]
fn frames_are_deterministic_and_advance_for_every_scene() {
    let area = Rect::new(0, 0, 80, 24);
    for scene in MotionScene::ALL {
        let mut state = MotionState::new(scene);
        state.advance(Duration::from_secs(12));
        let before = render(&state, area, false);
        assert_eq!(render(&state, area, false), before);
        state.advance(Duration::from_secs(2));
        assert_ne!(
            render(&state, area, false),
            before,
            "{} must animate",
            scene.label()
        );
    }
}

#[test]
fn clips_to_buffer_without_changing_field_coordinates_or_touching_other_cells() {
    let state = MotionState::new(MotionScene::Blueprint);
    let area = Rect::new(3, 2, 20, 10);
    let entire = render(&state, area, false);
    let clip = Rect::new(10, 5, 8, 4);
    let mut clipped = Buffer::empty(clip);
    state
        .background()
        .palette(MotionPalette::nori())
        .render(area, &mut clipped);
    for y in clip.top()..clip.bottom() {
        for x in clip.left()..clip.right() {
            assert_eq!(&clipped[(x, y)], &entire[(x, y)]);
        }
    }
    let mut outer = Buffer::filled(Rect::new(0, 0, 30, 16), ratatui::buffer::Cell::new("!"));
    state.background().render(area, &mut outer);
    assert_eq!(outer[(0, 0)].symbol(), "!");
    assert_eq!(outer[(23, 12)].symbol(), "!");
    let original = outer.clone();
    state.background().render(Rect::new(3, 3, 0, 0), &mut outer);
    assert_eq!(outer, original);
    render(&state, Rect::new(5, 7, 1, 1), false);
}

#[test]
fn quiet_area_clears_old_content_and_styles_and_keeps_foreground_composable() {
    let state = MotionState::default();
    let area = Rect::new(3, 2, 60, 24);
    let quiet = Rect::new(20, 8, 20, 8);
    let mut buffer = Buffer::empty(area);
    buffer.set_style(
        area,
        Style::new().fg(Color::Red).add_modifier(Modifier::BOLD),
    );
    state
        .background()
        .palette(MotionPalette::nori())
        .quiet_area(quiet)
        .render(area, &mut buffer);
    for y in quiet.top()..quiet.bottom() {
        for x in quiet.left()..quiet.right() {
            let cell = &buffer[(x, y)];
            assert_eq!(
                (cell.symbol(), cell.fg, cell.bg, cell.modifier),
                (" ", Color::Reset, Color::Rgb(10, 15, 12), Modifier::empty())
            );
        }
    }
    ratatui::widgets::Paragraph::new("Sign in").render(quiet, &mut buffer);
    assert_eq!(buffer[(20, 8)].symbol(), "S");
    assert_eq!(buffer[(20, 8)].bg, Color::Rgb(10, 15, 12));
}

#[test]
fn default_palette_keeps_terminal_background_and_all_glyphs_are_single_cell() {
    let area = Rect::new(0, 0, 40, 20);
    for scene in MotionScene::ALL {
        let state = MotionState::new(scene);
        let mut buffer = Buffer::empty(area);
        state.background().render(area, &mut buffer);
        for cell in &buffer.content {
            assert_eq!(cell.bg, Color::Reset);
            assert_eq!(UnicodeWidthStr::width(cell.symbol()), 1);
        }
        let ascii = render(&state, area, true);
        assert!(ascii.content.iter().all(|cell| cell.symbol().is_ascii()));
    }
    let palette = MotionPalette::from_theme(Theme {
        muted: Style::new().fg(Color::Magenta),
        ..Theme::default()
    });
    assert_eq!(palette.style(0.4, false).fg, Some(Color::Magenta));
    assert_eq!(palette.style(0.4, false).bg, None);
    assert_eq!(
        MotionPalette::nori().style(1.0, false).fg,
        Some(Color::Rgb(125, 214, 160))
    );
}

#[test]
fn intermediate_zoom_and_fade_have_snapshots() {
    for (name, milliseconds) in [("zoom", 2100), ("fade", 4200)] {
        let mut state = MotionState::default();
        state.set_scene(MotionScene::Sandboxes);
        state.advance(Duration::from_millis(milliseconds));
        let mut terminal = Terminal::new(TestBackend::new(60, 18)).unwrap();
        terminal
            .draw(|frame| frame.render_widget(state.background(), frame.area()))
            .unwrap();
        insta::assert_snapshot!(name, terminal.backend().to_string());
    }
}

#[test]
fn explicit_transition_endpoints_clamp_and_match_static_scenes() {
    let area = Rect::new(0, 0, 24, 12);
    let draw = |widget: MotionBackground| {
        let mut buffer = Buffer::empty(area);
        widget.render(area, &mut buffer);
        buffer
    };
    for from in MotionScene::ALL {
        for to in MotionScene::ALL {
            let start = MotionBackground::new(from, Duration::from_secs(12));
            let end = MotionBackground::new(to, Duration::from_secs(12));
            for progress in [f64::NEG_INFINITY, -1.0, f64::NAN, 0.0] {
                assert_eq!(
                    draw(start.clone().transition_to(to, progress)),
                    draw(start.clone())
                );
            }
            for progress in [1.0, 2.0, f64::INFINITY] {
                assert_eq!(
                    draw(start.clone().transition_to(to, progress)),
                    draw(end.clone())
                );
            }
            assert_eq!(
                draw(start.transition_to(to, 0.4).reduced_motion(true)),
                draw(MotionBackground::new(to, Duration::ZERO)),
            );
        }
    }
}

#[test]
fn explicit_time_can_seek_and_matches_controller_frames() {
    let area = Rect::new(3, 2, 36, 20);
    // Nonchronological input demonstrates that rendering retains no playback state.
    for millis in [2400, 0, 3000, 900, 1500, 2400] {
        for formation in [
            MotionFormation::Cycle,
            MotionFormation::Platoon,
            MotionFormation::Muster,
        ] {
            let mut state = MotionState::new(MotionScene::Dots);
            state.advance(Duration::from_secs(12));
            state.set_scene(MotionScene::Formations);
            state.advance(Duration::from_millis(millis));
            let mut controlled = Buffer::empty(area);
            state
                .background()
                .formation(formation)
                .palette(MotionPalette::nori())
                .render(area, &mut controlled);
            let mut explicit = Buffer::empty(area);
            MotionBackground::new(MotionScene::Dots, Duration::from_millis(12000 + millis))
                .transition_to(MotionScene::Formations, millis as f64 / 3000.0)
                .formation(formation)
                .palette(MotionPalette::nori())
                .render(area, &mut explicit);
            assert_eq!(explicit, controlled);
        }
    }
}

#[test]
fn cached_zoom_is_independent_of_clip_and_previous_frames() {
    let area = Rect::new(3, 2, 100, 40);
    let clip = Rect::new(19, 9, 41, 19);
    for time in [12, 94, 36, 12] {
        for progress in [0.4, 0.8, 1.0] {
            for formation in [
                MotionFormation::Cycle,
                MotionFormation::Platoon,
                MotionFormation::Muster,
            ] {
                let widget = MotionBackground::new(MotionScene::Dots, Duration::from_secs(time))
                    .transition_to(MotionScene::Formations, progress)
                    .formation(formation)
                    .palette(MotionPalette::nori());
                let mut full = Buffer::empty(area);
                widget.clone().render(area, &mut full);
                let mut clipped = Buffer::empty(clip);
                widget.render(area, &mut clipped);
                for y in clip.top()..clip.bottom() {
                    for x in clip.left()..clip.right() {
                        assert_eq!(&full[(x, y)], &clipped[(x, y)]);
                    }
                }
            }
        }
    }
}

#[test]
fn sandbox_zoom_preserves_endpoints_and_never_blanks_the_field() {
    let area = Rect::new(0, 0, 80, 24);
    let time = Duration::from_secs(12);
    let frame = |background: MotionBackground| {
        let mut buffer = Buffer::empty(area);
        background
            .formation(MotionFormation::Muster)
            .palette(MotionPalette::nori())
            .render(area, &mut buffer);
        buffer
    };
    assert_eq!(
        frame(MotionBackground::sandbox_zoom(time, 0.0)),
        frame(MotionBackground::new(MotionScene::Sandboxes, time))
    );
    assert_eq!(
        frame(MotionBackground::sandbox_zoom(time, 1.0)),
        frame(MotionBackground::new(MotionScene::Formations, time))
    );
    assert_eq!(
        frame(MotionBackground::sandbox_zoom(time, f64::NAN)),
        frame(MotionBackground::sandbox_zoom(time, 0.0))
    );
    assert_eq!(
        frame(MotionBackground::sandbox_zoom(time, 0.5).reduced_motion(true)),
        frame(MotionBackground::new(
            MotionScene::Formations,
            Duration::ZERO
        ))
    );
    for step in 0..=100 {
        let buffer = frame(MotionBackground::sandbox_zoom(
            time,
            f64::from(step) / 100.0,
        ));
        assert!(
            buffer.content.iter().any(|cell| cell.symbol() != " "),
            "blank frame at step {step}"
        );
    }
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal
        .draw(|f| {
            f.render_widget(
                MotionBackground::sandbox_zoom(time, 0.5)
                    .formation(MotionFormation::Muster)
                    .palette(MotionPalette::nori()),
                f.area(),
            )
        })
        .unwrap();
    insta::assert_snapshot!("sandbox_zoom_midpoint", terminal.backend().to_string());
}
