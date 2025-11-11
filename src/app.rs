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
    RunInstallation,
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
    pub install_options_state: ListState,
    pub current_stream_token: Option<tokio_util::sync::CancellationToken>,
}

impl Default for Model {
    fn default() -> Self {
        let mut list_state = ListState::default();
        list_state.select(Some(0));

        let mut install_options_state = ListState::default();
        install_options_state.select(Some(0));

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
            install_options_state,
            current_stream_token: None,
        }
    }
}

impl Model {
    /// Returns the number of install options based on whether install_cmd is available
    fn install_option_count(&self) -> usize {
        if self.install_prompt_cmd.is_some() {
            3 // Run Installation, Open Installation Page, Cancel
        } else {
            2 // Open Installation Page, Cancel
        }
    }

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
                }
            }

            Message::SubmitInput => {
                // Capture user message and transition to streaming mode
                let user_text = self.textarea.lines().join("\n");
                if !user_text.trim().is_empty() {
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
                self.textarea = TextArea::default(); // Reset textarea for next input
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
                self.textarea = TextArea::default();
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
                self.install_prompt_cmd = install_cmd.clone();

                // Set initial choice based on whether install_cmd is available
                self.install_prompt_choice = if install_cmd.is_some() {
                    InstallChoice::RunInstallation
                } else {
                    InstallChoice::OpenInstallPage
                };

                self.install_options_state.select(Some(0));
            }

            Message::NavigateInstallChoice => {
                let option_count = self.install_option_count();
                let current = self.install_options_state.selected().unwrap_or(0);
                let next = (current + 1) % option_count;
                self.install_options_state.select(Some(next));

                // Map index to InstallChoice based on option count
                self.install_prompt_choice = if option_count == 3 {
                    match next {
                        0 => InstallChoice::RunInstallation,
                        1 => InstallChoice::OpenInstallPage,
                        _ => InstallChoice::Cancel,
                    }
                } else {
                    match next {
                        0 => InstallChoice::OpenInstallPage,
                        _ => InstallChoice::Cancel,
                    }
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

            Message::Quit => {
                // Handled in main loop
            }
        }
    }
}
