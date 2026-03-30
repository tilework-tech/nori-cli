//! MCP server picker: interactive management UI for MCP server connections.
//!
//! Displays configured MCP servers with the ability to toggle, remove, and add
//! new servers. Follows the same `BottomPaneView` pattern as the hotkey picker.

use std::collections::BTreeMap;

use codex_core::config::types::McpServerConfig;
use codex_core::config::types::McpServerTransportConfig;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use crossterm::event::KeyModifiers;
use ratatui::buffer::Buffer;
use ratatui::layout::Constraint;
use ratatui::layout::Layout;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Block;
use ratatui::widgets::Widget;

use crate::app_event::AppEvent;
use crate::app_event_sender::AppEventSender;
use crate::bottom_pane::BottomPaneView;
use crate::bottom_pane::CancellationEvent;
use crate::render::Insets;
use crate::render::RectExt as _;
use crate::render::renderable::Renderable;
use crate::style::user_message_style;

/// The current mode of the picker state machine.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) enum Mode {
    /// Browsing the list of servers (+ "Add new..." at top).
    List,
    /// Confirming deletion of the server at the given list index.
    ConfirmDelete(usize),
    /// Choosing transport type for a new server (Stdio / HTTP).
    TransportSelect { selected: TransportChoice },
    /// Typing the server name.
    NameInput,
    /// Typing the command (stdio transport).
    CommandInput,
    /// Typing the args (stdio transport, space-separated).
    ArgsInput,
    /// Typing the URL (http transport).
    UrlInput,
    /// Typing an env var in KEY=VAL format (both transports).
    EnvInput,
    /// Typing a header in Key: Value format (http transport only).
    HeaderInput,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) enum TransportChoice {
    Stdio,
    Http,
}

/// State for the MCP server picker view.
pub(crate) struct McpServerPickerView {
    /// Sorted list of (name, config) pairs.
    servers: Vec<(String, McpServerConfig)>,
    /// Current UI mode.
    mode: Mode,
    /// Currently highlighted row in list mode (0 = "Add new...").
    selected_idx: usize,
    /// Whether the view should be dismissed.
    complete: bool,
    /// Channel to send config change events.
    app_event_tx: AppEventSender,

    // --- Wizard state ---
    /// Text buffer for the current input field.
    input_buffer: String,
    /// Accumulated wizard fields.
    wizard_name: String,
    wizard_transport: TransportChoice,
    wizard_command: String,
    wizard_args: String,
    wizard_url: String,
    wizard_env: Vec<(String, String)>,
    wizard_headers: Vec<(String, String)>,
}

impl McpServerPickerView {
    pub fn new(servers: &BTreeMap<String, McpServerConfig>, app_event_tx: AppEventSender) -> Self {
        let servers: Vec<(String, McpServerConfig)> = servers
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        Self {
            servers,
            mode: Mode::List,
            selected_idx: 0,
            complete: false,
            app_event_tx,
            input_buffer: String::new(),
            wizard_name: String::new(),
            wizard_transport: TransportChoice::Stdio,
            wizard_command: String::new(),
            wizard_args: String::new(),
            wizard_url: String::new(),
            wizard_env: Vec::new(),
            wizard_headers: Vec::new(),
        }
    }

    /// Total number of items in list mode: "Add new..." + servers.
    fn item_count(&self) -> usize {
        1 + self.servers.len()
    }

    /// Whether the given list index is the "Add new..." row.
    fn is_add_new_idx(&self, idx: usize) -> bool {
        idx == 0
    }

    /// Get the server index (into `self.servers`) for a list index.
    fn server_idx(&self, list_idx: usize) -> Option<usize> {
        if list_idx == 0 {
            None
        } else {
            Some(list_idx - 1)
        }
    }

    fn move_up(&mut self) {
        if self.item_count() == 0 {
            return;
        }
        if self.selected_idx == 0 {
            self.selected_idx = self.item_count() - 1;
        } else {
            self.selected_idx -= 1;
        }
    }

    fn move_down(&mut self) {
        if self.item_count() == 0 {
            return;
        }
        self.selected_idx = (self.selected_idx + 1) % self.item_count();
    }

    fn handle_list_enter(&mut self) {
        if self.is_add_new_idx(self.selected_idx) {
            // Start the add wizard
            self.reset_wizard();
            self.mode = Mode::TransportSelect {
                selected: TransportChoice::Stdio,
            };
        } else if let Some(server_idx) = self.server_idx(self.selected_idx) {
            // Toggle enabled
            if let Some((_, config)) = self.servers.get_mut(server_idx) {
                config.enabled = !config.enabled;
            }
            self.save_servers();
        }
    }

    fn handle_list_delete(&mut self) {
        if !self.is_add_new_idx(self.selected_idx) {
            self.mode = Mode::ConfirmDelete(self.selected_idx);
        }
    }

    fn confirm_delete(&mut self) {
        if let Mode::ConfirmDelete(list_idx) = self.mode
            && let Some(server_idx) = self.server_idx(list_idx)
        {
            self.servers.remove(server_idx);
            // Adjust selected index if needed
            if self.selected_idx >= self.item_count() && self.selected_idx > 0 {
                self.selected_idx = self.item_count() - 1;
            }
            self.save_servers();
        }
        self.mode = Mode::List;
    }

