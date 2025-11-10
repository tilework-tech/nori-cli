use color_eyre::Result;
use crossterm::event::{self, Event};
use ratatui::{
    layout::Alignment,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    DefaultTerminal, Frame,
};

fn render(frame: &mut Frame) {
    let styled_text = Line::from(vec![
        Span::styled(
            "Hello, ",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "World!",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::ITALIC),
        ),
    ]);

    let paragraph = Paragraph::new(styled_text).alignment(Alignment::Center);

    frame.render_widget(paragraph, frame.area());
}

fn main() {
    println!("Hello, world!");
}
