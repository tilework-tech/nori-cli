use crate::app::Model;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, List, ListItem, Paragraph},
};
use tui_components::render::Renderable;

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

    // Calculate dynamic textarea height using TextArea's built-in method
    let available_width = area.width.saturating_sub(2); // Account for borders
    let content_height = model.textarea.desired_height(available_width);
    let config = model.textarea.config();
    let total_height = content_height + config.padding_top + config.padding_bottom;
    // Apply max height constraint (from old calculate_textarea_height logic)
    const MAX_HEIGHT: u16 = 10;
    let textarea_height = total_height.min(MAX_HEIGHT + config.padding_top + config.padding_bottom);
    let inline_height = model.inline_height().min(area.height);

    // Build layout constraints
    let mut constraints = Vec::new();
    if inline_height > 0 {
        constraints.push(Constraint::Length(inline_height));
    }
    constraints.push(Constraint::Length(textarea_height));
    constraints.push(Constraint::Length(1)); // Agent info
    if model.show_autocomplete {
        let autocomplete_height = model.autocomplete_selection_list.desired_height(area.width);
        constraints.push(Constraint::Length(autocomplete_height));
    }
    constraints.push(Constraint::Length(1)); // Shimmer
    constraints.push(Constraint::Length(1)); // Instructions

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(area);

    let mut chunk_index = 0;

    if inline_height > 0 {
        render_inline_entries(model, frame, chunks[chunk_index]);
        chunk_index += 1;
    }

    // Input - textarea
    frame.render_widget(&model.textarea, chunks[chunk_index]);
    chunk_index += 1;

    // Agent info - show selected agent below prompt
    let selected_agent = model
        .agents
        .get(model.selected_agent_index)
        .map(|s| s.as_str())
        .unwrap_or("No agent selected");

    let debug_indicator = if model.show_debug_events {
        " [DEBUG]"
    } else {
        ""
    };

    let agent_info = Paragraph::new(format!("Agent: {selected_agent}{debug_indicator}"))
        .style(Style::default().fg(Color::Cyan));
    frame.render_widget(agent_info, chunks[chunk_index]);
    chunk_index += 1;

    let autocomplete_area = if model.show_autocomplete {
        let area = chunks[chunk_index];
        chunk_index += 1;
        Some(area)
    } else {
        None
    };

    // Loading animation - show during streaming
    let shimmer_chunk = chunks[chunk_index];
    chunk_index += 1;
    if model.current_mode == crate::app::AppMode::Streaming {
        // Use Shimmer component from tui-components
        use tui_components::Shimmer;
        let shimmer = Shimmer::new(format!("{selected_agent} processing..."));
        frame.render_widget(shimmer, shimmer_chunk);
    }

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
    frame.render_widget(instructions, chunks[chunk_index]);

    if let Some(area) = autocomplete_area {
        render_autocomplete_in_layout(model, frame, area);
    }
}

fn render_inline_entries(model: &Model, frame: &mut Frame, area: Rect) {
    if model.inline_entries.is_empty() {
        return;
    }

    use ratatui::text::Text;
    use ratatui::widgets::Paragraph;

    let mut lines = Vec::new();
    for entry in &model.inline_entries {
        lines.extend_from_slice(entry.lines());
    }

    let paragraph = Paragraph::new(Text::from(lines));
    frame.render_widget(paragraph, area);
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

    // Agent selection list
    model
        .agent_selection_list
        .render(chunks[1], frame.buffer_mut());

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
            Constraint::Length(5), // Options (3 items + 2 borders)
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
            "{backend_name} is not installed on your system.\n\nWould you like to install it now?"
        )
    } else {
        format!(
            "{backend_name} is not installed on your system.\n\nWould you like to open the installation page?"
        )
    };
    let message = Paragraph::new(message_text)
        .style(Style::default().fg(Color::White))
        .wrap(ratatui::widgets::Wrap { trim: false });
    frame.render_widget(message, chunks[1]);

    // Options as a list - show all 3 options when install_cmd exists
    let options: Vec<(&str, InstallChoice)> = if has_install_cmd {
        vec![
            ("Run Installation", InstallChoice::RunInstallation),
            ("Open Installation Page", InstallChoice::OpenInstallPage),
            ("Cancel", InstallChoice::Cancel),
        ]
    } else {
        vec![
            ("Open Installation Page", InstallChoice::OpenInstallPage),
            ("Cancel", InstallChoice::Cancel),
        ]
    };

    let items: Vec<ListItem> = options
        .iter()
        .map(|(label, choice)| {
            let is_selected = model.install_prompt_choice == *choice;

            let style = if is_selected {
                Style::default()
                    .bg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };

            let prefix = if is_selected { ">> " } else { "   " };
            ListItem::new(format!("{prefix}{label}")).style(style)
        })
        .collect();

    let list = List::new(items).block(Block::default().borders(Borders::ALL).title("Options"));
    frame.render_widget(list, chunks[2]);

    // Instructions
    let instructions = Paragraph::new("↑/↓: navigate | Enter: confirm | Esc: cancel")
        .style(Style::default().fg(Color::Gray));
    frame.render_widget(instructions, chunks[3]);
}

fn render_autocomplete_in_layout(model: &Model, frame: &mut Frame, area: Rect) {
    // Render autocomplete as part of the layout (not an overlay)
    // This area is the chunk allocated for autocomplete in the main layout

    if model.autocomplete_selection_list.selected_index().is_none() {
        return; // Nothing to show
    }

    // Render the autocomplete selection list
    model
        .autocomplete_selection_list
        .render(area, frame.buffer_mut());
}