    fn cancel_delete(&mut self) {
        self.mode = Mode::List;
    }

    fn reset_wizard(&mut self) {
        self.input_buffer.clear();
        self.wizard_name.clear();
        self.wizard_transport = TransportChoice::Stdio;
        self.wizard_command.clear();
        self.wizard_args.clear();
        self.wizard_url.clear();
        self.wizard_env.clear();
        self.wizard_headers.clear();
    }

    fn handle_transport_select_enter(&mut self) {
        if let Mode::TransportSelect { selected } = self.mode {
            self.wizard_transport = selected;
            self.mode = Mode::NameInput;
            self.input_buffer.clear();
        }
    }

    fn toggle_transport(&mut self) {
        if let Mode::TransportSelect { ref mut selected } = self.mode {
            *selected = match selected {
                TransportChoice::Stdio => TransportChoice::Http,
                TransportChoice::Http => TransportChoice::Stdio,
            };
        }
    }

    fn handle_name_submit(&mut self) {
        let name = self.input_buffer.trim().to_string();
        if name.is_empty() {
            return;
        }
        self.wizard_name = name;
        self.input_buffer.clear();
        match self.wizard_transport {
            TransportChoice::Stdio => self.mode = Mode::CommandInput,
            TransportChoice::Http => self.mode = Mode::UrlInput,
        }
    }

    fn handle_command_submit(&mut self) {
        let command = self.input_buffer.trim().to_string();
        if command.is_empty() {
            return;
        }
        self.wizard_command = command;
        self.input_buffer.clear();
        self.mode = Mode::ArgsInput;
    }

    fn handle_args_submit(&mut self) {
        self.wizard_args = self.input_buffer.trim().to_string();
        self.input_buffer.clear();
        self.mode = Mode::EnvInput;
    }

    fn handle_url_submit(&mut self) {
        let url = self.input_buffer.trim().to_string();
        if url.is_empty() {
            return;
        }
        self.wizard_url = url;
        self.input_buffer.clear();
        // HTTP has no general env field — go straight to headers
        self.mode = Mode::HeaderInput;
    }

    fn handle_env_submit(&mut self) {
        let input = self.input_buffer.trim().to_string();
        self.input_buffer.clear();
        if input.is_empty() {
            // Empty input means "done with env vars" — only used for Stdio
            self.finish_wizard();
            return;
        }
        if let Some((key, val)) = input.split_once('=') {
            let key = key.trim().to_string();
            let val = val.trim().to_string();
            if !key.is_empty() {
                self.wizard_env.push((key, val));
            }
        }
        // Stay in EnvInput mode for more entries
    }

    fn handle_header_submit(&mut self) {
        let input = self.input_buffer.trim().to_string();
        self.input_buffer.clear();
        if input.is_empty() {
            // Empty input means "done with headers"
            self.finish_wizard();
            return;
        }
        if let Some((key, val)) = input.split_once(':') {
            let key = key.trim().to_string();
            let val = val.trim().to_string();
            if !key.is_empty() {
                self.wizard_headers.push((key, val));
            }
        }
        // Stay in HeaderInput mode for more entries
    }

    fn finish_wizard(&mut self) {
        let config = match self.wizard_transport {
            TransportChoice::Stdio => {
                let args: Vec<String> = if self.wizard_args.is_empty() {
                    vec![]
                } else {
                    self.wizard_args
                        .split_whitespace()
                        .map(String::from)
                        .collect()
                };
                let env = if self.wizard_env.is_empty() {
                    None
                } else {
                    Some(self.wizard_env.iter().cloned().collect())
                };
                McpServerConfig {
                    transport: McpServerTransportConfig::Stdio {
                        command: self.wizard_command.clone(),
                        args,
                        env,
                        env_vars: vec![],
                        cwd: None,
                    },
                    enabled: true,
                    startup_timeout_sec: None,
                    tool_timeout_sec: None,
                    enabled_tools: None,
                    disabled_tools: None,
                }
            }
            TransportChoice::Http => {
                let http_headers = if self.wizard_headers.is_empty() {
                    None
                } else {
                    Some(self.wizard_headers.iter().cloned().collect())
                };
                McpServerConfig {
                    transport: McpServerTransportConfig::StreamableHttp {
                        url: self.wizard_url.clone(),
                        bearer_token_env_var: None,
                        http_headers,
                        env_http_headers: None,
                    },
                    enabled: true,
                    startup_timeout_sec: None,
                    tool_timeout_sec: None,
                    enabled_tools: None,
                    disabled_tools: None,
                }
            }
        };

        // Prevent duplicate names from silently overwriting
        if self
            .servers
            .iter()
            .any(|(name, _)| name == &self.wizard_name)
        {
            // Name already exists — stay in name input so user can pick a different name
            self.mode = Mode::NameInput;
            self.input_buffer = self.wizard_name.clone();
            return;
        }

        self.servers.push((self.wizard_name.clone(), config));
        self.save_servers();
        self.mode = Mode::List;
        // Select the newly added server (last in list, but list index = servers.len())
        self.selected_idx = self.servers.len(); // "Add new" is 0, so last server is at len
    }

