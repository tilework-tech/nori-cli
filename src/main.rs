use color_eyre::Result;
use crossterm::event::{Event, EventStream, KeyCode, KeyEvent, KeyEventKind};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use futures::StreamExt;
use nori_cli::app::{AppMode, Message, Model};
use nori_cli::backends::{AgentBackend, claude::ClaudeBackend, codex::CodexBackend};
use nori_cli::commands::{CommandRegistry, parse_slash_command};
use nori_cli::conversation::render_event;
use nori_cli::ui;
use ratatui::{DefaultTerminal, TerminalOptions, Viewport};
use tokio::sync::mpsc;
use tokio::time::{Duration, interval};

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;

    // Setup terminal with inline viewport (8 lines at bottom for input/instructions)
    enable_raw_mode()?;
    let mut terminal = ratatui::init_with_options(TerminalOptions {
        viewport: Viewport::Inline(8),
    });

    let result = run_app(&mut terminal).await;

    // Restore terminal
    disable_raw_mode()?;
    ratatui::restore();

    result
}

async fn run_app(terminal: &mut DefaultTerminal) -> Result<()> {
    let mut model = Model::default();
    let (tx, mut rx) = mpsc::unbounded_channel::<Message>();

    // Create command registry
    let command_registry = CommandRegistry::default();

    // Create channels for syncing state with event handler
    let (mode_tx, mut mode_rx) = mpsc::unbounded_channel::<AppMode>();
    let (overlay_tx, mut overlay_rx) = mpsc::unbounded_channel::<bool>();

    // Spawn event handling task
    let event_tx = tx.clone();
    tokio::spawn(async move {
        let mut reader = EventStream::new();
        let mut current_mode = AppMode::Selection;
        let mut show_overlay = false;
        loop {
            tokio::select! {
                // Receive mode updates from main loop (single source of truth)
                Some(mode) = mode_rx.recv() => {
                    current_mode = mode;
                }
                // Receive overlay state updates
                Some(overlay) = overlay_rx.recv() => {
                    show_overlay = overlay;
                }
                Some(Ok(event)) = reader.next() => {
                    if let Some(msg) = handle_event_simple(current_mode, show_overlay, event) {
                        let _ = event_tx.send(msg);
                    }
                }
            }
        }
    });

    // Render interval
    let mut render_interval = interval(Duration::from_millis(33)); // ~30 fps

    loop {
        tokio::select! {
            // Handle messages
            Some(msg) = rx.recv() => {
                match &msg {
                    Message::Quit => break,
                    Message::StreamEvent(event) => {
                        // Render event to scrollback using insert_before
                        let line = render_event(event);
                        terminal.insert_before(1, |buf| {
                            use ratatui::widgets::{Paragraph, Widget};
                            Paragraph::new(line.clone()).render(buf.area, buf);
                        })?;

                        model.update(msg);
                        let _ = mode_tx.send(model.current_mode);
                        let _ = overlay_tx.send(model.show_agent_router);
                    }
                    Message::SubmitInput => {
                        // Extract prompt from textarea
                        let prompt = model.textarea.lines().join("\n");
                        if !prompt.trim().is_empty() {
                            // Check if this is a slash command
                            if let Some(command_name) = parse_slash_command(&prompt) {
                                // Execute slash command
                                match command_registry.execute(&command_name, &mut model) {
                                    Ok(()) => {
                                        // Command executed successfully
                                        // Special handling for exit command
                                        if command_name == "exit" {
                                            let _ = tx.send(Message::Quit);
                                        }
                                        // Clear textarea after successful command
                                        model.textarea = tui_textarea::TextArea::default();
                                    }
                                    Err(err) => {
                                        // Command execution failed - show error
                                        let _ = tx.send(Message::Error(format!("{}\nAvailable commands: /exit, /switch-model", err)));
                                    }
                                }
                                // Send updated mode and overlay state to event handler
                                let _ = mode_tx.send(model.current_mode);
                                let _ = overlay_tx.send(model.show_agent_router);
                            } else {
                                // Regular prompt - spawn backend subprocess
                                let backend = get_backend(&model);
                                let stream_tx = tx.clone();
                                model.update(msg);
                                // Send updated mode and overlay state to event handler
                                let _ = mode_tx.send(model.current_mode);
                                let _ = overlay_tx.send(model.show_agent_router);

                                tokio::spawn(async move {
                                    if let Err(e) = spawn_and_stream(backend, prompt, stream_tx).await {
                                        // Error already sent via channel
                                        eprintln!("Streaming error: {}", e);
                                    }
                                });
                            }
                        }
                    }
                    _ => {
                        model.update(msg);
                        // Send updated mode and overlay state to event handler after every state change
                        let _ = mode_tx.send(model.current_mode);
                        let _ = overlay_tx.send(model.show_agent_router);
                    }
                }
            }

            // Render
            _ = render_interval.tick() => {
                terminal.draw(|frame| ui::render(&mut model, frame))?;
            }
        }
    }

    Ok(())
}

fn handle_event_simple(mode: AppMode, show_overlay: bool, event: Event) -> Option<Message> {
    if let Event::Key(key) = event
        && key.kind == KeyEventKind::Press
    {
        return handle_key_simple(mode, show_overlay, key);
    }
    None
}

fn handle_key_simple(mode: AppMode, show_overlay: bool, key: KeyEvent) -> Option<Message> {
    // If overlay is open, handle navigation
    if show_overlay {
        return match key.code {
            KeyCode::Up | KeyCode::Char('k') => Some(Message::PreviousItem),
            KeyCode::Down | KeyCode::Char('j') => Some(Message::NextItem),
            KeyCode::Enter => Some(Message::SelectItem),
            KeyCode::Esc => Some(Message::ExitInputMode),
            _ => None,
        };
    }

    // If streaming, only allow Esc to stop
    if mode == AppMode::Streaming {
        return match key.code {
            KeyCode::Esc => Some(Message::StreamComplete),
            _ => None,
        };
    }

    // Otherwise, handle chat input
    // Check Enter for submit
    if key.code == KeyCode::Enter && key.modifiers.is_empty() {
        return Some(Message::SubmitInput);
    }

    // Send all other key events to textarea
    Some(Message::KeyPress(key))
}

fn get_backend(model: &Model) -> Box<dyn AgentBackend + Send> {
    match model.selected_agent_index {
        Some(0) => Box::new(ClaudeBackend::new()),
        Some(1) => Box::new(CodexBackend::new()),
        _ => Box::new(ClaudeBackend::new()), // Default
    }
}

async fn spawn_and_stream(
    backend: Box<dyn AgentBackend + Send>,
    prompt: String,
    tx: mpsc::UnboundedSender<Message>,
) -> Result<()> {
    // Get stream from backend
    let mut stream = backend.spawn_stream(prompt);

    // Consume stream and send events to UI
    while let Some(event) = stream.next().await {
        let _ = tx.send(Message::StreamEvent(event));
    }

    // Signal completion
    tx.send(Message::StreamComplete)?;

    Ok(())
}
