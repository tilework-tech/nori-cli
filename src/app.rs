#![allow(dead_code)]

use crate::backends;
use crate::backends::AgentBackend;
use crate::conversation::ConversationEvent;
use tui_components::selection::{
    SelectionItem, SelectionList, SelectionListConfig, standard_popup_hint_line,
};
use tui_components::textarea::{TextArea, TextAreaConfig};

#[derive(Debug, Default, PartialEq, Clone, Copy)]
pub enum AppMode {
    #[default]
    Selection,
    Input,
    Streaming,
}

#[derive(Debug, Default, PartialEq, Eq, Clone, Copy)]
pub enum InstallChoice {
    #[default]
    RunInstallation,
    OpenInstallPage,
    Cancel,
}

impl InstallChoice {
    pub fn next(&self, has_install_cmd: bool) -> Self {
        use InstallChoice::*;
        match (self, has_install_cmd) {
            (RunInstallation, _) => OpenInstallPage,
            (OpenInstallPage, _) => Cancel,
            (Cancel, true) => RunInstallation,
            (Cancel, false) => OpenInstallPage,
        }
    }

    pub fn previous(&self, has_install_cmd: bool) -> Self {
        use InstallChoice::*;
        match (self, has_install_cmd) {
            (RunInstallation, _) => Cancel,
            (OpenInstallPage, true) => RunInstallation,
            (OpenInstallPage, false) => Cancel,
            (Cancel, _) => OpenInstallPage,
        }
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    // Navigation
    NextItem,
    PreviousItem,
    SelectItem,

    // Mode switching
    ExitInputMode,
    ToggleAgentRouter,

    // Input handling
    KeyPress(crossterm::event::KeyEvent),
    SubmitInput,
    InputChanged,

    // Autocomplete
    AutocompleteDown,
    AutocompleteUp,
    AutocompleteSelect,
    CloseAutocomplete,

    // Streaming
    StreamEvent(ConversationEvent),
    StreamComplete,
    CancelStream,

    // Error handling
    Error(String),

    // Install prompt
    ShowInstallPrompt {
        backend: String,
        url: String,
        install_cmd: Option<Vec<String>>,
    },
    NavigateInstallChoice,
    NavigateInstallChoiceNext,
    NavigateInstallChoicePrevious,
    ConfirmInstall,
    CancelInstall,
    InstallationComplete {
        success: bool,
        message: String,
    },

    // Control
    ClearTextarea,
    Quit,

    // Terminal events
    TerminalResize {
        width: u16,
        height: u16,
    },
    MouseEvent(crossterm::event::MouseEvent),
}

pub struct Model {
    pub current_mode: AppMode,
    pub agent_selection_list: SelectionList<String>,
    pub agents: Vec<String>,
    pub backend_availability: Vec<bool>,
    pub textarea: TextArea,
    pub response_events: Vec<ConversationEvent>,
    pub selected_agent_index: Option<usize>,
    pub session_id: Option<String>,
    pub error_message: Option<String>,
    pub show_agent_router: bool,
    pub show_install_prompt: bool,
    pub install_prompt_backend: Option<String>,
    pub install_prompt_url: Option<String>,
    pub install_prompt_cmd: Option<Vec<String>>,
    pub install_prompt_choice: InstallChoice,
    pub current_stream_token: Option<tokio_util::sync::CancellationToken>,
    pub last_ctrl_c_time: Option<std::time::Instant>,
    pub autocomplete_selection_list: SelectionList<String>,
    pub show_autocomplete: bool,
    pub show_debug_events: bool,
    pub use_codex_components: bool,
    pub loading_frame: usize,
    pub terminal_size: (u16, u16),
}

impl Default for Model {
    fn default() -> Self {
        let agents = vec![
            "Claude Code".to_string(),
            "Codex ACP".to_string(),
            "Claude Code ACP".to_string(),
            "Mock ACP Agent".to_string(),
        ];

        let backend_availability = vec![
            backends::is_available("claude"),
            backends::is_available(backends::codex_acp::CodexAcpBackend::new().command_name()),
            backends::is_available(
                backends::claude_code_acp::ClaudeCodeAcpBackend::new().command_name(),
            ),
            backends::is_available(crate::backends::mock::binary_path()),
        ];

        // Create agent selection list
        let agent_items: Vec<SelectionItem<String>> = agents
            .iter()
            .enumerate()
            .map(|(i, agent)| {
                let is_available = backend_availability[i];
                let name = if is_available {
                    agent.clone()
                } else {
                    format!("{agent} [Not Installed]")
                };
                SelectionItem {
                    data: agent.clone(),
                    name,
                    description: Some(if is_available {
                        "Available".to_string()
                    } else {
                        "Not installed on your system".to_string()
                    }),
                    selected_description: None,
                    is_current: i == 0,
                    display_shortcut: None,
                    search_value: Some(agent.to_lowercase()),
                }
            })
            .collect();

        let agent_config = SelectionListConfig::new()
            .with_title("Agent Router - Select an Agent")
            .with_footer_hint(standard_popup_hint_line());

        let agent_selection_list = SelectionList::new(agent_config, agent_items, Box::new(()));

        // Create empty autocomplete selection list (will be populated dynamically)
        let autocomplete_config = SelectionListConfig::new()
            .with_title("Commands")
            .with_footer_hint(standard_popup_hint_line());

        let autocomplete_selection_list =
            SelectionList::new(autocomplete_config, vec![], Box::new(()));

        Self {
            current_mode: AppMode::Selection,
            agent_selection_list,
            agents,
            backend_availability,
            textarea: create_textarea(),
            response_events: Vec::new(),
            selected_agent_index: None,
            session_id: None,
            error_message: None,
            show_agent_router: false,
            show_install_prompt: false,
            install_prompt_backend: None,
            install_prompt_url: None,
            install_prompt_cmd: None,
            install_prompt_choice: InstallChoice::default(),
            current_stream_token: None,
            last_ctrl_c_time: None,
            autocomplete_selection_list,
            show_autocomplete: false,
            show_debug_events: false,
            use_codex_components: true,
            loading_frame: 0,
            terminal_size: (80, 24), // Default terminal size
        }
    }
}

impl Model {
    pub fn update(&mut self, msg: Message) {
        match msg {
            Message::NextItem => {
                // Only navigate when agent router overlay is open
                if self.show_agent_router {
                    self.agent_selection_list.move_down();
                }
            }

            Message::PreviousItem => {
                // Only navigate when agent router overlay is open
                if self.show_agent_router {
                    self.agent_selection_list.move_up();
                }
            }

            Message::SelectItem => {
                // Select agent and close overlay
                self.selected_agent_index = self.agent_selection_list.selected_index();
                self.show_agent_router = false;
                self.error_message = None;
                self.textarea = create_textarea();
            }

            Message::ExitInputMode => {
                // Close the agent router overlay if open
                self.show_agent_router = false;
                self.textarea = create_textarea();
            }

            Message::KeyPress(key) => {
                // Only handle text input when overlay is NOT open
                if !self.show_agent_router {
                    self.textarea.handle_key(key);
                    // Clear Ctrl-C timer when user types (resets the double-press window)
                    self.last_ctrl_c_time = None;
                    // Clear any error/hint messages when user starts typing
                    self.error_message = None;
                }
            }

            Message::SubmitInput => {
                // Capture user message and transition to streaming mode
                let user_text = self.textarea.text().to_string();
                if !user_text.trim().is_empty() {
                    // Clear textarea immediately after capturing text
                    self.textarea = create_textarea();
                    self.response_events
                        .push(ConversationEvent::UserMessage { text: user_text });
                    self.current_mode = AppMode::Streaming;
                }
            }

            Message::StreamEvent(event) => {
                self.response_events.push(event);
            }

            Message::StreamComplete => {
                self.current_mode = AppMode::Selection;
                self.error_message = None; // Clear any errors when going back
                // Note: Textarea already cleared in SubmitInput
                // Note: We do NOT clear response_events to preserve chat history
            }

            Message::CancelStream => {
                // Cancel the current stream if one exists
                if let Some(token) = self.current_stream_token.take() {
                    token.cancel();
                }
                // Transition to Selection mode
                self.current_mode = AppMode::Selection;
                self.error_message = None;
                // Note: Textarea already cleared in SubmitInput
                // Add cancellation status message to history
                self.response_events.push(ConversationEvent::StatusMessage {
                    text: "Interrupted".to_string(),
                });
            }

            Message::Error(error) => {
                self.error_message = Some(error);
                // Stay in Streaming mode so user can see the stderr output
                // They can press Esc to go back to Selection
            }

            Message::ToggleAgentRouter => {
                self.show_agent_router = !self.show_agent_router;
            }

            Message::ShowInstallPrompt {
                backend,
                url,
                install_cmd,
            } => {
                self.show_install_prompt = true;
                self.install_prompt_backend = Some(backend);
                self.install_prompt_url = Some(url);
                let has_install_cmd = install_cmd.as_ref().map(|v| !v.is_empty()).unwrap_or(false);
                self.install_prompt_cmd = install_cmd;
                self.install_prompt_choice = if has_install_cmd {
                    InstallChoice::RunInstallation
                } else {
                    InstallChoice::OpenInstallPage
                };
            }

            Message::NavigateInstallChoice => {
                self.install_prompt_choice = match self.install_prompt_choice {
                    InstallChoice::RunInstallation => InstallChoice::OpenInstallPage,
                    InstallChoice::OpenInstallPage => InstallChoice::Cancel,
                    InstallChoice::Cancel => InstallChoice::RunInstallation,
                };
            }

            Message::NavigateInstallChoiceNext => {
                let has_install_cmd = self
                    .install_prompt_cmd
                    .as_ref()
                    .map(|v| !v.is_empty())
                    .unwrap_or(false);
                self.install_prompt_choice = self.install_prompt_choice.next(has_install_cmd);
            }

            Message::NavigateInstallChoicePrevious => {
                let has_install_cmd = self
                    .install_prompt_cmd
                    .as_ref()
                    .map(|v| !v.is_empty())
                    .unwrap_or(false);
                self.install_prompt_choice = self.install_prompt_choice.previous(has_install_cmd);
            }

            Message::ConfirmInstall => {
                // Don't close prompt here - it will be closed after installation completes
                // This message just triggers the installation process
            }

            Message::CancelInstall => {
                self.show_install_prompt = false;
                self.install_prompt_backend = None;
                self.install_prompt_url = None;
                self.install_prompt_cmd = None;
            }

            Message::InstallationComplete { success, message } => {
                self.show_install_prompt = false;
                self.install_prompt_backend = None;
                self.install_prompt_url = None;
                self.install_prompt_cmd = None;

                if success {
                    // Re-check backend availability
                    self.backend_availability = vec![
                        backends::is_available("claude"),
                        backends::is_available(
                            backends::codex_acp::CodexAcpBackend::new().command_name(),
                        ),
                        backends::is_available(
                            backends::claude_code_acp::ClaudeCodeAcpBackend::new().command_name(),
                        ),
                        backends::is_available(crate::backends::mock::binary_path()),
                    ];
                } else {
                    self.error_message = Some(message);
                }
            }

            Message::ClearTextarea => {
                const CTRL_C_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);
                let now = std::time::Instant::now();

                match self.last_ctrl_c_time {
                    None => {
                        // First Ctrl-C: clear textarea and show hint
                        self.textarea = create_textarea();
                        self.last_ctrl_c_time = Some(now);
                        self.error_message = Some("Press Ctrl-C again to exit".to_string());
                    }
                    Some(last_time) if now.duration_since(last_time) < CTRL_C_TIMEOUT => {
                        // Second Ctrl-C within timeout: clear timestamp to signal quit
                        self.last_ctrl_c_time = None;
                        self.error_message = None;
                    }
                    Some(_) => {
                        // Ctrl-C after timeout expired: treat as first press
                        self.textarea = create_textarea();
                        self.last_ctrl_c_time = Some(now);
                        self.error_message = Some("Press Ctrl-C again to exit".to_string());
                    }
                }
            }

            Message::InputChanged => {
                // Autocomplete state will be updated in main.rs after textarea changes
            }

            Message::AutocompleteDown => {
                self.autocomplete_selection_list.move_down();
            }

            Message::AutocompleteUp => {
                self.autocomplete_selection_list.move_up();
            }

            Message::AutocompleteSelect => {
                if let Some(item) = self.autocomplete_selection_list.selected_item() {
                    // Replace textarea content with selected command
                    self.textarea = {
                        let mut textarea = create_textarea();
                        let text = format!("/{}", item.data);
                        textarea.set_text(&text);
                        textarea.set_cursor(text.len());
                        textarea
                    };
                }
                self.show_autocomplete = false;
                // Clear the autocomplete list
                let config = SelectionListConfig::new()
                    .with_title("Commands")
                    .with_footer_hint(standard_popup_hint_line());
                self.autocomplete_selection_list = SelectionList::new(config, vec![], Box::new(()));
            }

            Message::CloseAutocomplete => {
                self.show_autocomplete = false;
                // Clear the autocomplete list
                let config = SelectionListConfig::new()
                    .with_title("Commands")
                    .with_footer_hint(standard_popup_hint_line());
                self.autocomplete_selection_list = SelectionList::new(config, vec![], Box::new(()));
                // self.autocomplete_filtered_commands.clear();
                self.textarea = create_textarea();
            }

            Message::Quit => {
                // Handled in main loop
            }

            Message::TerminalResize { width, height } => {
                self.terminal_size = (width, height);
            }

            Message::MouseEvent(_mouse_event) => {
                // TODO: Handle mouse events (scrolling, clicking, etc.)
                // For now, just consume the event
            }
        }
    }
}

fn create_textarea() -> TextArea {
    use ratatui::style::{Color, Style};

    // Variation 1: Default - Light gray background, › prefix, balanced padding
    let config = TextAreaConfig::default()
        .with_background_style(Style::default().bg(Color::DarkGray))
        .with_padding(1, 1, 0, 0)
        .with_prefix("›", Style::default())
        .with_placeholder("Write a message...");

    TextArea::new(config)
}