    fn save_servers(&self) {
        let map: BTreeMap<String, McpServerConfig> = self
            .servers
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        self.app_event_tx.send(AppEvent::SaveMcpServers(map));
    }

    fn handle_text_input(&mut self, key_event: KeyEvent) {
        match key_event.code {
            KeyCode::Char(c) => {
                self.input_buffer.push(c);
            }
            KeyCode::Backspace => {
                self.input_buffer.pop();
            }
            _ => {}
        }
    }

    fn go_back(&mut self) {
        match &self.mode {
            Mode::List => {
                self.complete = true;
            }
            Mode::ConfirmDelete(_) => {
                self.cancel_delete();
            }
            Mode::TransportSelect { .. } => {
                self.mode = Mode::List;
            }
            Mode::NameInput => {
                self.mode = Mode::TransportSelect {
                    selected: self.wizard_transport,
                };
                self.input_buffer.clear();
            }
            Mode::CommandInput => {
                self.mode = Mode::NameInput;
                self.input_buffer = self.wizard_name.clone();
            }
            Mode::ArgsInput => {
                self.mode = Mode::CommandInput;
                self.input_buffer = self.wizard_command.clone();
            }
            Mode::UrlInput => {
                self.mode = Mode::NameInput;
                self.input_buffer = self.wizard_name.clone();
            }
            Mode::EnvInput => {
                // EnvInput is only used for Stdio transport
                self.mode = Mode::ArgsInput;
                self.input_buffer = self.wizard_args.clone();
            }
            Mode::HeaderInput => {
                // HeaderInput is only used for HTTP transport; go back to URL
                self.mode = Mode::UrlInput;
                self.input_buffer = self.wizard_url.clone();
            }
        }
    }

    /// Current mode title for the wizard.
    fn wizard_title(&self) -> &'static str {
        match &self.mode {
            Mode::List => "MCP Servers",
            Mode::ConfirmDelete(_) => "MCP Servers",
            Mode::TransportSelect { .. } => "Add MCP Server",
            Mode::NameInput => "Add MCP Server",
            Mode::CommandInput => "Add MCP Server",
            Mode::ArgsInput => "Add MCP Server",
            Mode::UrlInput => "Add MCP Server",
            Mode::EnvInput => "Add MCP Server",
            Mode::HeaderInput => "Add MCP Server",
        }
    }

    /// Current mode subtitle for the wizard.
    fn wizard_subtitle(&self) -> &'static str {
        match &self.mode {
            Mode::List => "Manage MCP server connections",
            Mode::ConfirmDelete(_) => "Press d again to confirm, esc to cancel",
            Mode::TransportSelect { .. } => "Select transport type",
            Mode::NameInput => "Enter server name",
            Mode::CommandInput => "Enter command to run",
            Mode::ArgsInput => "Enter args (space-separated, or empty to skip)",
            Mode::UrlInput => "Enter server URL",
            Mode::EnvInput => "Enter env var (KEY=VALUE, or empty to finish)",
            Mode::HeaderInput => "Enter header (Key: Value, or empty to finish)",
        }
    }

    /// Footer hint for the current mode.
    fn footer_hint(&self) -> &'static str {
        match &self.mode {
            Mode::List => "↑↓ select · enter toggle · d delete · esc close",
            Mode::ConfirmDelete(_) => "d confirm delete · esc cancel",
            Mode::TransportSelect { .. } => "↑↓ select · enter choose · esc back",
            Mode::NameInput
            | Mode::CommandInput
            | Mode::ArgsInput
            | Mode::UrlInput
            | Mode::EnvInput
            | Mode::HeaderInput => "enter submit · esc back",
        }
    }

    #[cfg(test)]
    pub(crate) fn mode(&self) -> &Mode {
        &self.mode
    }

    #[cfg(test)]
    pub(crate) fn selected_idx(&self) -> usize {
        self.selected_idx
    }

    #[cfg(test)]
    pub(crate) fn input_buffer(&self) -> &str {
        &self.input_buffer
    }
}

