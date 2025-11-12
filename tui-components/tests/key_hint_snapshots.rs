use codex_tui_components::key_hint::{alt, ctrl, plain, shift, KeyBinding};
use crossterm::event::KeyCode;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::Span;
use ratatui::widgets::Widget;

#[test]
fn test_plain_key() {
    let binding = plain(KeyCode::Enter);
    let span: Span = binding.into();

    let mut buf = Buffer::empty(Rect::new(0, 0, 20, 1));
    span.clone().render(buf.area, &mut buf);

    insta::assert_snapshot!(format!("{buf:?}"));
}

#[test]
fn test_ctrl_key() {
    let binding = ctrl(KeyCode::Char('c'));
    let span: Span = binding.into();

    let mut buf = Buffer::empty(Rect::new(0, 0, 20, 1));
    span.clone().render(buf.area, &mut buf);

    insta::assert_snapshot!(format!("{buf:?}"));
}

#[test]
fn test_alt_key() {
    let binding = alt(KeyCode::Char('f'));
    let span: Span = binding.into();

    let mut buf = Buffer::empty(Rect::new(0, 0, 20, 1));
    span.clone().render(buf.area, &mut buf);

    insta::assert_snapshot!(format!("{buf:?}"));
}

#[test]
fn test_shift_key() {
    let binding = shift(KeyCode::Tab);
    let span: Span = binding.into();

    let mut buf = Buffer::empty(Rect::new(0, 0, 20, 1));
    span.clone().render(buf.area, &mut buf);

    insta::assert_snapshot!(format!("{buf:?}"));
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
        let mut buf = Buffer::empty(Rect::new(0, 0, 10, 1));
        span.clone().render(buf.area, &mut buf);
        outputs.push(format!("{buf:?}"));
    }

    insta::assert_snapshot!(outputs.join("\n---\n"));
}

#[test]
fn test_page_keys() {
    let pg_up = plain(KeyCode::PageUp);
    let pg_down = plain(KeyCode::PageDown);

    let span1: Span = pg_up.into();
    let span2: Span = pg_down.into();

    let mut buf1 = Buffer::empty(Rect::new(0, 0, 10, 1));
    let mut buf2 = Buffer::empty(Rect::new(0, 0, 10, 1));

    span1.clone().render(buf1.area, &mut buf1);
    span2.clone().render(buf2.area, &mut buf2);

    insta::assert_snapshot!(format!("{buf1:?}\n---\n{buf2:?}"));
}

#[test]
fn test_multiple_modifiers() {
    use crossterm::event::KeyModifiers;

    let binding = KeyBinding::new(
        KeyCode::Char('s'),
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    );
    let span: Span = binding.into();

    let mut buf = Buffer::empty(Rect::new(0, 0, 20, 1));
    span.clone().render(buf.area, &mut buf);

    insta::assert_snapshot!(format!("{buf:?}"));
}
