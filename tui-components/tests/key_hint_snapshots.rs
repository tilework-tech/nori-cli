use crossterm::event::KeyCode;
#[cfg(test)]
use ratatui::Terminal;
#[cfg(test)]
use ratatui::backend::TestBackend;
use ratatui::text::Span;
use tui_components::key_hint::{KeyBinding, alt, ctrl, plain, shift};

#[test]
fn test_plain_key() {
    let binding = plain(KeyCode::Enter);
    let span: Span = binding.into();

    let mut terminal = Terminal::new(TestBackend::new(20, 1)).unwrap();
    terminal
        .draw(|frame| {
            frame.render_widget(&span, frame.area());
        })
        .unwrap();

    insta::assert_snapshot!(terminal.backend());
}

#[test]
fn test_ctrl_key() {
    let binding = ctrl(KeyCode::Char('c'));
    let span: Span = binding.into();

    let mut terminal = Terminal::new(TestBackend::new(20, 1)).unwrap();
    terminal
        .draw(|frame| {
            frame.render_widget(&span, frame.area());
        })
        .unwrap();

    insta::assert_snapshot!(terminal.backend());
}

#[test]
fn test_alt_key() {
    let binding = alt(KeyCode::Char('f'));
    let span: Span = binding.into();

    let mut terminal = Terminal::new(TestBackend::new(20, 1)).unwrap();
    terminal
        .draw(|frame| {
            frame.render_widget(&span, frame.area());
        })
        .unwrap();

    insta::assert_snapshot!(terminal.backend());
}

#[test]
fn test_shift_key() {
    let binding = shift(KeyCode::Tab);
    let span: Span = binding.into();

    let mut terminal = Terminal::new(TestBackend::new(20, 1)).unwrap();
    terminal
        .draw(|frame| {
            frame.render_widget(&span, frame.area());
        })
        .unwrap();

    insta::assert_snapshot!(terminal.backend());
}

#[test]
fn test_arrow_keys() {
    let bindings = vec![
        plain(KeyCode::Up),
        plain(KeyCode::Down),
        plain(KeyCode::Left),
        plain(KeyCode::Right),
    ];

    let mut outputs = Vec::new();
    for binding in bindings {
        let span: Span = binding.into();
        let mut terminal = Terminal::new(TestBackend::new(10, 1)).unwrap();
        terminal
            .draw(|frame| {
                frame.render_widget(&span, frame.area());
            })
            .unwrap();
        outputs.push(terminal.backend().to_string());
    }

    insta::assert_snapshot!(outputs.join("\n---\n"));
}

#[test]
fn test_page_keys() {
    let pg_up = plain(KeyCode::PageUp);
    let pg_down = plain(KeyCode::PageDown);

    let span1: Span = pg_up.into();
    let span2: Span = pg_down.into();

    let mut terminal1 = Terminal::new(TestBackend::new(10, 1)).unwrap();
    let mut terminal2 = Terminal::new(TestBackend::new(10, 1)).unwrap();

    terminal1
        .draw(|frame| {
            frame.render_widget(&span1, frame.area());
        })
        .unwrap();
    terminal2
        .draw(|frame| {
            frame.render_widget(&span2, frame.area());
        })
        .unwrap();

    insta::assert_snapshot!(format!(
        "{}\n---\n{}",
        terminal1.backend(),
        terminal2.backend()
    ));
}

#[test]
fn test_multiple_modifiers() {
    use crossterm::event::KeyModifiers;

    let binding = KeyBinding::new(
        KeyCode::Char('s'),
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    );
    let span: Span = binding.into();

    let mut terminal = Terminal::new(TestBackend::new(20, 1)).unwrap();
    terminal
        .draw(|frame| {
            frame.render_widget(&span, frame.area());
        })
        .unwrap();

    insta::assert_snapshot!(terminal.backend());
}