impl BottomPaneView for McpServerPickerView {
    fn handle_key_event(&mut self, key_event: KeyEvent) {
        if key_event.kind != KeyEventKind::Press && key_event.kind != KeyEventKind::Repeat {
            return;
        }

        match &self.mode {
            Mode::List => match key_event {
                KeyEvent {
                    code: KeyCode::Up, ..
                }
                | KeyEvent {
                    code: KeyCode::Char('k'),
                    modifiers: KeyModifiers::NONE,
                    ..
                } => self.move_up(),
                KeyEvent {
                    code: KeyCode::Down,
                    ..
                }
                | KeyEvent {
                    code: KeyCode::Char('j'),
                    modifiers: KeyModifiers::NONE,
                    ..
                } => self.move_down(),
                KeyEvent {
                    code: KeyCode::Enter,
                    modifiers: KeyModifiers::NONE,
                    ..
                } => self.handle_list_enter(),
                KeyEvent {
                    code: KeyCode::Char('d'),
                    modifiers: KeyModifiers::NONE,
                    ..
                } => self.handle_list_delete(),
                KeyEvent {
                    code: KeyCode::Esc, ..
                } => self.go_back(),
                _ => {}
            },
            Mode::ConfirmDelete(_) => match key_event.code {
                KeyCode::Char('d') => self.confirm_delete(),
                KeyCode::Esc => self.cancel_delete(),
                _ => {}
            },
            Mode::TransportSelect { .. } => match key_event {
                KeyEvent {
                    code: KeyCode::Up, ..
                }
                | KeyEvent {
                    code: KeyCode::Down,
                    ..
                }
                | KeyEvent {
                    code: KeyCode::Char('k'),
                    modifiers: KeyModifiers::NONE,
                    ..
                }
                | KeyEvent {
                    code: KeyCode::Char('j'),
                    modifiers: KeyModifiers::NONE,
                    ..
                } => self.toggle_transport(),
                KeyEvent {
                    code: KeyCode::Enter,
                    modifiers: KeyModifiers::NONE,
                    ..
                } => self.handle_transport_select_enter(),
                KeyEvent {
                    code: KeyCode::Esc, ..
                } => self.go_back(),
                _ => {}
            },
            Mode::NameInput => match key_event.code {
                KeyCode::Enter => self.handle_name_submit(),
                KeyCode::Esc => self.go_back(),
                _ => self.handle_text_input(key_event),
            },
            Mode::CommandInput => match key_event.code {
                KeyCode::Enter => self.handle_command_submit(),
                KeyCode::Esc => self.go_back(),
                _ => self.handle_text_input(key_event),
            },
            Mode::ArgsInput => match key_event.code {
                KeyCode::Enter => self.handle_args_submit(),
                KeyCode::Esc => self.go_back(),
                _ => self.handle_text_input(key_event),
            },
            Mode::UrlInput => match key_event.code {
                KeyCode::Enter => self.handle_url_submit(),
                KeyCode::Esc => self.go_back(),
                _ => self.handle_text_input(key_event),
            },
            Mode::EnvInput => match key_event.code {
                KeyCode::Enter => self.handle_env_submit(),
                KeyCode::Esc => self.go_back(),
                _ => self.handle_text_input(key_event),
            },
            Mode::HeaderInput => match key_event.code {
                KeyCode::Enter => self.handle_header_submit(),
                KeyCode::Esc => self.go_back(),
                _ => self.handle_text_input(key_event),
            },
        }
    }

    fn is_complete(&self) -> bool {
        self.complete
    }

    fn on_ctrl_c(&mut self) -> CancellationEvent {
        self.complete = true;
        CancellationEvent::Handled
    }
}

impl Renderable for McpServerPickerView {
    fn desired_height(&self, _width: u16) -> u16 {
        let content_rows = match &self.mode {
            Mode::List | Mode::ConfirmDelete(_) => {
                // title + subtitle + blank + items + blank + footer
                3 + self.item_count() + 2
            }
            Mode::TransportSelect { .. } => {
                // title + subtitle + blank + 2 options + blank + footer
                3 + 2 + 2
            }
            Mode::NameInput
            | Mode::CommandInput
            | Mode::ArgsInput
            | Mode::UrlInput
            | Mode::EnvInput
            | Mode::HeaderInput => {
                // title + subtitle + blank + input line + blank + footer
                // + env/header entries if any
                let extra = match &self.mode {
                    Mode::EnvInput => self.wizard_env.len(),
                    Mode::HeaderInput => self.wizard_headers.len(),
                    _ => 0,
                };
                3 + 1 + extra + 2
            }
        };
        // Plus vertical inset (1 top + 1 bottom)
        (content_rows + 2) as u16
    }

