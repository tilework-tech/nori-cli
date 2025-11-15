use tui_components::throbber::Throbber;

#[cfg(test)]
use ratatui::{Terminal, backend::TestBackend};

#[test]
fn test_throbber_basic() {
    let throbber = Throbber::new("Loading...");
    let mut terminal = Terminal::new(TestBackend::new(20, 1)).unwrap();
    terminal
        .draw(|frame| {
            frame.render_widget_ref(throbber, frame.area());
        })
        .unwrap();

    insta::assert_snapshot!(terminal.backend());
}

#[test]
fn test_throbber_empty() {
    let throbber = Throbber::new("");
    let mut terminal = Terminal::new(TestBackend::new(20, 1)).unwrap();
    terminal
        .draw(|frame| {
            frame.render_widget_ref(throbber, frame.area());
        })
        .unwrap();

    insta::assert_snapshot!(terminal.backend());
}

#[test]
fn test_throbber_long_text() {
    let throbber = Throbber::new("Processing a very long operation that takes time...");
    let mut terminal = Terminal::new(TestBackend::new(60, 1)).unwrap();
    terminal
        .draw(|frame| {
            frame.render_widget_ref(throbber, frame.area());
        })
        .unwrap();

    insta::assert_snapshot!(terminal.backend());
}

#[test]
fn test_throbber_custom_frames() {
    let frames = ["|", "/", "-", "\\"];
    let throbber = Throbber::with_frames("Custom frames", &frames);
    let mut terminal = Terminal::new(TestBackend::new(20, 1)).unwrap();
    terminal
        .draw(|frame| {
            frame.render_widget_ref(throbber, frame.area());
        })
        .unwrap();

    insta::assert_snapshot!(terminal.backend());
}

#[test]
fn test_throbber_unicode() {
    let throbber = Throbber::new("Loading… 🚀");
    let mut terminal = Terminal::new(TestBackend::new(20, 1)).unwrap();
    terminal
        .draw(|frame| {
            frame.render_widget_ref(throbber, frame.area());
        })
        .unwrap();

    insta::assert_snapshot!(terminal.backend());
}
