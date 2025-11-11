use crate::app::Model;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, List, ListItem, Paragraph},
};

pub fn render(model: &mut Model, frame: &mut Frame) {
    // Install prompt takes priority (blocking action)
    if model.show_install_prompt {
        render_install_prompt_fullscreen(model, frame);
        return;
    }

    // Agent router takes second priority
    if model.show_agent_router {
        render_agent_selection_fullscreen(model, frame);
        return;
    }

    // Default: render chat view
    render_chat(model, frame);
}

fn render_chat(model: &mut Model, frame: &mut Frame) {
    let area = frame.area();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4), // Input
            Constraint::Length(1), // Agent info
            Constraint::Length(1), // Instructions
        ])
        .split(area);

    // Input - textarea (messages scroll in terminal scrollback above viewport)
    let input_block = Block::default().borders(Borders::ALL);
    let inner_area = input_block.inner(chunks[0]);
    frame.render_widget(input_block, chunks[0]);
    frame.render_widget(&model.textarea, inner_area);

    // Agent info - show selected agent below prompt
    let selected_agent = model
        .selected_agent_index
        .and_then(|i| model.agents.get(i))
        .map(|s| s.as_str())
        .unwrap_or("No agent selected");

    let agent_info = Paragraph::new(format!("Agent: {}", selected_agent))
        .style(Style::default().fg(Color::Cyan));
    frame.render_widget(agent_info, chunks[1]);

    // Instructions - show error/hint message if present, otherwise show default instructions
    let instructions_text = if let Some(ref msg) = model.error_message {
        msg.clone()
    } else {
        "/switch-model: agents | /exit: quit".to_string()
    };

    let instructions_style = if model.error_message.is_some() {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::Gray)
    };

    let instructions = Paragraph::new(instructions_text).style(instructions_style);
    frame.render_widget(instructions, chunks[2]);
}

fn render_agent_selection_fullscreen(model: &mut Model, frame: &mut Frame) {
    let area = frame.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(3),
            Constraint::Length(2),
        ])
        .split(area);

    // Title
    let title = Paragraph::new("Agent Router - Select an Agent").style(
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    );
    frame.render_widget(title, chunks[0]);

    // Agent list with availability indication
    let items: Vec<ListItem> = model
        .agents
        .iter()
        .enumerate()
        .map(|(i, agent)| {
            let is_available = model.backend_availability.get(i).copied().unwrap_or(false);
            let text = if is_available {
                agent.to_string()
            } else {
                format!("{} [Not Installed]", agent)
            };
            let style = if is_available {
                Style::default()
            } else {
                Style::default().fg(Color::DarkGray)
            };
            ListItem::new(text).style(style)
        })
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
    let instructions = Paragraph::new("↑/↓: navigate | Enter: select | Esc: close")
        .style(Style::default().fg(Color::Gray));
    frame.render_widget(instructions, chunks[2]);
}

fn render_install_prompt_fullscreen(model: &Model, frame: &mut Frame) {
    use crate::app::InstallChoice;

    let area = frame.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2), // Title
            Constraint::Min(2),    // Message (flexible)
            Constraint::Length(3), // Options
            Constraint::Length(1), // Instructions
        ])
        .split(area);

    // Title
    let title = Paragraph::new("Backend Not Installed").style(
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    );
    frame.render_widget(title, chunks[0]);

    // Message
    let backend_name = model.install_prompt_backend.as_deref().unwrap_or("Backend");
    let has_install_cmd = model
        .install_prompt_cmd
        .as_ref()
        .map(|v| !v.is_empty())
        .unwrap_or(false);

    let message_text = if has_install_cmd {
        format!(
            "{} is not installed on your system.\n\nWould you like to install it now?",
            backend_name
        )
    } else {
        format!(
            "{} is not installed on your system.\n\nWould you like to open the installation page?",
            backend_name
        )
    };
    let message = Paragraph::new(message_text)
        .style(Style::default().fg(Color::White))
        .wrap(ratatui::widgets::Wrap { trim: false });
    frame.render_widget(message, chunks[1]);

    // Options as a list
    let first_option = if has_install_cmd {
        "Run Installation"
    } else {
        "Open Installation Page"
    };

    let options = [first_option, "Cancel"];

    let items: Vec<ListItem> = options
        .iter()
        .enumerate()
        .map(|(i, option)| {
            let is_selected = matches!(
                (i, model.install_prompt_choice),
                (0, InstallChoice::OpenInstallPage) | (1, InstallChoice::Cancel)
            );

            let style = if is_selected {
                Style::default()
                    .bg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };

            let prefix = if is_selected { ">> " } else { "   " };
            ListItem::new(format!("{}{}", prefix, option)).style(style)
        })
        .collect();

    let list = List::new(items).block(Block::default().borders(Borders::ALL).title("Options"));
    frame.render_widget(list, chunks[2]);

    // Instructions
    let instructions = Paragraph::new("↑/↓: navigate | Enter: confirm | Esc: cancel")
        .style(Style::default().fg(Color::Gray));
    frame.render_widget(instructions, chunks[3]);
}