    fn render(&self, area: Rect, buf: &mut Buffer) {
        if area.height == 0 || area.width == 0 {
            return;
        }

        Block::default()
            .style(user_message_style())
            .render(area, buf);

        let content_area = area.inset(Insets::vh(1, 2));
        if content_area.height == 0 || content_area.width == 0 {
            return;
        }

        let mut constraints = vec![
            Constraint::Length(1), // title
            Constraint::Length(1), // subtitle
            Constraint::Length(1), // blank
        ];

        match &self.mode {
            Mode::List | Mode::ConfirmDelete(_) => {
                for _ in 0..self.item_count() {
                    constraints.push(Constraint::Length(1));
                }
            }
            Mode::TransportSelect { .. } => {
                constraints.push(Constraint::Length(1)); // stdio
                constraints.push(Constraint::Length(1)); // http
            }
            Mode::EnvInput => {
                for _ in &self.wizard_env {
                    constraints.push(Constraint::Length(1));
                }
                constraints.push(Constraint::Length(1)); // input line
            }
            Mode::HeaderInput => {
                for _ in &self.wizard_headers {
                    constraints.push(Constraint::Length(1));
                }
                constraints.push(Constraint::Length(1)); // input line
            }
            _ => {
                constraints.push(Constraint::Length(1)); // input line
            }
        }

        constraints.push(Constraint::Length(1)); // blank
        constraints.push(Constraint::Length(1)); // footer

        let areas = Layout::vertical(constraints).split(content_area);
        let mut row = 0;

        // Title
        Line::from(self.wizard_title().bold()).render(areas[row], buf);
        row += 1;

        // Subtitle
        Line::from(self.wizard_subtitle().dim()).render(areas[row], buf);
        row += 1;

        // Blank
        row += 1;

        match &self.mode {
            Mode::List | Mode::ConfirmDelete(_) => {
                let confirm_idx = if let Mode::ConfirmDelete(idx) = self.mode {
                    Some(idx)
                } else {
                    None
                };

                // "Add new..." row
                {
                    let is_selected = self.selected_idx == 0;
                    let prefix = if is_selected { "› " } else { "  " };
                    let line = if is_selected {
                        Line::from(vec![
                            prefix.to_string().bold(),
                            "+ Add new...".to_string().bold(),
                        ])
                    } else {
                        Line::from(vec![prefix.into(), "+ Add new...".dim()])
                    };
                    line.render(areas[row], buf);
                    row += 1;
                }

                // Server rows
                for (idx, (name, config)) in self.servers.iter().enumerate() {
                    let list_idx = idx + 1;
                    let is_selected = list_idx == self.selected_idx;
                    let prefix = if is_selected { "› " } else { "  " };

                    let transport_label = match &config.transport {
                        McpServerTransportConfig::Stdio { .. } => "stdio",
                        McpServerTransportConfig::StreamableHttp { .. } => "http",
                    };

                    let status = if config.enabled { "on" } else { "off" };
                    let is_confirming = confirm_idx == Some(list_idx);

                    let right_text = if is_confirming {
                        "press d to confirm delete".to_string()
                    } else {
                        format!("{transport_label} ({status})")
                    };

                    let left_len = prefix.len() + name.len();
                    let right_len = right_text.len();
                    let total_width = areas[row].width as usize;
                    let padding = total_width.saturating_sub(left_len + right_len);

                    let spans: Vec<Span<'static>> = if is_confirming {
                        vec![
                            prefix.to_string().bold(),
                            name.clone().bold(),
                            " ".repeat(padding).into(),
                            right_text.red(),
                        ]
                    } else if is_selected {
                        vec![
                            prefix.to_string().bold(),
                            name.clone().bold(),
                            " ".repeat(padding).into(),
                            right_text.cyan(),
                        ]
                    } else {
                        vec![
                            prefix.to_string().into(),
                            name.clone().into(),
                            " ".repeat(padding).into(),
                            right_text.dim(),
                        ]
                    };

                    Line::from(spans).render(areas[row], buf);
                    row += 1;
                }
            }
            Mode::TransportSelect { selected } => {
                let options = [
                    (TransportChoice::Stdio, "Stdio", "Run a local command"),
                    (TransportChoice::Http, "HTTP", "Connect to a remote URL"),
                ];
                for (choice, label, desc) in &options {
                    let is_selected = selected == choice;
                    let prefix = if is_selected { "› " } else { "  " };
                    let line = if is_selected {
                        Line::from(vec![
                            prefix.to_string().bold(),
                            label.to_string().bold(),
                            format!(" — {desc}").dim(),
                        ])
                    } else {
                        Line::from(vec![
                            prefix.into(),
                            label.to_string().dim(),
                            format!(" — {desc}").dim(),
                        ])
                    };
                    line.render(areas[row], buf);
                    row += 1;
                }
            }
            Mode::EnvInput => {
                for (key, val) in &self.wizard_env {
                    let line = Line::from(format!("  {key}={val}").dim());
                    line.render(areas[row], buf);
                    row += 1;
                }
                let prompt = format!("> {}_", self.input_buffer);
                Line::from(prompt).render(areas[row], buf);
                row += 1;
            }
            Mode::HeaderInput => {
                for (key, val) in &self.wizard_headers {
                    let line = Line::from(format!("  {key}: {val}").dim());
                    line.render(areas[row], buf);
                    row += 1;
                }
                let prompt = format!("> {}_", self.input_buffer);
                Line::from(prompt).render(areas[row], buf);
                row += 1;
            }
            _ => {
                let prompt = format!("> {}_", self.input_buffer);
                Line::from(prompt).render(areas[row], buf);
                row += 1;
            }
        }

        // Blank
        row += 1;

        // Footer hint
        Line::from(self.footer_hint().dim()).render(areas[row], buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_event::AppEvent;
    use crossterm::event::KeyEvent;
    use crossterm::event::KeyModifiers;
    use pretty_assertions::assert_eq;
    use tokio::sync::mpsc::unbounded_channel;

    fn make_picker(
        servers: &BTreeMap<String, McpServerConfig>,
    ) -> (
        McpServerPickerView,
        tokio::sync::mpsc::UnboundedReceiver<AppEvent>,
    ) {
        let (tx_raw, rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx_raw);
        let picker = McpServerPickerView::new(servers, tx);
        (picker, rx)
    }

    fn empty_picker() -> (
        McpServerPickerView,
        tokio::sync::mpsc::UnboundedReceiver<AppEvent>,
    ) {
        make_picker(&BTreeMap::new())
    }

    fn picker_with_servers() -> (
        McpServerPickerView,
        tokio::sync::mpsc::UnboundedReceiver<AppEvent>,
    ) {
        let mut servers = BTreeMap::new();
        servers.insert(
            "docs".to_string(),
            McpServerConfig {
                transport: McpServerTransportConfig::Stdio {
                    command: "docs-server".to_string(),
                    args: vec![],
                    env: None,
                    env_vars: vec![],
                    cwd: None,
                },
                enabled: true,
                startup_timeout_sec: None,
                tool_timeout_sec: None,
                enabled_tools: None,
                disabled_tools: None,
            },
        );
        servers.insert(
            "remote-api".to_string(),
            McpServerConfig {
                transport: McpServerTransportConfig::StreamableHttp {
                    url: "https://example.com/mcp".to_string(),
                    bearer_token_env_var: None,
                    http_headers: None,
                    env_http_headers: None,
                },
                enabled: true,
                startup_timeout_sec: None,
                tool_timeout_sec: None,
                enabled_tools: None,
                disabled_tools: None,
            },
        );
        make_picker(&servers)
    }

    fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }

