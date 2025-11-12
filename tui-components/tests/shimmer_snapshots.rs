use codex_tui_components::shimmer::{ColorPalette, Shimmer};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::WidgetRef;

#[test]
fn test_shimmer_basic() {
    let shimmer = Shimmer::new("Loading...");
    let mut buf = Buffer::empty(Rect::new(0, 0, 20, 1));
    (&shimmer).render_ref(buf.area, &mut buf);

    insta::assert_snapshot!(format!("{buf:?}"));
}

#[test]
fn test_shimmer_empty() {
    let shimmer = Shimmer::new("");
    let mut buf = Buffer::empty(Rect::new(0, 0, 20, 1));
    (&shimmer).render_ref(buf.area, &mut buf);

    insta::assert_snapshot!(format!("{buf:?}"));
}

#[test]
fn test_shimmer_long_text() {
    let shimmer = Shimmer::new("Processing a very long operation that takes time...");
    let mut buf = Buffer::empty(Rect::new(0, 0, 60, 1));
    (&shimmer).render_ref(buf.area, &mut buf);

    insta::assert_snapshot!(format!("{buf:?}"));
}

#[test]
fn test_shimmer_custom_palette() {
    let palette = ColorPalette::new((50, 100, 150), (200, 220, 255));
    let shimmer = Shimmer::with_palette("Custom colors", palette);
    let mut buf = Buffer::empty(Rect::new(0, 0, 20, 1));
    (&shimmer).render_ref(buf.area, &mut buf);

    insta::assert_snapshot!(format!("{buf:?}"));
}

#[test]
fn test_shimmer_unicode() {
    let shimmer = Shimmer::new("Loading… 🚀");
    let mut buf = Buffer::empty(Rect::new(0, 0, 20, 1));
    (&shimmer).render_ref(buf.area, &mut buf);

    insta::assert_snapshot!(format!("{buf:?}"));
}
