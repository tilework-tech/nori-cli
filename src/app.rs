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

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum InstallChoice {
    OpenInstallPage,
    Cancel,
}

impl Default for InstallChoice {
    fn default() -> Self {
        Self::OpenInstallPage
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

    // Streaming
    StreamEvent(ConversationEvent),
    StreamComplete,

    // Error handling
    Error(String),

    // Install prompt
    ShowInstallPrompt { backend: String, url: String },
    NavigateInstallChoice,
    ConfirmInstall,
    CancelInstall,

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
    pub install_prompt_choice: InstallChoice,
}

impl Default for Model {
    fn default() -> Self {
        use crate::backends;

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
            install_prompt_choice: InstallChoice::default(),
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

            Message::Error(error) => {
                self.error_message = Some(error);
                // Stay in Streaming mode so user can see the stderr output
                // They can press Esc to go back to Selection
            }

            Message::ToggleAgentRouter => {
                self.show_agent_router = !self.show_agent_router;
            }

            Message::ShowInstallPrompt { backend, url } => {
                self.show_install_prompt = true;
                self.install_prompt_backend = Some(backend);
                self.install_prompt_url = Some(url);
                self.install_prompt_choice = InstallChoice::default();
            }

            Message::NavigateInstallChoice => {
                self.install_prompt_choice = match self.install_prompt_choice {
                    InstallChoice::OpenInstallPage => InstallChoice::Cancel,
                    InstallChoice::Cancel => InstallChoice::OpenInstallPage,
                };
            }

            Message::ConfirmInstall => {
                if let InstallChoice::OpenInstallPage = self.install_prompt_choice {
                    if let Some(ref url) = self.install_prompt_url {
                        // Attempt to open the URL in browser
                        let _ = opener::open(url);
                    }
                }
                // Close the prompt either way
                self.show_install_prompt = false;
                self.install_prompt_backend = None;
                self.install_prompt_url = None;
            }

            Message::CancelInstall => {
                self.show_install_prompt = false;
                self.install_prompt_backend = None;
                self.install_prompt_url = None;
            }

            Message::Quit => {
                // Handled in main loop
            }
        }
    }
}