    fn press(picker: &mut McpServerPickerView, code: KeyCode) {
        picker.handle_key_event(key(code, KeyModifiers::NONE));
    }

    fn type_str(picker: &mut McpServerPickerView, s: &str) {
        for c in s.chars() {
            press(picker, KeyCode::Char(c));
        }
    }

    fn last_save_event(
        rx: &mut tokio::sync::mpsc::UnboundedReceiver<AppEvent>,
    ) -> Option<BTreeMap<String, McpServerConfig>> {
        let mut last = None;
        while let Ok(event) = rx.try_recv() {
            if let AppEvent::SaveMcpServers(servers) = event {
                last = Some(servers);
            }
        }
        last
    }

    // --- List mode tests ---

    #[test]
    fn navigation_wraps_around() {
        let (mut picker, _rx) = picker_with_servers();
        // 3 items: Add new, docs, remote-api
        assert_eq!(picker.selected_idx(), 0);

        press(&mut picker, KeyCode::Up);
        assert_eq!(picker.selected_idx(), 2); // wraps to last

        press(&mut picker, KeyCode::Down);
        assert_eq!(picker.selected_idx(), 0); // wraps to first
    }

    #[test]
    fn jk_navigation_works() {
        let (mut picker, _rx) = picker_with_servers();
        press(&mut picker, KeyCode::Char('j'));
        assert_eq!(picker.selected_idx(), 1);
        press(&mut picker, KeyCode::Char('k'));
        assert_eq!(picker.selected_idx(), 0);
    }

    #[test]
    fn toggle_enabled_on_server() {
        let (mut picker, mut rx) = picker_with_servers();

        // Navigate to first server (index 1: "docs")
        press(&mut picker, KeyCode::Down);
        assert_eq!(picker.selected_idx(), 1);

        // Toggle (Enter)
        press(&mut picker, KeyCode::Enter);

        let servers = last_save_event(&mut rx).expect("should have saved");
        let docs = servers.get("docs").expect("docs should exist");
        assert!(!docs.enabled, "docs should now be disabled");
    }

    #[test]
    fn delete_server_with_confirm() {
        let (mut picker, mut rx) = picker_with_servers();

        // Navigate to "docs" (index 1)
        press(&mut picker, KeyCode::Down);

        // Press 'd' to start delete
        press(&mut picker, KeyCode::Char('d'));
        assert_eq!(picker.mode(), &Mode::ConfirmDelete(1));

        // Press 'd' again to confirm
        press(&mut picker, KeyCode::Char('d'));
        assert_eq!(picker.mode(), &Mode::List);

        let servers = last_save_event(&mut rx).expect("should have saved");
        assert!(!servers.contains_key("docs"), "docs should be removed");
        assert!(
            servers.contains_key("remote-api"),
            "remote-api should remain"
        );
    }

    #[test]
    fn delete_cancelled_by_esc() {
        let (mut picker, mut rx) = picker_with_servers();

        press(&mut picker, KeyCode::Down); // -> docs
        press(&mut picker, KeyCode::Char('d')); // start delete
        assert_eq!(picker.mode(), &Mode::ConfirmDelete(1));

        press(&mut picker, KeyCode::Esc); // cancel
        assert_eq!(picker.mode(), &Mode::List);
        assert!(rx.try_recv().is_err(), "no save should have happened");
    }

    #[test]
    fn delete_not_available_on_add_new() {
        let (mut picker, _rx) = picker_with_servers();
        assert_eq!(picker.selected_idx(), 0); // on "Add new..."
        press(&mut picker, KeyCode::Char('d'));
        // Should stay in list mode — 'd' on "Add new" is a no-op
        assert_eq!(picker.mode(), &Mode::List);
    }

    #[test]
    fn esc_closes_picker_in_list_mode() {
        let (mut picker, _rx) = picker_with_servers();
        assert!(!picker.is_complete());
        press(&mut picker, KeyCode::Esc);
        assert!(picker.is_complete());
    }

    // --- Add wizard tests: Stdio flow ---

