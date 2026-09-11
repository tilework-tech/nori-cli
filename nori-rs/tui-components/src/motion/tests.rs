#![allow(clippy::disallowed_methods)] // Verify the explicit RGB art palette.

use super::*;
use pretty_assertions::assert_eq;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use unicode_width::UnicodeWidthStr;

fn render(state: &MotionState, area: Rect, ascii: bool) -> Buffer {
    let mut buffer = Buffer::empty(area);
    MotionBackground::new(state)
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
                .draw(|frame| frame.render_widget(MotionBackground::new(&state), frame.area()))
                .unwrap();
            insta::assert_snapshot!(
                format!("{}_{}x{}", scene.label().to_lowercase(), width, height),
                terminal.backend().to_string()
            );
        }
    }
}

#[test]
fn transitions_reverse_from_the_current_frame_and_finish_without_overshoot() {
    let mut state = MotionState::default();
    state.set_scene(MotionScene::Blueprint);
    state.advance(Duration::from_secs(2));
    assert_eq!(state.position, 2.0 / 3.0);
    let before = state.position;
    state.set_scene(MotionScene::Dots);
    assert_eq!(state.position, before);
    state.advance(Duration::from_secs(1));
    assert_eq!(state.position, 1.0 / 3.0);
    state.advance(Duration::from_secs(100));
    assert_eq!((state.position, state.scene()), (0.0, MotionScene::Dots));
    state.set_scene(MotionScene::Blueprint);
    state.advance(Duration::MAX);
    assert_eq!(
        (state.position, state.scene()),
        (4.0, MotionScene::Blueprint)
    );
    // Very long-running clocks remain safe for field hash/index conversions.
    render(&state, Rect::new(0, 0, 20, 10), false);
}

#[test]
fn reduced_motion_freezes_the_entire_field_and_makes_scene_changes_immediate() {
    let area = Rect::new(0, 0, 60, 20);
    let mut state = MotionState::new(MotionScene::Creatures);
    state.advance(Duration::from_secs(12));
    state.set_reduced_motion(true);
    let before = render(&state, area, false);
    state.advance(Duration::from_secs(50));
    assert_eq!(render(&state, area, false), before);
    state.set_scene(MotionScene::Blueprint);
    assert_eq!(state.position, 4.0);
    state.set_reduced_motion(false);
    state.advance(Duration::from_secs(1));
    assert_eq!(state.elapsed, Duration::from_secs(13));
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
    MotionBackground::new(&state)
        .palette(MotionPalette::nori())
        .render(area, &mut clipped);
    for y in clip.top()..clip.bottom() {
        for x in clip.left()..clip.right() {
            assert_eq!(&clipped[(x, y)], &entire[(x, y)]);
        }
    }
    let mut outer = Buffer::filled(Rect::new(0, 0, 30, 16), ratatui::buffer::Cell::new("!"));
    MotionBackground::new(&state).render(area, &mut outer);
    assert_eq!(outer[(0, 0)].symbol(), "!");
    assert_eq!(outer[(23, 12)].symbol(), "!");
    let original = outer.clone();
    MotionBackground::new(&state).render(Rect::new(3, 3, 0, 0), &mut outer);
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
    MotionBackground::new(&state)
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
        MotionBackground::new(&state).render(area, &mut buffer);
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
            .draw(|frame| frame.render_widget(MotionBackground::new(&state), frame.area()))
            .unwrap();
        insta::assert_snapshot!(name, terminal.backend().to_string());
    }
}
