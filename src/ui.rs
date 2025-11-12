use crate::app::Model;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, List, ListItem, Paragraph},
};
use tui_textarea::TextArea;
use unicode_width::UnicodeWidthStr;

pub fn calculate_textarea_height(textarea: &TextArea, available_width: u16) -> u16 {
    const MIN_HEIGHT: u16 = 3;
    const MAX_HEIGHT: u16 = 10;
    const BORDER_HEIGHT: u16 = 2;

    let available_width = available_width.max(1); // Prevent division by zero

    let mut total_lines = 0u16;
    for line in textarea.lines() {
        let line_width = line.width() as u16;
        // Calculate how many wrapped lines this will take
        let wrapped_lines = if line_width == 0 {
            1 // Empty line still takes 1 line
        } else {
            line_width.div_ceil(available_width).max(1)
        };
        total_lines += wrapped_lines;
    }

    let height = total_lines.clamp(MIN_HEIGHT, MAX_HEIGHT);
    height + BORDER_HEIGHT
}

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

    // Calculate dynamic textarea height
    let available_width = area.width.saturating_sub(2); // Account for borders
    let textarea_height = calculate_textarea_height(&model.textarea, available_width);

    // Adjust layout based on whether autocomplete is visible
    let constraints = if model.show_autocomplete {
        // Calculate height needed for autocomplete (2 commands + borders = 4 lines)
        let autocomplete_height =
            (model.autocomplete_filtered_commands.len() as u16).clamp(1, 6) + 2;
        vec![
            Constraint::Length(textarea_height),     // Input (dynamic)
            Constraint::Length(autocomplete_height), // Autocomplete
            Constraint::Length(1),                   // Shimmer
            Constraint::Length(1),                   // Instructions
        ]
    } else {
        vec![
            Constraint::Length(textarea_height), // Input (dynamic)
            Constraint::Length(1),               // Agent info
            Constraint::Length(1),               // Shimmer
            Constraint::Length(1),               // Instructions
        ]
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
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

    let debug_indicator = if model.show_debug_events {
        " [DEBUG]"
    } else {
        ""
    };

    let agent_info = Paragraph::new(format!("Agent: {selected_agent}{debug_indicator}"))
        .style(Style::default().fg(Color::Cyan));
    frame.render_widget(agent_info, chunks[1]);

    // Loading animation - show during streaming
    if model.current_mode == crate::app::AppMode::Streaming {
        if model.use_codex_components {
            // Use Shimmer component from tui-components
            use tui_components::Shimmer;
            let shimmer = Shimmer::new(format!("{selected_agent} processing..."));
            frame.render_widget(shimmer, chunks[2]);
        } else {
            // Use legacy spinner animation
            let frames = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
            let spinner_text = format!(
                "{} {}",
                frames[model.loading_frame % frames.len()],
                format!("{selected_agent} processing...")
            );
            let spinner = Paragraph::new(spinner_text);
            frame.render_widget(spinner, chunks[2]);
        }
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

    // Render autocomplete dropdown in its own layout chunk (if visible)
    if model.show_autocomplete {
        render_autocomplete_in_layout(model, frame, chunks[1]);
        frame.render_widget(instructions, chunks[3]); // Instructions at bottom
    } else {
        frame.render_widget(instructions, chunks[3]); // Instructions directly after shimmer
    }
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
                format!("{agent} [Not Installed]")
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

    if model.autocomplete_filtered_commands.is_empty() {
        return; // Nothing to show
    }

    // Build list items
    let items: Vec<ListItem> = model
        .autocomplete_filtered_commands
        .iter()
        .map(|cmd| ListItem::new(format!("/{cmd}")))
        .collect();

    // Create highlighted list - render directly in the provided area
    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title("Commands"))
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("> ");

    // Create stateful widget with current selection
    let mut list_state = ratatui::widgets::ListState::default();
    list_state.select(Some(model.autocomplete_selected_index));

    frame.render_stateful_widget(list, area, &mut list_state);
}