    #[test]
    fn add_stdio_server_full_flow() {
        let (mut picker, mut rx) = empty_picker();

        // Wizard: Add new -> Stdio -> name -> command -> args -> env -> done
        press(&mut picker, KeyCode::Enter); // Add new -> TransportSelect
        press(&mut picker, KeyCode::Enter); // Stdio -> NameInput
        type_str(&mut picker, "my-server");
        press(&mut picker, KeyCode::Enter); // -> CommandInput
        type_str(&mut picker, "npx");
        press(&mut picker, KeyCode::Enter); // -> ArgsInput
        type_str(&mut picker, "-y @my/mcp-server");
        press(&mut picker, KeyCode::Enter); // -> EnvInput
        type_str(&mut picker, "API_KEY=secret123");
        press(&mut picker, KeyCode::Enter); // add env var
        press(&mut picker, KeyCode::Enter); // empty -> finish (stdio skips headers)

        assert_eq!(picker.mode(), &Mode::List);

        let servers = last_save_event(&mut rx).expect("should have saved");
        let server = servers.get("my-server").expect("my-server should exist");
        assert!(server.enabled);
        match &server.transport {
            McpServerTransportConfig::Stdio {
                command, args, env, ..
            } => {
                assert_eq!(command, "npx");
                assert_eq!(args, &["-y", "@my/mcp-server"]);
                let env = env.as_ref().expect("should have env");
                assert_eq!(env.get("API_KEY"), Some(&"secret123".to_string()));
            }
            _ => panic!("expected Stdio transport"),
        }
    }

    // --- Add wizard tests: HTTP flow ---

    #[test]
    fn add_http_server_full_flow() {
        let (mut picker, mut rx) = empty_picker();

        // Wizard: Add new -> HTTP -> name -> url -> headers -> done
        press(&mut picker, KeyCode::Enter); // Add new -> TransportSelect
        press(&mut picker, KeyCode::Down); // Toggle to HTTP
        press(&mut picker, KeyCode::Enter); // HTTP -> NameInput
        type_str(&mut picker, "notion");
        press(&mut picker, KeyCode::Enter); // -> UrlInput
        type_str(&mut picker, "https://mcp.notion.com/mcp");
        press(&mut picker, KeyCode::Enter); // -> HeaderInput (HTTP skips env)
        type_str(&mut picker, "Authorization: Bearer tok123");
        press(&mut picker, KeyCode::Enter); // add header
        press(&mut picker, KeyCode::Enter); // empty -> finish

        assert_eq!(picker.mode(), &Mode::List);

        let servers = last_save_event(&mut rx).expect("should have saved");
        let server = servers.get("notion").expect("notion should exist");
        assert!(server.enabled);
        match &server.transport {
            McpServerTransportConfig::StreamableHttp {
                url, http_headers, ..
            } => {
                assert_eq!(url, "https://mcp.notion.com/mcp");
                let headers = http_headers.as_ref().expect("should have headers");
                assert_eq!(
                    headers.get("Authorization"),
                    Some(&"Bearer tok123".to_string())
                );
            }
            _ => panic!("expected StreamableHttp transport"),
        }
    }

    // --- Wizard back navigation ---

    #[test]
    fn esc_in_wizard_goes_back_step_by_step() {
        let (mut picker, _rx) = empty_picker();

        // Start wizard
        press(&mut picker, KeyCode::Enter); // -> TransportSelect
        press(&mut picker, KeyCode::Enter); // -> NameInput (stdio)
        type_str(&mut picker, "test");
        press(&mut picker, KeyCode::Enter); // -> CommandInput

        // Esc goes back to NameInput with name pre-filled
        press(&mut picker, KeyCode::Esc);
        assert_eq!(picker.mode(), &Mode::NameInput);
        assert_eq!(picker.input_buffer(), "test");

        // Esc goes back to TransportSelect
        press(&mut picker, KeyCode::Esc);
        assert!(matches!(picker.mode(), Mode::TransportSelect { .. }));

        // Esc goes back to List
        press(&mut picker, KeyCode::Esc);
        assert_eq!(picker.mode(), &Mode::List);
    }

    // --- Empty input rejection ---

    #[test]
    fn empty_name_not_accepted() {
        let (mut picker, mut rx) = empty_picker();
        press(&mut picker, KeyCode::Enter); // -> TransportSelect
        press(&mut picker, KeyCode::Enter); // -> NameInput
        press(&mut picker, KeyCode::Enter); // empty name -> stays
        assert_eq!(picker.mode(), &Mode::NameInput);
        assert!(
            rx.try_recv().is_err(),
            "no save should happen for empty name"
        );
    }

    #[test]
    fn empty_command_not_accepted() {
        let (mut picker, mut rx) = empty_picker();
        press(&mut picker, KeyCode::Enter); // -> TransportSelect
        press(&mut picker, KeyCode::Enter); // -> NameInput
        type_str(&mut picker, "test");
        press(&mut picker, KeyCode::Enter); // -> CommandInput
        press(&mut picker, KeyCode::Enter); // empty command -> stays
        assert_eq!(picker.mode(), &Mode::CommandInput);
        assert!(
            rx.try_recv().is_err(),
            "no save should happen for empty command"
        );
    }

    #[test]
    fn empty_url_not_accepted() {
        let (mut picker, mut rx) = empty_picker();
        press(&mut picker, KeyCode::Enter); // -> TransportSelect
        press(&mut picker, KeyCode::Down); // -> HTTP
        press(&mut picker, KeyCode::Enter); // -> NameInput
        type_str(&mut picker, "test");
        press(&mut picker, KeyCode::Enter); // -> UrlInput
        press(&mut picker, KeyCode::Enter); // empty url -> stays
        assert_eq!(picker.mode(), &Mode::UrlInput);
        assert!(
            rx.try_recv().is_err(),
            "no save should happen for empty url"
        );
    }

