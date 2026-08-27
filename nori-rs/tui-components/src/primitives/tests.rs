use super::*;
use insta::assert_snapshot;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::style::Color;
use ratatui::style::Style;

fn snapshot_widget(widget: impl Widget, width: u16, height: u16) -> String {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal
        .draw(|frame| frame.render_widget(widget, frame.area()))
        .expect("draw widget");
    terminal.backend().to_string()
}

#[test]
fn semantic_messages_snapshot() {
    let messages = [
        SemanticMessage::new(MessageLevel::Info, "Connected to the agent"),
        SemanticMessage::new(MessageLevel::Success, "Snapshot accepted"),
        SemanticMessage::new(MessageLevel::Warning, "Two sessions are still running")
            .detail("Open the session picker to inspect them."),
        SemanticMessage::new(MessageLevel::Error, "Could not resume the session")
            .detail("The agent no longer reports that session id."),
    ];
    let backend = TestBackend::new(58, 10);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal
        .draw(|frame| {
            for (index, message) in messages.into_iter().enumerate() {
                let area = Rect::new(0, index as u16 * 2, frame.area().width, 2);
                frame.render_widget(message, area);
            }
        })
        .expect("draw messages");

    assert_snapshot!(terminal.backend().to_string());
}

#[test]
fn empty_state_snapshot() {
    assert_snapshot!(snapshot_widget(
        EmptyState::new("No matching sessions").detail("Try a title, project path, or session id."),
        52,
        3,
    ));
}

#[test]
fn empty_state_uses_the_informational_accent_not_the_pointer_accent() {
    let backend = TestBackend::new(32, 2);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    let theme = Theme {
        pointer: Style::new().fg(Color::Magenta),
        info: Style::new().fg(Color::Cyan),
        ..Theme::default()
    };
    terminal
        .draw(|frame| {
            frame.render_widget(EmptyState::new("No sessions").theme(theme), frame.area())
        })
        .expect("draw empty state");

    assert_eq!(terminal.backend().buffer()[(0, 0)].fg, Color::Cyan);
}

#[test]
fn key_hints_wrap_snapshot() {
    let hints = KeyHints::new([
        KeyHint::new("↑↓", "move"),
        KeyHint::new("enter", "open"),
        KeyHint::new("/", "search"),
        KeyHint::new("esc", "close"),
    ]);
    assert_snapshot!(snapshot_widget(hints, 29, 3));
}

#[test]
fn key_hints_start_at_the_left_edge_of_the_caller_area() {
    let backend = TestBackend::new(44, 3);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal
        .draw(|frame| {
            frame.render_widget(
                KeyHints::new([KeyHint::new("1-3", "choose"), KeyHint::new("esc", "close")]),
                Rect::new(3, 1, 40, 1),
            )
        })
        .expect("draw key hints");

    assert_eq!(terminal.backend().buffer()[(2, 1)].symbol(), " ");
    assert_eq!(terminal.backend().buffer()[(3, 1)].symbol(), "1");
}
