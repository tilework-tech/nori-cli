use tui_components::shimmer::{ColorPalette, Shimmer};

#[cfg(test)]
use ratatui::{Terminal, backend::TestBackend};

#[test]
fn test_shimmer_basic() {
    let shimmer = Shimmer::new("Loading...");
    let mut terminal = Terminal::new(TestBackend::new(20, 1)).unwrap();
    terminal
        .draw(|frame| {
            frame.render_widget_ref(shimmer, frame.area());
        })
        .unwrap();

    insta::assert_snapshot!(terminal.backend());
}

#[test]
fn test_shimmer_empty() {
    let shimmer = Shimmer::new("");
    let mut terminal = Terminal::new(TestBackend::new(20, 1)).unwrap();
    terminal
        .draw(|frame| {
            frame.render_widget_ref(shimmer, frame.area());
        })
        .unwrap();

    insta::assert_snapshot!(terminal.backend());
}

#[test]
fn test_shimmer_long_text() {
    let shimmer = Shimmer::new("Processing a very long operation that takes time...");
    let mut terminal = Terminal::new(TestBackend::new(60, 1)).unwrap();
    terminal
        .draw(|frame| {
            frame.render_widget_ref(shimmer, frame.area());
        })
        .unwrap();

    insta::assert_snapshot!(terminal.backend());
}

#[test]
fn test_shimmer_custom_palette() {
    let palette = ColorPalette::new((50, 100, 150), (200, 220, 255));
    let shimmer = Shimmer::with_palette("Custom colors", palette);
    let mut terminal = Terminal::new(TestBackend::new(20, 1)).unwrap();
    terminal
        .draw(|frame| {
            frame.render_widget_ref(shimmer, frame.area());
        })
        .unwrap();

    insta::assert_snapshot!(terminal.backend());
}

#[test]
fn test_shimmer_unicode() {
    let shimmer = Shimmer::new("Loading… 🚀");
    let mut terminal = Terminal::new(TestBackend::new(20, 1)).unwrap();
    terminal
        .draw(|frame| {
            frame.render_widget_ref(shimmer, frame.area());
        })
        .unwrap();

    insta::assert_snapshot!(terminal.backend());
}