    // --- Backspace in text input ---

    #[test]
    fn backspace_removes_characters() {
        let (mut picker, _rx) = empty_picker();
        press(&mut picker, KeyCode::Enter); // -> TransportSelect
        press(&mut picker, KeyCode::Enter); // -> NameInput
        type_str(&mut picker, "abc");
        assert_eq!(picker.input_buffer(), "abc");
        press(&mut picker, KeyCode::Backspace);
        assert_eq!(picker.input_buffer(), "ab");
    }

    // --- Rendering smoke tests ---

    #[test]
    fn renders_list_mode_without_panic() {
        let (picker, _rx) = picker_with_servers();
        let width = 60;
        let height = picker.desired_height(width);
        let area = Rect::new(0, 0, width, height);
        let mut buf = Buffer::empty(area);
        picker.render(area, &mut buf);

        let text: String = (0..area.height)
            .map(|row| {
                (0..area.width)
                    .map(|col| {
                        let s = buf[(col, row)].symbol();
                        if s.is_empty() { " " } else { s }
                    })
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");

        assert!(text.contains("MCP Servers"), "should contain title");
        assert!(text.contains("Add new"), "should contain Add new option");
        assert!(text.contains("docs"), "should contain server name");
    }

    #[test]
    fn renders_transport_select_without_panic() {
        let (mut picker, _rx) = empty_picker();
        press(&mut picker, KeyCode::Enter); // -> TransportSelect

        let width = 60;
        let height = picker.desired_height(width);
        let area = Rect::new(0, 0, width, height);
        let mut buf = Buffer::empty(area);
        picker.render(area, &mut buf);

        let text: String = (0..area.height)
            .map(|row| {
                (0..area.width)
                    .map(|col| {
                        let s = buf[(col, row)].symbol();
                        if s.is_empty() { " " } else { s }
                    })
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");

        assert!(text.contains("Stdio"), "should contain Stdio option");
        assert!(text.contains("HTTP"), "should contain HTTP option");
    }

    #[test]
    fn renders_text_input_without_panic() {
        let (mut picker, _rx) = empty_picker();
        press(&mut picker, KeyCode::Enter); // -> TransportSelect
        press(&mut picker, KeyCode::Enter); // -> NameInput
        type_str(&mut picker, "hello");

        let width = 60;
        let height = picker.desired_height(width);
        let area = Rect::new(0, 0, width, height);
        let mut buf = Buffer::empty(area);
        picker.render(area, &mut buf);

        let text: String = (0..area.height)
            .map(|row| {
                (0..area.width)
                    .map(|col| {
                        let s = buf[(col, row)].symbol();
                        if s.is_empty() { " " } else { s }
                    })
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");

        assert!(text.contains("hello"), "should show typed text");
    }

    // --- Stdio with no args or env ---

    #[test]
    fn add_stdio_server_minimal() {
        let (mut picker, mut rx) = empty_picker();

        press(&mut picker, KeyCode::Enter); // -> TransportSelect
        press(&mut picker, KeyCode::Enter); // -> NameInput (stdio)
        type_str(&mut picker, "simple");
        press(&mut picker, KeyCode::Enter); // -> CommandInput
        type_str(&mut picker, "my-cmd");
        press(&mut picker, KeyCode::Enter); // -> ArgsInput
        press(&mut picker, KeyCode::Enter); // empty args -> EnvInput
        press(&mut picker, KeyCode::Enter); // empty env -> finishes

        assert_eq!(picker.mode(), &Mode::List);
        let servers = last_save_event(&mut rx).expect("should have saved");
        let server = servers.get("simple").expect("simple should exist");
        match &server.transport {
            McpServerTransportConfig::Stdio {
                command, args, env, ..
            } => {
                assert_eq!(command, "my-cmd");
                assert!(args.is_empty());
                assert!(env.is_none());
            }
            _ => panic!("expected Stdio transport"),
        }
    }

    #[test]
    fn duplicate_name_rejected() {
        let (mut picker, mut rx) = picker_with_servers();

        // Try to add a server named "docs" (already exists)
        press(&mut picker, KeyCode::Enter); // -> TransportSelect
        press(&mut picker, KeyCode::Enter); // Stdio -> NameInput
        type_str(&mut picker, "docs");
        press(&mut picker, KeyCode::Enter); // -> CommandInput
        type_str(&mut picker, "some-cmd");
        press(&mut picker, KeyCode::Enter); // -> ArgsInput
        press(&mut picker, KeyCode::Enter); // skip args -> EnvInput
        press(&mut picker, KeyCode::Enter); // skip env -> finish_wizard

        // Should be sent back to NameInput due to duplicate
        assert_eq!(picker.mode(), &Mode::NameInput);
        assert_eq!(picker.input_buffer(), "docs");
        assert!(
            rx.try_recv().is_err(),
            "no save should happen for duplicate name"
        );
    }
}
