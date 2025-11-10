use ratatui::widgets::ListState;
use tui_textarea::TextArea;

#[derive(Debug, Default, PartialEq, Clone, Copy)]
pub enum AppMode {
    #[default]
    Selection,
    Input,
    Streaming,
}

#[derive(Debug, Clone)]
pub enum Message {
    // Navigation
    NextItem,
    PreviousItem,
    SelectItem,

    // Mode switching
    ExitInputMode,

    // Input handling
    KeyPress(crossterm::event::KeyEvent),
    SubmitInput,

    // Streaming
    StreamChunk(String),
    StreamComplete,

    // Error handling
    Error(String),

    // Control
    Quit,
}

#[derive(Debug)]
pub struct Model {
    pub current_mode: AppMode,
    pub list_state: ListState,
    pub agents: Vec<String>,
    pub textarea: TextArea<'static>,
    pub response_text: Vec<String>,
    pub selected_agent_index: Option<usize>,
    pub session_id: Option<String>,
    pub error_message: Option<String>,
}

impl Default for Model {
    fn default() -> Self {
        let mut list_state = ListState::default();
        list_state.select(Some(0));

        Self {
            current_mode: AppMode::Selection,
            list_state,
            agents: vec!["Claude Code".to_string(), "GPT Codex".to_string()],
            textarea: TextArea::default(),
            response_text: Vec::new(),
            selected_agent_index: None,
            session_id: None,
            error_message: None,
        }
    }
}

impl Model {
    pub fn update(&mut self, msg: Message) {
        match msg {
            Message::NextItem => {
                if self.current_mode == AppMode::Selection {
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
                if self.current_mode == AppMode::Selection {
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
                if self.current_mode == AppMode::Selection {
                    self.selected_agent_index = self.list_state.selected();
                    self.current_mode = AppMode::Input;
                    self.textarea = TextArea::default();
                    self.error_message = None;
                }
            }

            Message::ExitInputMode => {
                if self.current_mode == AppMode::Input {
                    self.current_mode = AppMode::Selection;
                }
            }

            Message::KeyPress(key) => {
                if self.current_mode == AppMode::Input {
                    self.textarea.input(key);
                }
            }

            Message::SubmitInput => {
                if self.current_mode == AppMode::Input {
                    self.current_mode = AppMode::Streaming;
                    // Don't clear response_text - we want to append
                }
            }

            Message::StreamChunk(text) => {
                self.response_text.push(text);
            }

            Message::StreamComplete => {
                self.current_mode = AppMode::Selection;
                self.error_message = None; // Clear any errors when going back
            }

            Message::Error(error) => {
                self.error_message = Some(error);
                // Stay in Streaming mode so user can see the stderr output
                // They can press Esc to go back to Selection
            }

            Message::Quit => {
                // Handled in main loop
            }
        }
    }
}
