use crate::app::Model;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
};

pub fn render(model: &mut Model, frame: &mut Frame) {
    // Always render the chat view as base
    render_chat(model, frame);

    // Conditionally render agent router overlay on top
    if model.show_agent_router {
        let area = centered_rect(60, 40, frame.area());
        frame.render_widget(Clear, area);
        render_agent_router_overlay(model, frame, area);
    }
}

fn render_chat(model: &mut Model, frame: &mut Frame) {
    let area = frame.area();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Title
            Constraint::Length(4), // Input
            Constraint::Length(1), // Instructions
        ])
        .split(area);

    // Title - show selected agent
    let selected_agent = model
        .selected_agent_index
        .and_then(|i| model.agents.get(i))
        .map(|s| s.as_str())
        .unwrap_or("No agent selected");

    let title = Paragraph::new(format!("Agent: {}", selected_agent))
        .block(Block::default().borders(Borders::ALL))
        .style(Style::default().fg(Color::Cyan));
    frame.render_widget(title, chunks[0]);

    // Input - textarea (messages scroll in terminal scrollback above viewport)
    let input_block = Block::default().borders(Borders::ALL).title("Prompt");
    let inner_area = input_block.inner(chunks[1]);
    frame.render_widget(input_block, chunks[1]);
    frame.render_widget(&model.textarea, inner_area);

    // Instructions
    let instructions = Paragraph::new("Alt+Enter: send | /switch-model: agents | /exit: quit")
        .style(Style::default().fg(Color::Gray));
    frame.render_widget(instructions, chunks[2]);
}

fn render_agent_router_overlay(model: &mut Model, frame: &mut Frame, area: Rect) {
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
    let instructions = Paragraph::new("Use ↑/↓ to navigate, Enter to select, Esc to close")
        .block(Block::default().borders(Borders::ALL))
        .style(Style::default().fg(Color::Gray));
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
