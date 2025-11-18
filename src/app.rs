#![allow(dead_code)]

use crate::backends;
use crate::backends::AgentBackend;
use crate::conversation::ConversationEvent;
use crate::history::{
    CommittedInlineEntry, InlineEntryId, InlineEntryKind, InlineEntryState, InlineEntryUpdate,
};
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

#[derive(Clone)]
pub struct BackendOption {
    pub name: &'static str,
    pub availability_check: fn() -> bool,
    pub factory: fn() -> Box<dyn AgentBackend + Send>,
}

impl BackendOption {
    pub fn is_available(&self) -> bool {
        (self.availability_check)()
    }

    pub fn create_backend(&self) -> Box<dyn AgentBackend + Send> {
        (self.factory)()
    }
}

pub const BACKEND_OPTIONS: &[BackendOption] = &[
    BackendOption {
        name: "Claude Code ACP",
        availability_check: || {
            backends::is_available(
                backends::claude_code_acp::ClaudeCodeAcpBackend::new().command_name(),
            )
        },
        factory: || Box::new(backends::claude_code_acp::ClaudeCodeAcpBackend::new()),
    },
    BackendOption {
        name: "Codex ACP",
        availability_check: || {
            backends::is_available(backends::codex_acp::CodexAcpBackend::new().command_name())
        },
        factory: || Box::new(backends::codex_acp::CodexAcpBackend::new()),
    },
    BackendOption {
        name: "Gemini ACP",
        availability_check: || {
            backends::is_available(backends::gemini_acp::GeminiAcpBackend::new().command_name())
        },
        factory: || Box::new(backends::gemini_acp::GeminiAcpBackend::new()),
    },
    BackendOption {
        name: "Mock ACP Agent",
        availability_check: || backends::is_available(crate::backends::mock::binary_path()),
        factory: || Box::new(backends::mock::MockBackend::new()),
    },
    BackendOption {
        name: "Claude Code",
        availability_check: || {
            backends::is_available(backends::claude::ClaudeBackend::new().command_name())
        },
        factory: || Box::new(backends::claude::ClaudeBackend::new()),
    },
];

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
    BeginInlineEntry {
        id: InlineEntryId,
        kind: InlineEntryKind,
    },
    UpdateInlineEntry {
        id: InlineEntryId,
        update: InlineEntryUpdate,
    },
    CommitInlineEntry {
        id: InlineEntryId,
    },
    AbortInlineEntry {
        id: InlineEntryId,
    },
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
    pub textarea: TextArea,
    pub response_events: Vec<ConversationEvent>,
    pub inline_entries: Vec<InlineEntryState>,
    pub selected_agent_index: usize,
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
    pub terminal_size: (u16, u16),
    pub current_backend: Option<Box<dyn AgentBackend>>,
    pub current_backend_agent_index: Option<usize>,
}

impl Default for Model {
    fn default() -> Self {
        // Create agent selection list from BACKEND_OPTIONS
        let agent_items: Vec<SelectionItem<String>> = BACKEND_OPTIONS
            .iter()
            .enumerate()
            .map(|(i, backend_option)| {
                let is_available = backend_option.is_available();
                let name = if is_available {
                    backend_option.name.to_string()
                } else {
                    format!("{} [Not Installed]", backend_option.name)
                };
                SelectionItem {
                    data: backend_option.name.to_string(),
                    name,
                    description: Some(if is_available {
                        "Available".to_string()
                    } else {
                        "Not installed on your system".to_string()
                    }),
                    selected_description: None,
                    is_current: i == 0,
                    display_shortcut: None,
                    search_value: Some(backend_option.name.to_lowercase()),
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
            textarea: create_textarea(),
            response_events: Vec::new(),
            inline_entries: Vec::new(),
            selected_agent_index: 0,
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
            terminal_size: (80, 24), // Default terminal size
            current_backend: None,
            current_backend_agent_index: None,
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
                self.selected_agent_index = self.agent_selection_list.selected_index().unwrap_or(0);
                self.show_agent_router = false;
                self.error_message = None;
                self.clear_textarea();
            }

            Message::ExitInputMode => {
                // Close the agent router overlay if open
                self.show_agent_router = false;
                self.clear_textarea();
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
                    self.clear_textarea();
                    self.response_events
                        .push(ConversationEvent::UserMessage { text: user_text });
                    self.current_mode = AppMode::Streaming;
                }
            }

            Message::StreamEvent(event) => {
                self.response_events.push(event);
            }

            Message::BeginInlineEntry { id, kind } => {
                self.begin_inline_entry(id, kind);
            }

            Message::UpdateInlineEntry { id, update } => {
                self.update_inline_entry(&id, update);
            }

            Message::AbortInlineEntry { id } => {
                self.abort_inline_entry(&id);
            }

            Message::CommitInlineEntry { .. } => {
                // Commit is handled in the main loop so the scrollback can be updated there.
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
                    // Re-create agent selection list to reflect new availability
                    let agent_items: Vec<SelectionItem<String>> = BACKEND_OPTIONS
                        .iter()
                        .enumerate()
                        .map(|(i, backend_option)| {
                            let is_available = backend_option.is_available();
                            let name = if is_available {
                                backend_option.name.to_string()
                            } else {
                                format!("{} [Not Installed]", backend_option.name)
                            };
                            SelectionItem {
                                data: backend_option.name.to_string(),
                                name,
                                description: Some(if is_available {
                                    "Available".to_string()
                                } else {
                                    "Not installed on your system".to_string()
                                }),
                                selected_description: None,
                                is_current: i == self.selected_agent_index,
                                display_shortcut: None,
                                search_value: Some(backend_option.name.to_lowercase()),
                            }
                        })
                        .collect();

                    let agent_config = SelectionListConfig::new()
                        .with_title("Agent Router - Select an Agent")
                        .with_footer_hint(standard_popup_hint_line());

                    self.agent_selection_list =
                        SelectionList::new(agent_config, agent_items, Box::new(()));
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
                        self.clear_textarea();
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
                        self.clear_textarea();
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
                self.clear_textarea();
            }

            Message::Quit => {
                // Handled in main loop
            }

            Message::TerminalResize { width, height } => {
                self.terminal_size = (width, height);
                self.rewrap_inline_entries();
            }

            Message::MouseEvent(_mouse_event) => {
                // TODO: Handle mouse events (scrolling, clicking, etc.)
                // For now, just consume the event
            }
        }
    }

