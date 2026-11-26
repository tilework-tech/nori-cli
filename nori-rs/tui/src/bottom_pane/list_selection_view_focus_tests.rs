use super::*;
use crate::app_event::AppEvent;
use pretty_assertions::assert_eq;
use ratatui::style::Color;
use ratatui::style::Modifier;
use tokio::sync::mpsc::unbounded_channel;

fn prompt() -> ListSelectionView {
    let (tx, _rx) = unbounded_channel::<AppEvent>();
    let items = ["Allow once", "Allow for this session", "Deny"]
        .into_iter()
        .map(|name| SelectionItem {
            name: name.to_string(),
            description: Some(format!("About {name}")),
            dismiss_on_select: true,
            actions: vec![Box::new(|_: &AppEventSender| {})],
            ..Default::default()
        })
        .collect();
    ListSelectionView::new(
        SelectionViewParams {
            title: Some("Run a synthetic command?".to_string()),
            items,
            ..Default::default()
        },
        AppEventSender::new(tx),
    )
}

fn render(view: &ListSelectionView, width: u16) -> Buffer {
    let area = Rect::new(0, 0, width, view.desired_height(width));
    let mut buf = Buffer::empty(area);
    view.render(area, &mut buf);
    buf
}

fn line(buf: &Buffer, y: u16) -> String {
    (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect()
}

fn row_of(buf: &Buffer, text: &str) -> u16 {
    (0..buf.area.height)
        .find(|y| line(buf, *y).contains(text))
        .unwrap_or_else(|| panic!("no row contains {text:?}"))
}

fn column_of(buf: &Buffer, y: u16, text: &str) -> u16 {
    let line = line(buf, y);
    let byte = line
        .find(text)
        .unwrap_or_else(|| panic!("{text:?} missing"));
    line[..byte].chars().count() as u16
}

/// Rails, pointer and their style on one row, left to right.
fn marks(buf: &Buffer, y: u16) -> Vec<(String, Color, bool)> {
    (0..buf.area.width)
        .map(|x| &buf[(x, y)])
        .filter(|cell| ["▏", "›", "▕"].contains(&cell.symbol()))
        .map(|cell| {
            (
                cell.symbol().to_string(),
                cell.fg,
                cell.modifier.contains(Modifier::BOLD),
            )
        })
        .collect()
}

#[test]
fn focused_prompt_row_has_rails_and_a_green_pointer_but_plain_text() {
    let buf = render(&prompt(), 48);
    let focused = row_of(&buf, "Allow once");

    assert_eq!(
        marks(&buf, focused),
        vec![
            ("▏".to_string(), Color::Green, true),
            ("›".to_string(), Color::Green, true),
            ("▕".to_string(), Color::Green, true),
        ]
    );
    assert!(line(&buf, focused).contains("▏› 1. Allow once"));
    let label = column_of(&buf, focused, "Allow once");
    assert_ne!(buf[(label, focused)].fg, Color::Green);
    assert!(!buf[(label, focused)].modifier.contains(Modifier::BOLD));
}

#[test]
fn wrapped_lines_of_the_focused_prompt_keep_the_rails() {
    let buf = render(&prompt(), 48);
    let description = row_of(&buf, "Allow once") + 1;

    assert_eq!(
        marks(&buf, description)
            .into_iter()
            .map(|(mark, _, _)| mark)
            .collect::<Vec<_>>(),
        vec!["▏", "▕"]
    );
}

#[test]
fn other_prompt_rows_have_no_marks_and_share_the_number_column() {
    let buf = render(&prompt(), 48);
    let focused = row_of(&buf, "Allow once");
    let other = row_of(&buf, "Deny");

    assert_eq!(marks(&buf, other), Vec::new());
    assert_eq!(column_of(&buf, other, "3."), column_of(&buf, focused, "1."));
    assert_eq!(
        column_of(&buf, other, "Deny"),
        column_of(&buf, focused, "Allow once")
    );
}

#[test]
fn moving_down_moves_the_marks() {
    let mut view = prompt();
    view.handle_key_event(crossterm::event::KeyEvent::from(
        crossterm::event::KeyCode::Down,
    ));
    let buf = render(&view, 48);

    assert_eq!(marks(&buf, row_of(&buf, "Allow once")), Vec::new());
    assert_eq!(marks(&buf, row_of(&buf, "Allow for this session")).len(), 3);
}

#[test]
fn prompt_focus_snapshot() {
    let buf = render(&prompt(), 48);
    let text = (0..buf.area.height)
        .map(|y| line(&buf, y))
        .collect::<Vec<_>>()
        .join("\n");
    insta::assert_snapshot!(text);
}

fn render_on(view: &ListSelectionView, width: u16, background: Option<(u8, u8, u8)>) -> Buffer {
    let area = Rect::new(0, 0, width, view.desired_height(width));
    let mut buf = Buffer::empty(area);
    view.render_for_background(area, &mut buf, background);
    buf
}

#[test]
#[allow(clippy::disallowed_methods)]
fn on_a_shaded_terminal_the_focused_row_differs_from_the_panel_across_the_row() {
    let buf = render_on(&prompt(), 48, Some((20, 20, 20)));
    let title = row_of(&buf, "Run a synthetic command?");
    let focused = row_of(&buf, "Allow once");
    let other = row_of(&buf, "Deny");
    let panel = buf[(column_of(&buf, title, "Run"), title)].bg;
    let left = column_of(&buf, focused, "▏");
    let right = column_of(&buf, focused, "▕");

    assert_eq!(panel, Color::Rgb(43, 43, 43));
    for x in left..=right {
        assert_ne!(buf[(x, focused)].bg, panel, "cell {x} of the focused row");
    }
    assert_eq!(buf[(column_of(&buf, other, "Deny"), other)].bg, panel);
    assert_eq!(
        marks(&buf, focused),
        vec![
            ("▏".to_string(), Color::Green, false),
            ("›".to_string(), Color::Green, false),
            ("▕".to_string(), Color::Green, false),
        ]
    );
}

#[test]
fn on_an_unshaded_terminal_the_focused_marks_are_bold_and_nothing_is_shaded() {
    let buf = render_on(&prompt(), 48, None);
    let focused = row_of(&buf, "Allow once");

    assert_eq!(
        marks(&buf, focused)
            .into_iter()
            .map(|(_, _, bold)| bold)
            .collect::<Vec<_>>(),
        vec![true, true, true]
    );
    let label = column_of(&buf, focused, "Allow once");
    assert_eq!(buf[(label, focused)].bg, Color::Reset);
}
