//! MCP server picker: interactive management UI for MCP server connections.
//!
//! Displays configured MCP servers with the ability to toggle, remove, and add
//! new servers. Follows the same `BottomPaneView` pattern as the hotkey picker.

use std::collections::BTreeMap;
use std::collections::HashMap;

use codex_core::config::types::McpServerConfig;
use codex_core::config::types::McpServerTransportConfig;
use codex_protocol::protocol::McpAuthStatus;
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
    /// Typing the bearer token env var name (http transport only).
    SecretInput,
    /// Typing the OAuth client ID (http transport only, for servers without dynamic registration).
    ClientIdInput,
    /// Typing the OAuth client secret env var name (http transport only).
    ClientSecretEnvVarInput,
    /// OAuth flow in progress for a server.
    OAuthInProgress { server_name: String },
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
    /// Auth status per server name.
    auth_statuses: HashMap<String, McpAuthStatus>,
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
    wizard_bearer_token_env_var: String,
    wizard_client_id: String,
    wizard_client_secret_env_var: String,
    /// When set, the picker will auto-trigger OAuth when auth statuses arrive
    /// with `NotLoggedIn` for this server.
    pending_oauth_server: Option<String>,
}

impl McpServerPickerView {
    pub fn new(servers: &BTreeMap<String, McpServerConfig>, app_event_tx: AppEventSender) -> Self {
        let servers: Vec<(String, McpServerConfig)> = servers
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        Self {
            servers,
            auth_statuses: HashMap::new(),
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
            wizard_bearer_token_env_var: String::new(),
            wizard_client_id: String::new(),
            wizard_client_secret_env_var: String::new(),
            pending_oauth_server: None,
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

    fn handle_list_login(&mut self) {
        let Some(server_idx) = self.server_idx(self.selected_idx) else {
            return;
        };
        let Some((name, config)) = self.servers.get(server_idx) else {
            return;
        };
        // Only allow login on StreamableHttp servers that are NotLoggedIn.
        let auth_status = self
            .auth_statuses
            .get(name.as_str())
            .copied()
            .unwrap_or(McpAuthStatus::Unsupported);
        if auth_status != McpAuthStatus::NotLoggedIn {
            return;
        }
        if let McpServerTransportConfig::StreamableHttp {
            url,
            http_headers,
            env_http_headers,
            client_id,
            client_secret_env_var,
            ..
        } = &config.transport
        {
            let server_name = name.clone();
            self.app_event_tx.send(AppEvent::McpOAuthLogin {
                server_name: server_name.clone(),
                server_url: url.clone(),
                http_headers: http_headers.clone(),
                env_http_headers: env_http_headers.clone(),
                client_id: client_id.clone(),
                client_secret_env_var: client_secret_env_var.clone(),
            });
            self.mode = Mode::OAuthInProgress { server_name };
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
        self.wizard_bearer_token_env_var.clear();
        self.wizard_client_id.clear();
        self.wizard_client_secret_env_var.clear();
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
            // Empty input means "done with headers" — move to SecretInput
            self.mode = Mode::SecretInput;
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

    fn handle_secret_submit(&mut self) {
        let input = self.input_buffer.trim().to_string();
        self.input_buffer.clear();
        self.wizard_bearer_token_env_var = input;
        if self.wizard_bearer_token_env_var.is_empty() {
            // No bearer token — offer client credential inputs for OAuth
            self.mode = Mode::ClientIdInput;
        } else {
            // Bearer token provided — skip client credentials (mutually exclusive)
            self.finish_wizard();
        }
    }

    fn handle_client_id_submit(&mut self) {
        let input = self.input_buffer.trim().to_string();
        self.input_buffer.clear();
        if input.is_empty() {
            // No client ID — skip client secret too, finish wizard
            self.finish_wizard();
        } else {
            self.wizard_client_id = input;
            self.mode = Mode::ClientSecretEnvVarInput;
        }
    }

    fn handle_client_secret_env_var_submit(&mut self) {
        let input = self.input_buffer.trim().to_string();
        self.input_buffer.clear();
        self.wizard_client_secret_env_var = input;
        self.finish_wizard();
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
                let bearer_token_env_var = if self.wizard_bearer_token_env_var.is_empty() {
                    None
                } else {
                    Some(self.wizard_bearer_token_env_var.clone())
                };
                let client_id = if self.wizard_client_id.is_empty() {
                    None
                } else {
                    Some(self.wizard_client_id.clone())
                };
                let client_secret_env_var = if self.wizard_client_secret_env_var.is_empty() {
                    None
                } else {
                    Some(self.wizard_client_secret_env_var.clone())
                };
                McpServerConfig {
                    transport: McpServerTransportConfig::StreamableHttp {
                        url: self.wizard_url.clone(),
                        bearer_token_env_var,
                        http_headers,
                        env_http_headers: None,
                        client_id,
                        client_secret_env_var,
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

        // For HTTP servers without bearer token, set up auto-probe for OAuth
        let is_http_without_bearer = matches!(self.wizard_transport, TransportChoice::Http)
            && self.wizard_bearer_token_env_var.is_empty();

        let server_name = self.wizard_name.clone();
        self.servers.push((server_name.clone(), config));
        self.save_servers();
        self.mode = Mode::List;
        // Select the newly added server (last in list, but list index = servers.len())
        self.selected_idx = self.servers.len(); // "Add new" is 0, so last server is at len

        if is_http_without_bearer {
            self.pending_oauth_server = Some(server_name);
            self.app_event_tx.send(AppEvent::ComputeMcpAuthStatuses);
        }
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
            Mode::SecretInput => {
                self.mode = Mode::HeaderInput;
                self.input_buffer.clear();
            }
            Mode::ClientIdInput => {
                self.mode = Mode::SecretInput;
                self.input_buffer.clear();
            }
            Mode::ClientSecretEnvVarInput => {
                self.mode = Mode::ClientIdInput;
                self.input_buffer = self.wizard_client_id.clone();
            }
            Mode::OAuthInProgress { server_name } => {
                self.app_event_tx.send(AppEvent::McpOAuthLoginCancel {
                    server_name: server_name.clone(),
                });
                self.mode = Mode::List;
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
            Mode::SecretInput | Mode::ClientIdInput | Mode::ClientSecretEnvVarInput => {
                "Add MCP Server"
            }
            Mode::OAuthInProgress { .. } => "MCP Servers",
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
            Mode::SecretInput => "Enter bearer token env var name (or empty to skip)",
            Mode::ClientIdInput => "Enter OAuth client ID (or empty to skip)",
            Mode::ClientSecretEnvVarInput => "Enter client secret env var name (or empty to skip)",
            Mode::OAuthInProgress { .. } => "Authenticating...",
        }
    }

    /// Footer hint for the current mode.
    fn footer_hint(&self) -> &'static str {
        match &self.mode {
            Mode::List => "↑↓ select · enter toggle · l login · d delete · esc close",
            Mode::ConfirmDelete(_) => "d confirm delete · esc cancel",
            Mode::TransportSelect { .. } => "↑↓ select · enter choose · esc back",
            Mode::NameInput
            | Mode::CommandInput
            | Mode::ArgsInput
            | Mode::UrlInput
            | Mode::EnvInput
            | Mode::HeaderInput
            | Mode::SecretInput
            | Mode::ClientIdInput
            | Mode::ClientSecretEnvVarInput => "enter submit · esc back",
            Mode::OAuthInProgress { .. } => "esc cancel",
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

    #[cfg(test)]
    pub(crate) fn pending_oauth_server(&self) -> Option<&str> {
        self.pending_oauth_server.as_deref()
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
                    code: KeyCode::Char('l'),
                    modifiers: KeyModifiers::NONE,
                    ..
                } => self.handle_list_login(),
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
            Mode::SecretInput => match key_event.code {
                KeyCode::Enter => self.handle_secret_submit(),
                KeyCode::Esc => self.go_back(),
                _ => self.handle_text_input(key_event),
            },
            Mode::ClientIdInput => match key_event.code {
                KeyCode::Enter => self.handle_client_id_submit(),
                KeyCode::Esc => self.go_back(),
                _ => self.handle_text_input(key_event),
            },
            Mode::ClientSecretEnvVarInput => match key_event.code {
                KeyCode::Enter => self.handle_client_secret_env_var_submit(),
                KeyCode::Esc => self.go_back(),
                _ => self.handle_text_input(key_event),
            },
            Mode::OAuthInProgress { .. } => {
                if key_event.code == KeyCode::Esc {
                    self.go_back()
                }
            }
        }
    }

    fn is_complete(&self) -> bool {
        self.complete
    }

    fn on_ctrl_c(&mut self) -> CancellationEvent {
        self.complete = true;
        CancellationEvent::Handled
    }

    fn update_mcp_auth_statuses(
        &mut self,
        statuses: &std::collections::HashMap<String, codex_protocol::protocol::McpAuthStatus>,
    ) {
        self.auth_statuses = statuses.clone();

        // Auto-trigger OAuth for a just-added server that requires it
        if let Some(pending_name) = self.pending_oauth_server.take()
            && let Some(status) = statuses.get(&pending_name)
            && *status == McpAuthStatus::NotLoggedIn
        {
            // Find the server config and trigger OAuth
            if let Some((_, config)) = self.servers.iter().find(|(name, _)| *name == pending_name)
                && let McpServerTransportConfig::StreamableHttp {
                    url,
                    http_headers,
                    env_http_headers,
                    client_id,
                    client_secret_env_var,
                    ..
                } = &config.transport
            {
                self.app_event_tx.send(AppEvent::McpOAuthLogin {
                    server_name: pending_name.clone(),
                    server_url: url.clone(),
                    http_headers: http_headers.clone(),
                    env_http_headers: env_http_headers.clone(),
                    client_id: client_id.clone(),
                    client_secret_env_var: client_secret_env_var.clone(),
                });
                self.mode = Mode::OAuthInProgress {
                    server_name: pending_name,
                };
            }
        }
    }

    fn handle_mcp_oauth_complete(&mut self, server_name: &str, _success: bool) {
        if let Mode::OAuthInProgress {
            server_name: ref current,
        } = self.mode
            && current == server_name
        {
            self.mode = Mode::List;
        }
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
            Mode::OAuthInProgress { .. } => {
                // title + subtitle + blank + status line + blank + footer
                3 + 1 + 2
            }
            Mode::NameInput
            | Mode::CommandInput
            | Mode::ArgsInput
            | Mode::UrlInput
            | Mode::EnvInput
            | Mode::HeaderInput
            | Mode::SecretInput
            | Mode::ClientIdInput
            | Mode::ClientSecretEnvVarInput => {
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
            Mode::OAuthInProgress { .. } => {
                constraints.push(Constraint::Length(1)); // status line
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
            Mode::OAuthInProgress { server_name } => {
                let msg = format!("Waiting for browser authentication for `{server_name}`...");
                Line::from(msg.dim()).render(areas[row], buf);
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
    use codex_protocol::protocol::McpAuthStatus;
    use crossterm::event::KeyEvent;
    use crossterm::event::KeyModifiers;
    use pretty_assertions::assert_eq;
    use std::collections::HashMap;
    use tokio::sync::mpsc::unbounded_channel;

    fn make_picker(
        servers: &BTreeMap<String, McpServerConfig>,
    ) -> (
        McpServerPickerView,
        tokio::sync::mpsc::UnboundedReceiver<AppEvent>,
    ) {
        make_picker_with_auth(servers, &HashMap::new())
    }

    fn make_picker_with_auth(
        servers: &BTreeMap<String, McpServerConfig>,
        auth_statuses: &HashMap<String, McpAuthStatus>,
    ) -> (
        McpServerPickerView,
        tokio::sync::mpsc::UnboundedReceiver<AppEvent>,
    ) {
        let (tx_raw, rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx_raw);
        let mut picker = McpServerPickerView::new(servers, tx);
        picker.auth_statuses = auth_statuses.clone();
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
                    client_id: None,
                    client_secret_env_var: None,
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

        // Wizard: Add new -> HTTP -> name -> url -> headers -> secret -> client_id -> done
        press(&mut picker, KeyCode::Enter); // Add new -> TransportSelect
        press(&mut picker, KeyCode::Down); // Toggle to HTTP
        press(&mut picker, KeyCode::Enter); // HTTP -> NameInput
        type_str(&mut picker, "notion");
        press(&mut picker, KeyCode::Enter); // -> UrlInput
        type_str(&mut picker, "https://mcp.notion.com/mcp");
        press(&mut picker, KeyCode::Enter); // -> HeaderInput (HTTP skips env)
        type_str(&mut picker, "Authorization: Bearer tok123");
        press(&mut picker, KeyCode::Enter); // add header
        press(&mut picker, KeyCode::Enter); // empty headers -> SecretInput
        press(&mut picker, KeyCode::Enter); // empty secret -> ClientIdInput
        press(&mut picker, KeyCode::Enter); // empty client_id -> finish

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

    // --- OAuth login tests ---

    fn picker_with_not_logged_in_http_server() -> (
        McpServerPickerView,
        tokio::sync::mpsc::UnboundedReceiver<AppEvent>,
    ) {
        let mut servers = BTreeMap::new();
        servers.insert(
            "slack".to_string(),
            McpServerConfig {
                transport: McpServerTransportConfig::StreamableHttp {
                    url: "https://mcp.slack.com/mcp".to_string(),
                    bearer_token_env_var: None,
                    http_headers: None,
                    env_http_headers: None,
                    client_id: None,
                    client_secret_env_var: None,
                },
                enabled: true,
                startup_timeout_sec: None,
                tool_timeout_sec: None,
                enabled_tools: None,
                disabled_tools: None,
            },
        );
        servers.insert(
            "docs".to_string(),
            McpServerConfig {
                transport: McpServerTransportConfig::Stdio {
                    command: "docs-server".to_string(),
                    args: vec![],
                    env: None,
                    env_vars: vec![],
                },
                enabled: true,
                startup_timeout_sec: None,
                tool_timeout_sec: None,
                enabled_tools: None,
                disabled_tools: None,
            },
        );
        let mut auth_statuses = HashMap::new();
        auth_statuses.insert("slack".to_string(), McpAuthStatus::NotLoggedIn);
        auth_statuses.insert("docs".to_string(), McpAuthStatus::Unsupported);
        make_picker_with_auth(&servers, &auth_statuses)
    }

    #[test]
    fn login_on_not_logged_in_http_server_emits_event() {
        let (mut picker, mut rx) = picker_with_not_logged_in_http_server();

        // Navigate to "slack" (servers sorted: docs=1, slack=2)
        press(&mut picker, KeyCode::Down); // -> docs (index 1)
        press(&mut picker, KeyCode::Down); // -> slack (index 2)
        assert_eq!(picker.selected_idx(), 2);

        // Press 'l' to login
        press(&mut picker, KeyCode::Char('l'));

        // Should emit McpOAuthLogin event
        let event = rx.try_recv().expect("should have emitted an event");
        match event {
            AppEvent::McpOAuthLogin { server_name, .. } => {
                assert_eq!(server_name, "slack");
            }
            other => panic!("expected McpOAuthLogin event, got {other:?}"),
        }
    }

    #[test]
    fn login_on_stdio_server_is_noop() {
        let (mut picker, mut rx) = picker_with_not_logged_in_http_server();

        // Navigate to "docs" (Stdio server, index 1)
        press(&mut picker, KeyCode::Down);
        assert_eq!(picker.selected_idx(), 1);

        // Press 'l' to attempt login
        press(&mut picker, KeyCode::Char('l'));

        // Should not emit any event
        assert!(
            rx.try_recv().is_err(),
            "login on stdio server should be a no-op"
        );
    }

    #[test]
    fn login_on_add_new_is_noop() {
        let (mut picker, mut rx) = picker_with_not_logged_in_http_server();

        // Stay on "Add new..." (index 0)
        assert_eq!(picker.selected_idx(), 0);

        // Press 'l' to attempt login
        press(&mut picker, KeyCode::Char('l'));

        // Should not emit any event
        assert!(
            rx.try_recv().is_err(),
            "login on 'Add new...' should be a no-op"
        );
    }

    #[test]
    fn login_on_bearer_token_server_is_noop() {
        let mut servers = BTreeMap::new();
        servers.insert(
            "slack".to_string(),
            McpServerConfig {
                transport: McpServerTransportConfig::StreamableHttp {
                    url: "https://mcp.slack.com/mcp".to_string(),
                    bearer_token_env_var: Some("SLACK_TOKEN".to_string()),
                    http_headers: None,
                    env_http_headers: None,
                    client_id: None,
                    client_secret_env_var: None,
                },
                enabled: true,
                startup_timeout_sec: None,
                tool_timeout_sec: None,
                enabled_tools: None,
                disabled_tools: None,
            },
        );
        let mut auth_statuses = HashMap::new();
        auth_statuses.insert("slack".to_string(), McpAuthStatus::BearerToken);
        let (mut picker, mut rx) = make_picker_with_auth(&servers, &auth_statuses);

        press(&mut picker, KeyCode::Down);
        press(&mut picker, KeyCode::Char('l'));

        assert!(
            rx.try_recv().is_err(),
            "login on BearerToken server should be a no-op"
        );
    }

    #[test]
    fn login_works_after_auth_statuses_update() {
        // Create picker WITHOUT auth statuses (simulates production behavior).
        let mut servers = BTreeMap::new();
        servers.insert(
            "slack".to_string(),
            McpServerConfig {
                transport: McpServerTransportConfig::StreamableHttp {
                    url: "https://mcp.slack.com/mcp".to_string(),
                    bearer_token_env_var: None,
                    http_headers: None,
                    env_http_headers: None,
                    client_id: None,
                    client_secret_env_var: None,
                },
                enabled: true,
                startup_timeout_sec: None,
                tool_timeout_sec: None,
                enabled_tools: None,
                disabled_tools: None,
            },
        );
        let (mut picker, mut rx) = make_picker(&servers);

        // Navigate to "slack" (index 1)
        press(&mut picker, KeyCode::Down);
        assert_eq!(picker.selected_idx(), 1);

        // Press 'l' — should be a no-op because auth statuses are empty.
        press(&mut picker, KeyCode::Char('l'));
        assert!(
            rx.try_recv().is_err(),
            "login should be a no-op before auth statuses arrive"
        );

        // Simulate auth statuses arriving asynchronously.
        let mut statuses = HashMap::new();
        statuses.insert("slack".to_string(), McpAuthStatus::NotLoggedIn);
        picker.update_mcp_auth_statuses(&statuses);

        // Press 'l' again — should now emit McpOAuthLogin.
        press(&mut picker, KeyCode::Char('l'));
        let event = rx
            .try_recv()
            .expect("should have emitted an event after auth status update");
        match event {
            AppEvent::McpOAuthLogin { server_name, .. } => {
                assert_eq!(server_name, "slack");
            }
            other => panic!("expected McpOAuthLogin event, got {other:?}"),
        }
    }

    #[test]
    fn login_on_already_authenticated_server_is_noop() {
        let mut servers = BTreeMap::new();
        servers.insert(
            "slack".to_string(),
            McpServerConfig {
                transport: McpServerTransportConfig::StreamableHttp {
                    url: "https://mcp.slack.com/mcp".to_string(),
                    bearer_token_env_var: None,
                    http_headers: None,
                    env_http_headers: None,
                    client_id: None,
                    client_secret_env_var: None,
                },
                enabled: true,
                startup_timeout_sec: None,
                tool_timeout_sec: None,
                enabled_tools: None,
                disabled_tools: None,
            },
        );
        let mut auth_statuses = HashMap::new();
        auth_statuses.insert("slack".to_string(), McpAuthStatus::OAuth);
        let (mut picker, mut rx) = make_picker_with_auth(&servers, &auth_statuses);

        // Navigate to "slack" (already authenticated)
        press(&mut picker, KeyCode::Down);

        // Press 'l'
        press(&mut picker, KeyCode::Char('l'));

        // Should not emit login event (already authenticated)
        assert!(
            rx.try_recv().is_err(),
            "login on already authenticated server should be a no-op"
        );
    }

    // --- SecretInput mode tests ---

    #[test]
    fn add_http_server_with_bearer_token_env_var() {
        let (mut picker, mut rx) = empty_picker();

        // Wizard: Add new -> HTTP -> name -> url -> headers (skip) -> secret -> finish
        press(&mut picker, KeyCode::Enter); // -> TransportSelect
        press(&mut picker, KeyCode::Down); // Toggle to HTTP
        press(&mut picker, KeyCode::Enter); // HTTP -> NameInput
        type_str(&mut picker, "slack");
        press(&mut picker, KeyCode::Enter); // -> UrlInput
        type_str(&mut picker, "https://mcp.slack.com/mcp");
        press(&mut picker, KeyCode::Enter); // -> HeaderInput
        press(&mut picker, KeyCode::Enter); // empty -> SecretInput
        assert_eq!(picker.mode(), &Mode::SecretInput);

        type_str(&mut picker, "SLACK_BOT_TOKEN");
        press(&mut picker, KeyCode::Enter); // finish wizard

        assert_eq!(picker.mode(), &Mode::List);

        let servers = last_save_event(&mut rx).expect("should have saved");
        let server = servers.get("slack").expect("slack should exist");
        match &server.transport {
            McpServerTransportConfig::StreamableHttp {
                url,
                bearer_token_env_var,
                ..
            } => {
                assert_eq!(url, "https://mcp.slack.com/mcp");
                assert_eq!(
                    bearer_token_env_var.as_deref(),
                    Some("SLACK_BOT_TOKEN"),
                    "bearer_token_env_var should be set"
                );
            }
            _ => panic!("expected StreamableHttp transport"),
        }
    }

    #[test]
    fn add_http_server_skip_bearer_token() {
        let (mut picker, mut rx) = empty_picker();

        // Wizard: Add new -> HTTP -> name -> url -> headers (skip) -> secret (skip) -> client_id (skip) -> finish
        press(&mut picker, KeyCode::Enter); // -> TransportSelect
        press(&mut picker, KeyCode::Down); // Toggle to HTTP
        press(&mut picker, KeyCode::Enter); // HTTP -> NameInput
        type_str(&mut picker, "notion");
        press(&mut picker, KeyCode::Enter); // -> UrlInput
        type_str(&mut picker, "https://mcp.notion.com/mcp");
        press(&mut picker, KeyCode::Enter); // -> HeaderInput
        press(&mut picker, KeyCode::Enter); // empty -> SecretInput
        press(&mut picker, KeyCode::Enter); // empty -> ClientIdInput
        press(&mut picker, KeyCode::Enter); // empty -> finish wizard

        assert_eq!(picker.mode(), &Mode::List);

        let servers = last_save_event(&mut rx).expect("should have saved");
        let server = servers.get("notion").expect("notion should exist");
        match &server.transport {
            McpServerTransportConfig::StreamableHttp {
                bearer_token_env_var,
                ..
            } => {
                assert_eq!(
                    *bearer_token_env_var, None,
                    "bearer_token_env_var should be None when skipped"
                );
            }
            _ => panic!("expected StreamableHttp transport"),
        }
    }

    #[test]
    fn esc_from_secret_input_goes_back_to_header_input() {
        let (mut picker, _rx) = empty_picker();

        // Navigate to SecretInput
        press(&mut picker, KeyCode::Enter); // -> TransportSelect
        press(&mut picker, KeyCode::Down); // Toggle to HTTP
        press(&mut picker, KeyCode::Enter); // HTTP -> NameInput
        type_str(&mut picker, "test");
        press(&mut picker, KeyCode::Enter); // -> UrlInput
        type_str(&mut picker, "https://example.com");
        press(&mut picker, KeyCode::Enter); // -> HeaderInput
        press(&mut picker, KeyCode::Enter); // empty -> SecretInput
        assert_eq!(picker.mode(), &Mode::SecretInput);

        // Esc should go back to HeaderInput
        press(&mut picker, KeyCode::Esc);
        assert_eq!(picker.mode(), &Mode::HeaderInput);
    }

    // --- Auto-probe and OAuthInProgress tests ---

    #[test]
    fn auto_probe_triggers_oauth_for_not_logged_in_server() {
        let (mut picker, mut rx) = empty_picker();

        // Add HTTP server without bearer token via wizard
        press(&mut picker, KeyCode::Enter); // -> TransportSelect
        press(&mut picker, KeyCode::Down); // Toggle to HTTP
        press(&mut picker, KeyCode::Enter); // HTTP -> NameInput
        type_str(&mut picker, "slack");
        press(&mut picker, KeyCode::Enter); // -> UrlInput
        type_str(&mut picker, "https://mcp.slack.com/mcp");
        press(&mut picker, KeyCode::Enter); // -> HeaderInput
        press(&mut picker, KeyCode::Enter); // -> SecretInput
        press(&mut picker, KeyCode::Enter); // empty secret -> ClientIdInput
        press(&mut picker, KeyCode::Enter); // empty client_id -> finish

        // Drain the SaveMcpServers event
        let _ = last_save_event(&mut rx);

        // pending_oauth_server should be set
        assert_eq!(
            picker.pending_oauth_server(),
            Some("slack"),
            "pending_oauth_server should be set after adding HTTP server without bearer token"
        );

        // Simulate auth statuses arriving with NotLoggedIn
        let mut statuses = HashMap::new();
        statuses.insert("slack".to_string(), McpAuthStatus::NotLoggedIn);
        picker.update_mcp_auth_statuses(&statuses);

        // Should have entered OAuthInProgress mode
        assert_eq!(
            picker.mode(),
            &Mode::OAuthInProgress {
                server_name: "slack".to_string()
            },
            "should enter OAuthInProgress mode for NotLoggedIn server"
        );

        // Should have emitted McpOAuthLogin event
        let mut found_oauth = false;
        while let Ok(event) = rx.try_recv() {
            if matches!(event, AppEvent::McpOAuthLogin { .. }) {
                found_oauth = true;
            }
        }
        assert!(found_oauth, "should emit McpOAuthLogin event");
    }

    #[test]
    fn auto_probe_skips_when_bearer_token_set() {
        let (mut picker, mut rx) = empty_picker();

        // Add HTTP server WITH bearer token via wizard
        press(&mut picker, KeyCode::Enter); // -> TransportSelect
        press(&mut picker, KeyCode::Down); // Toggle to HTTP
        press(&mut picker, KeyCode::Enter); // HTTP -> NameInput
        type_str(&mut picker, "slack");
        press(&mut picker, KeyCode::Enter); // -> UrlInput
        type_str(&mut picker, "https://mcp.slack.com/mcp");
        press(&mut picker, KeyCode::Enter); // -> HeaderInput
        press(&mut picker, KeyCode::Enter); // -> SecretInput
        type_str(&mut picker, "SLACK_BOT_TOKEN");
        press(&mut picker, KeyCode::Enter); // finish

        // Drain the SaveMcpServers event
        let _ = last_save_event(&mut rx);

        // pending_oauth_server should NOT be set when bearer token was provided
        assert_eq!(
            picker.pending_oauth_server(),
            None,
            "pending_oauth_server should NOT be set when bearer token is provided"
        );
    }

    #[test]
    fn auto_probe_skips_on_unsupported() {
        let (mut picker, mut rx) = empty_picker();

        // Add HTTP server without bearer token
        press(&mut picker, KeyCode::Enter); // -> TransportSelect
        press(&mut picker, KeyCode::Down); // Toggle to HTTP
        press(&mut picker, KeyCode::Enter); // HTTP -> NameInput
        type_str(&mut picker, "my-api");
        press(&mut picker, KeyCode::Enter); // -> UrlInput
        type_str(&mut picker, "https://api.example.com/mcp");
        press(&mut picker, KeyCode::Enter); // -> HeaderInput
        press(&mut picker, KeyCode::Enter); // -> SecretInput
        press(&mut picker, KeyCode::Enter); // empty secret -> ClientIdInput
        press(&mut picker, KeyCode::Enter); // empty client_id -> finish

        // Drain the SaveMcpServers event
        let _ = last_save_event(&mut rx);

        // Simulate auth status returning Unsupported
        let mut statuses = HashMap::new();
        statuses.insert("my-api".to_string(), McpAuthStatus::Unsupported);
        picker.update_mcp_auth_statuses(&statuses);

        // Should stay in List mode — no OAuth triggered
        assert_eq!(
            picker.mode(),
            &Mode::List,
            "should stay in List mode when server is Unsupported"
        );

        // No McpOAuthLogin event should have been emitted
        let mut found_oauth = false;
        while let Ok(event) = rx.try_recv() {
            if matches!(event, AppEvent::McpOAuthLogin { .. }) {
                found_oauth = true;
            }
        }
        assert!(
            !found_oauth,
            "should NOT emit McpOAuthLogin for Unsupported server"
        );
    }

    #[test]
    fn oauth_in_progress_esc_cancels_and_returns_to_list() {
        let (mut picker, mut rx) = empty_picker();

        // Add HTTP server and trigger auto-probe -> OAuthInProgress
        press(&mut picker, KeyCode::Enter); // -> TransportSelect
        press(&mut picker, KeyCode::Down); // Toggle to HTTP
        press(&mut picker, KeyCode::Enter); // HTTP -> NameInput
        type_str(&mut picker, "slack");
        press(&mut picker, KeyCode::Enter); // -> UrlInput
        type_str(&mut picker, "https://mcp.slack.com/mcp");
        press(&mut picker, KeyCode::Enter); // -> HeaderInput
        press(&mut picker, KeyCode::Enter); // -> SecretInput
        press(&mut picker, KeyCode::Enter); // empty secret -> ClientIdInput
        press(&mut picker, KeyCode::Enter); // empty client_id -> finish

        // Drain events
        while rx.try_recv().is_ok() {}

        // Simulate auth status NotLoggedIn -> auto-enters OAuthInProgress
        let mut statuses = HashMap::new();
        statuses.insert("slack".to_string(), McpAuthStatus::NotLoggedIn);
        picker.update_mcp_auth_statuses(&statuses);

        // Drain events from auto-trigger
        while rx.try_recv().is_ok() {}

        // Should be in OAuthInProgress — if not, the precondition fails (expected in RED)
        assert_eq!(
            picker.mode(),
            &Mode::OAuthInProgress {
                server_name: "slack".to_string()
            },
            "precondition: should be in OAuthInProgress"
        );

        // Press Esc to cancel
        press(&mut picker, KeyCode::Esc);

        // Should return to List
        assert_eq!(picker.mode(), &Mode::List, "Esc should return to List mode");

        // Should emit McpOAuthLoginCancel event
        let mut found_cancel = false;
        while let Ok(event) = rx.try_recv() {
            if matches!(event, AppEvent::McpOAuthLoginCancel { .. }) {
                found_cancel = true;
            }
        }
        assert!(found_cancel, "should emit McpOAuthLoginCancel event on Esc");
    }

    // --- Client credential input tests ---

    #[test]
    fn add_http_server_with_client_credentials() {
        let (mut picker, mut rx) = empty_picker();

        // Wizard: Add new -> HTTP -> name -> url -> headers (skip) -> secret (skip) -> client_id -> client_secret_env_var -> done
        press(&mut picker, KeyCode::Enter); // -> TransportSelect
        press(&mut picker, KeyCode::Down); // Toggle to HTTP
        press(&mut picker, KeyCode::Enter); // HTTP -> NameInput
        type_str(&mut picker, "slack");
        press(&mut picker, KeyCode::Enter); // -> UrlInput
        type_str(&mut picker, "https://mcp.slack.com/mcp");
        press(&mut picker, KeyCode::Enter); // -> HeaderInput
        press(&mut picker, KeyCode::Enter); // empty -> SecretInput
        press(&mut picker, KeyCode::Enter); // empty secret -> ClientIdInput
        assert_eq!(picker.mode(), &Mode::ClientIdInput);

        type_str(&mut picker, "12345.67890");
        press(&mut picker, KeyCode::Enter); // -> ClientSecretEnvVarInput
        assert_eq!(picker.mode(), &Mode::ClientSecretEnvVarInput);

        type_str(&mut picker, "SLACK_CLIENT_SECRET");
        press(&mut picker, KeyCode::Enter); // finish wizard

        assert_eq!(picker.mode(), &Mode::List);

        let servers = last_save_event(&mut rx).expect("should have saved");
        let server = servers.get("slack").expect("slack should exist");
        match &server.transport {
            McpServerTransportConfig::StreamableHttp {
                url,
                client_id,
                client_secret_env_var,
                bearer_token_env_var,
                ..
            } => {
                assert_eq!(url, "https://mcp.slack.com/mcp");
                assert_eq!(client_id.as_deref(), Some("12345.67890"));
                assert_eq!(
                    client_secret_env_var.as_deref(),
                    Some("SLACK_CLIENT_SECRET")
                );
                assert_eq!(*bearer_token_env_var, None);
            }
            _ => panic!("expected StreamableHttp transport"),
        }
    }

    #[test]
    fn add_http_server_skip_client_credentials() {
        let (mut picker, mut rx) = empty_picker();

        // Wizard: Add new -> HTTP -> name -> url -> headers (skip) -> secret (skip) -> client_id (skip) -> done
        press(&mut picker, KeyCode::Enter); // -> TransportSelect
        press(&mut picker, KeyCode::Down); // Toggle to HTTP
        press(&mut picker, KeyCode::Enter); // HTTP -> NameInput
        type_str(&mut picker, "notion");
        press(&mut picker, KeyCode::Enter); // -> UrlInput
        type_str(&mut picker, "https://mcp.notion.com/mcp");
        press(&mut picker, KeyCode::Enter); // -> HeaderInput
        press(&mut picker, KeyCode::Enter); // empty -> SecretInput
        press(&mut picker, KeyCode::Enter); // empty secret -> ClientIdInput
        press(&mut picker, KeyCode::Enter); // empty client_id -> finish wizard (skips client secret)

        assert_eq!(picker.mode(), &Mode::List);

        let servers = last_save_event(&mut rx).expect("should have saved");
        let server = servers.get("notion").expect("notion should exist");
        match &server.transport {
            McpServerTransportConfig::StreamableHttp {
                client_id,
                client_secret_env_var,
                ..
            } => {
                assert_eq!(*client_id, None, "client_id should be None when skipped");
                assert_eq!(
                    *client_secret_env_var, None,
                    "client_secret_env_var should be None when skipped"
                );
            }
            _ => panic!("expected StreamableHttp transport"),
        }
    }

    #[test]
    fn add_http_server_with_client_id_only_no_secret() {
        let (mut picker, mut rx) = empty_picker();

        // Public OAuth client - has client_id but no secret
        press(&mut picker, KeyCode::Enter); // -> TransportSelect
        press(&mut picker, KeyCode::Down); // Toggle to HTTP
        press(&mut picker, KeyCode::Enter); // HTTP -> NameInput
        type_str(&mut picker, "my-app");
        press(&mut picker, KeyCode::Enter); // -> UrlInput
        type_str(&mut picker, "https://api.example.com/mcp");
        press(&mut picker, KeyCode::Enter); // -> HeaderInput
        press(&mut picker, KeyCode::Enter); // empty -> SecretInput
        press(&mut picker, KeyCode::Enter); // empty secret -> ClientIdInput
        type_str(&mut picker, "public-client-123");
        press(&mut picker, KeyCode::Enter); // -> ClientSecretEnvVarInput
        press(&mut picker, KeyCode::Enter); // empty secret -> finish wizard

        assert_eq!(picker.mode(), &Mode::List);

        let servers = last_save_event(&mut rx).expect("should have saved");
        let server = servers.get("my-app").expect("my-app should exist");
        match &server.transport {
            McpServerTransportConfig::StreamableHttp {
                client_id,
                client_secret_env_var,
                ..
            } => {
                assert_eq!(client_id.as_deref(), Some("public-client-123"));
                assert_eq!(*client_secret_env_var, None, "no secret for public client");
            }
            _ => panic!("expected StreamableHttp transport"),
        }
    }

    #[test]
    fn bearer_token_skips_client_credential_inputs() {
        let (mut picker, mut rx) = empty_picker();

        // When bearer token is provided, client credential steps should be skipped
        press(&mut picker, KeyCode::Enter); // -> TransportSelect
        press(&mut picker, KeyCode::Down); // Toggle to HTTP
        press(&mut picker, KeyCode::Enter); // HTTP -> NameInput
        type_str(&mut picker, "github");
        press(&mut picker, KeyCode::Enter); // -> UrlInput
        type_str(&mut picker, "https://api.github.com/mcp");
        press(&mut picker, KeyCode::Enter); // -> HeaderInput
        press(&mut picker, KeyCode::Enter); // empty -> SecretInput
        type_str(&mut picker, "GITHUB_TOKEN");
        press(&mut picker, KeyCode::Enter); // non-empty secret -> finish wizard (skip client creds)

        // Should go straight to List mode, NOT ClientIdInput
        assert_eq!(picker.mode(), &Mode::List);

        let servers = last_save_event(&mut rx).expect("should have saved");
        let server = servers.get("github").expect("github should exist");
        match &server.transport {
            McpServerTransportConfig::StreamableHttp {
                bearer_token_env_var,
                client_id,
                client_secret_env_var,
                ..
            } => {
                assert_eq!(bearer_token_env_var.as_deref(), Some("GITHUB_TOKEN"));
                assert_eq!(
                    *client_id, None,
                    "client_id should be None when bearer token is set"
                );
                assert_eq!(
                    *client_secret_env_var, None,
                    "client_secret_env_var should be None when bearer token is set"
                );
            }
            _ => panic!("expected StreamableHttp transport"),
        }
    }

    #[test]
    fn esc_from_client_id_input_goes_back_to_secret_input() {
        let (mut picker, _rx) = empty_picker();

        press(&mut picker, KeyCode::Enter); // -> TransportSelect
        press(&mut picker, KeyCode::Down); // Toggle to HTTP
        press(&mut picker, KeyCode::Enter); // HTTP -> NameInput
        type_str(&mut picker, "test");
        press(&mut picker, KeyCode::Enter); // -> UrlInput
        type_str(&mut picker, "https://example.com");
        press(&mut picker, KeyCode::Enter); // -> HeaderInput
        press(&mut picker, KeyCode::Enter); // empty -> SecretInput
        press(&mut picker, KeyCode::Enter); // empty -> ClientIdInput
        assert_eq!(picker.mode(), &Mode::ClientIdInput);

        press(&mut picker, KeyCode::Esc);
        assert_eq!(picker.mode(), &Mode::SecretInput);
    }

    #[test]
    fn esc_from_client_secret_env_var_input_goes_back_to_client_id_input() {
        let (mut picker, _rx) = empty_picker();

        press(&mut picker, KeyCode::Enter); // -> TransportSelect
        press(&mut picker, KeyCode::Down); // Toggle to HTTP
        press(&mut picker, KeyCode::Enter); // HTTP -> NameInput
        type_str(&mut picker, "test");
        press(&mut picker, KeyCode::Enter); // -> UrlInput
        type_str(&mut picker, "https://example.com");
        press(&mut picker, KeyCode::Enter); // -> HeaderInput
        press(&mut picker, KeyCode::Enter); // empty -> SecretInput
        press(&mut picker, KeyCode::Enter); // empty -> ClientIdInput
        type_str(&mut picker, "my-client-id");
        press(&mut picker, KeyCode::Enter); // -> ClientSecretEnvVarInput
        assert_eq!(picker.mode(), &Mode::ClientSecretEnvVarInput);

        press(&mut picker, KeyCode::Esc);
        assert_eq!(picker.mode(), &Mode::ClientIdInput);
        assert_eq!(picker.input_buffer(), "my-client-id");
    }

    #[test]
    fn oauth_login_event_includes_client_credentials() {
        let mut servers = BTreeMap::new();
        servers.insert(
            "slack".to_string(),
            McpServerConfig {
                transport: McpServerTransportConfig::StreamableHttp {
                    url: "https://mcp.slack.com/mcp".to_string(),
                    bearer_token_env_var: None,
                    http_headers: None,
                    env_http_headers: None,
                    client_id: Some("12345.67890".to_string()),
                    client_secret_env_var: Some("SLACK_CLIENT_SECRET".to_string()),
                },
                enabled: true,
                startup_timeout_sec: None,
                tool_timeout_sec: None,
                enabled_tools: None,
                disabled_tools: None,
            },
        );
        let mut auth_statuses = HashMap::new();
        auth_statuses.insert("slack".to_string(), McpAuthStatus::NotLoggedIn);
        let (mut picker, mut rx) = make_picker_with_auth(&servers, &auth_statuses);

        // Navigate to "slack" (index 1)
        press(&mut picker, KeyCode::Down);
        press(&mut picker, KeyCode::Char('l'));

        let event = rx.try_recv().expect("should have emitted an event");
        match event {
            AppEvent::McpOAuthLogin {
                server_name,
                client_id,
                client_secret_env_var,
                ..
            } => {
                assert_eq!(server_name, "slack");
                assert_eq!(client_id, Some("12345.67890".to_string()));
                assert_eq!(
                    client_secret_env_var,
                    Some("SLACK_CLIENT_SECRET".to_string())
                );
            }
            other => panic!("expected McpOAuthLogin event, got {other:?}"),
        }
    }

    #[test]
    fn oauth_complete_success_returns_to_list() {
        let (mut picker, mut rx) = empty_picker();

        // Add HTTP server and trigger auto-probe -> OAuthInProgress
        press(&mut picker, KeyCode::Enter); // -> TransportSelect
        press(&mut picker, KeyCode::Down); // Toggle to HTTP
        press(&mut picker, KeyCode::Enter); // HTTP -> NameInput
        type_str(&mut picker, "slack");
        press(&mut picker, KeyCode::Enter); // -> UrlInput
        type_str(&mut picker, "https://mcp.slack.com/mcp");
        press(&mut picker, KeyCode::Enter); // -> HeaderInput
        press(&mut picker, KeyCode::Enter); // -> SecretInput
        press(&mut picker, KeyCode::Enter); // empty secret -> ClientIdInput
        press(&mut picker, KeyCode::Enter); // empty client_id -> finish

        // Drain events
        while rx.try_recv().is_ok() {}

        // Simulate auth status NotLoggedIn -> OAuthInProgress
        let mut statuses = HashMap::new();
        statuses.insert("slack".to_string(), McpAuthStatus::NotLoggedIn);
        picker.update_mcp_auth_statuses(&statuses);

        // Simulate OAuth completion
        picker.handle_mcp_oauth_complete("slack", true);

        // Should return to List mode
        assert_eq!(
            picker.mode(),
            &Mode::List,
            "should return to List mode on OAuth success"
        );
    }
}