    pub fn clear_textarea(&mut self) {
        self.textarea = create_textarea();
    }

    pub fn inline_wrap_width(&self) -> usize {
        self.terminal_size.0.saturating_sub(2).max(1) as usize
    }

    pub fn inline_height(&self) -> u16 {
        self.inline_entries.iter().map(|entry| entry.height()).sum()
    }

    pub fn begin_inline_entry(&mut self, id: InlineEntryId, kind: InlineEntryKind) {
        let mut entry = InlineEntryState::new(id, kind);
        let width = self.inline_wrap_width();
        if width > 0 {
            entry.rewrap(width);
        }
        self.inline_entries.push(entry);
    }

    pub fn update_inline_entry(&mut self, id: &InlineEntryId, update: InlineEntryUpdate) {
        let width = self.inline_wrap_width();
        if let Some(entry) = self.inline_entries.iter_mut().find(|entry| &entry.id == id) {
            entry.apply_update(update, width);
        }
    }

    pub fn commit_inline_entry(&mut self, id: &InlineEntryId) -> Option<CommittedInlineEntry> {
        let index = self
            .inline_entries
            .iter()
            .position(|entry| &entry.id == id)?;
        let entry = self.inline_entries.remove(index);
        Some(entry.into_committed())
    }

    pub fn abort_inline_entry(&mut self, id: &InlineEntryId) -> Option<InlineEntryState> {
        let index = self
            .inline_entries
            .iter()
            .position(|entry| &entry.id == id)?;
        Some(self.inline_entries.remove(index))
    }

    pub fn rewrap_inline_entries(&mut self) {
        let width = self.inline_wrap_width();
        if width == 0 {
            return;
        }
        for entry in &mut self.inline_entries {
            entry.rewrap(width);
        }
    }

    pub fn get_backend(&self) -> Box<dyn AgentBackend + Send> {
        BACKEND_OPTIONS
            .get(self.selected_agent_index)
            .map(|option| option.create_backend())
            .unwrap_or_else(|| Box::new(backends::claude_code_acp::ClaudeCodeAcpBackend::new()))
    }

    /// Ensures a backend exists for the currently selected agent, reusing the existing
    /// backend if the agent hasn't changed, or creating a new one if it has.
    /// This is the preferred method for obtaining a backend for prompt submission.
    pub fn ensure_backend_for_current_agent(&mut self) -> &mut Box<dyn AgentBackend> {
        // Check if we need to create a new backend
        let needs_new_backend = match self.current_backend_agent_index {
            None => true, // No backend exists yet
            Some(cached_index) => cached_index != self.selected_agent_index, // Agent changed
        };

        if needs_new_backend {
            // Drop the old backend (if any) and create a new one
            // Note: Dropping the old backend will trigger its Drop impl,
            // which for ACP runners will kill the subprocess
            self.current_backend = Some(
                BACKEND_OPTIONS
                    .get(self.selected_agent_index)
                    .map(|option| {
                        // Create backend without Send bound for storage
                        // We'll need to modify the factory function signature
                        option.create_backend()
                    })
                    .unwrap_or_else(|| {
                        Box::new(backends::claude_code_acp::ClaudeCodeAcpBackend::new())
                    }),
            );
            self.current_backend_agent_index = Some(self.selected_agent_index);
        }

        // Return a mutable reference to the backend
        self.current_backend
            .as_mut()
            .expect("Backend should exist after ensure")
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
