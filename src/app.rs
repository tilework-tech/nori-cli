use crate::backends;
use crate::conversation::ConversationEvent;
use ratatui::widgets::ListState;
use tui_textarea::TextArea;

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
    OpenInstallPage,
    Cancel,
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
    ConfirmInstall,
    CancelInstall,
    InstallationComplete {
        success: bool,
        message: String,
    },

    // Control
    ClearTextarea,
    Quit,
}

#[derive(Debug)]
pub struct Model {
    pub current_mode: AppMode,
    pub list_state: ListState,
    pub agents: Vec<String>,
    pub backend_availability: Vec<bool>,
    pub textarea: TextArea<'static>,
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
    pub show_autocomplete: bool,
    pub autocomplete_filtered_commands: Vec<String>,
    pub autocomplete_selected_index: usize,
}

impl Default for Model {
    fn default() -> Self {
        let mut list_state = ListState::default();
        list_state.select(Some(0));

        Self {
            current_mode: AppMode::Selection,
            list_state,
            agents: vec!["Claude Code".to_string(), "GPT Codex".to_string()],
            backend_availability: vec![
                backends::is_available("claude"),
                backends::is_available("codex"),
            ],
            textarea: TextArea::default(),
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
            show_autocomplete: false,
            autocomplete_filtered_commands: Vec::new(),
            autocomplete_selected_index: 0,
        }
    }
}

impl Model {
    pub fn update(&mut self, msg: Message) {
        match msg {
            Message::NextItem => {
                // Only navigate when agent router overlay is open
                if self.show_agent_router {
                    let i = match self.list_state.selected() {
                        Some(i) => {
                            if i >= self.agents.len() - 1 {
                                0
                            } else {
                                i + 1
                            }
                        }
                        None => 0,
                    };
                    self.list_state.select(Some(i));
                }
            }

            Message::PreviousItem => {
                // Only navigate when agent router overlay is open
                if self.show_agent_router {
                    let i = match self.list_state.selected() {
                        Some(i) => {
                            if i == 0 {
                                self.agents.len() - 1
                            } else {
                                i - 1
                            }
                        }
                        None => 0,
                    };
                    self.list_state.select(Some(i));
                }
            }

            Message::SelectItem => {
                // Select agent and close overlay
                self.selected_agent_index = self.list_state.selected();
                self.show_agent_router = false;
                self.error_message = None;
            }

            Message::ExitInputMode => {
                // Close the agent router overlay if open
                self.show_agent_router = false;
            }

            Message::KeyPress(key) => {
                // Only handle text input when overlay is NOT open
                if !self.show_agent_router {
                    self.textarea.input(key);
                    // Clear Ctrl-C timer when user types (resets the double-press window)
                    self.last_ctrl_c_time = None;
                    // Clear any error/hint messages when user starts typing
                    self.error_message = None;
                }
            }

            Message::SubmitInput => {
                // Capture user message and transition to streaming mode
                let user_text = self.textarea.lines().join("\n");
                if !user_text.trim().is_empty() {
                    // Clear textarea immediately after capturing text
                    self.textarea = TextArea::default();
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
                // Add cancellation event to history
                self.response_events
                    .push(ConversationEvent::StreamCancelled);
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
                self.install_prompt_cmd = install_cmd;
                self.install_prompt_choice = InstallChoice::default();
            }

            Message::NavigateInstallChoice => {
                self.install_prompt_choice = match self.install_prompt_choice {
                    InstallChoice::OpenInstallPage => InstallChoice::Cancel,
                    InstallChoice::Cancel => InstallChoice::OpenInstallPage,
                };
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
                        backends::is_available("codex"),
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
                        self.textarea = TextArea::default();
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
                        self.textarea = TextArea::default();
                        self.last_ctrl_c_time = Some(now);
                        self.error_message = Some("Press Ctrl-C again to exit".to_string());
                    }
                }
            }

            Message::InputChanged => {
                // Autocomplete state will be updated in main.rs after textarea changes
            }

            Message::AutocompleteDown => {
                if !self.autocomplete_filtered_commands.is_empty() {
                    self.autocomplete_selected_index = (self.autocomplete_selected_index + 1)
                        % self.autocomplete_filtered_commands.len();
                }
            }

            Message::AutocompleteUp => {
                if !self.autocomplete_filtered_commands.is_empty() {
                    let len = self.autocomplete_filtered_commands.len();
                    self.autocomplete_selected_index = if self.autocomplete_selected_index == 0 {
                        len - 1
                    } else {
                        self.autocomplete_selected_index - 1
                    };
                }
            }

            Message::AutocompleteSelect => {
                if let Some(selected_cmd) = self
                    .autocomplete_filtered_commands
                    .get(self.autocomplete_selected_index)
                {
                    // Replace textarea content with selected command
                    self.textarea = TextArea::from([format!("/{}", selected_cmd)]);
                    self.textarea.move_cursor(tui_textarea::CursorMove::End);
                }
                self.show_autocomplete = false;
                self.autocomplete_filtered_commands.clear();
            }

            Message::CloseAutocomplete => {
                self.show_autocomplete = false;
                self.autocomplete_filtered_commands.clear();
            }

            Message::Quit => {
                // Handled in main loop
            }
        }
    }
}
