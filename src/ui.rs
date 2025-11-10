use crate::app::{AppMode, Model};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
};

pub fn render(model: &mut Model, frame: &mut Frame) {
    match model.current_mode {
        AppMode::Selection => render_selection(model, frame),
        AppMode::Input => render_input(model, frame),
        AppMode::Streaming => render_streaming(model, frame),
    }
}

fn render_selection(model: &mut Model, frame: &mut Frame) {
    let area = frame.area();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(5),
            Constraint::Length(3),
        ])
        .split(area);

    // Title
    let title = Paragraph::new("Agent Router - Select an Agent")
        .block(Block::default().borders(Borders::ALL))
        .style(Style::default().fg(Color::Cyan));
    frame.render_widget(title, chunks[0]);

    // Agent list
    let items: Vec<ListItem> = model
        .agents
        .iter()
        .map(|agent| ListItem::new(agent.as_str()))
        .collect();

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title("Agents"))
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol(">> ");

    frame.render_stateful_widget(list, chunks[1], &mut model.list_state);

    // Instructions
    let instructions = Paragraph::new("Use ↑/↓ to navigate, Enter to select, q to quit")
        .block(Block::default().borders(Borders::ALL))
        .style(Style::default().fg(Color::Gray));
    frame.render_widget(instructions, chunks[2]);

    // Show error if present
    if let Some(ref error) = model.error_message {
        let error_area = centered_rect(60, 20, area);
        let error_widget = Paragraph::new(error.as_str())
            .block(Block::default().borders(Borders::ALL).title("Error"))
            .style(Style::default().fg(Color::Red))
            .wrap(Wrap { trim: true });
        frame.render_widget(error_widget, error_area);
    }
}

fn render_input(model: &mut Model, frame: &mut Frame) {
    let area = frame.area();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(5),
            Constraint::Length(3),
        ])
        .split(area);

    // Title
    let selected_agent = model
        .selected_agent_index
        .and_then(|i| model.agents.get(i))
        .map(|s| s.as_str())
        .unwrap_or("Unknown");

    let title = Paragraph::new(format!("Agent: {} - Enter your prompt", selected_agent))
        .block(Block::default().borders(Borders::ALL))
        .style(Style::default().fg(Color::Cyan));
    frame.render_widget(title, chunks[0]);

    // Text input
    let input_block = Block::default().borders(Borders::ALL).title("Prompt");
    let inner_area = input_block.inner(chunks[1]);
    frame.render_widget(input_block, chunks[1]);
    frame.render_widget(&model.textarea, inner_area);

    // Instructions
    let instructions = Paragraph::new("Alt+Enter to submit, Esc to go back")
        .block(Block::default().borders(Borders::ALL))
        .style(Style::default().fg(Color::Gray));
    frame.render_widget(instructions, chunks[2]);
}

fn render_streaming(model: &Model, frame: &mut Frame) {
    let area = frame.area();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(5),
            Constraint::Length(3),
        ])
        .split(area);

    // Title - change color if there's an error
    let (title_text, title_color) = if model.error_message.is_some() {
        ("Error - See details below", Color::Red)
    } else {
        ("Streaming Response...", Color::Green)
    };
    let title = Paragraph::new(title_text)
        .block(Block::default().borders(Borders::ALL))
        .style(Style::default().fg(title_color));
    frame.render_widget(title, chunks[0]);

    // Response text
    let response_content = model.response_text.join("\n");
    let response = Paragraph::new(response_content)
        .block(Block::default().borders(Borders::ALL).title("Response"))
        .wrap(Wrap { trim: false });
    frame.render_widget(response, chunks[1]);

    // Instructions - show error message if present
    let instruction_text = if let Some(ref error) = model.error_message {
        format!("ERROR: {}", error)
    } else {
        "Receiving response... (Esc to go back, q to quit)".to_string()
    };
    let instructions = Paragraph::new(instruction_text)
        .block(Block::default().borders(Borders::ALL))
        .style(Style::default().fg(if model.error_message.is_some() {
            Color::Red
        } else {
            Color::Gray
        }))
        .wrap(Wrap { trim: true });
    frame.render_widget(instructions, chunks[2]);
}

// Helper function to create centered rect
fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}
