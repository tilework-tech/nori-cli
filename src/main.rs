use color_eyre::Result;
use crossterm::event::{Event, EventStream, KeyCode, KeyEvent, KeyEventKind};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use crossterm::{cursor::MoveTo, execute};
use futures::StreamExt;
use nori_cli::app::{AppMode, InstallChoice, Message, Model};
use nori_cli::backends::{self, AgentBackend, claude::ClaudeBackend, codex::CodexBackend};
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
    // Get cursor position and move to bottom of viewport before restoring
    let cursor_pos = terminal.get_cursor_position()?;
    // Move cursor to the line after the viewport bottom
    execute!(std::io::stdout(), MoveTo(0, cursor_pos.y + 1))?;
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
    let (install_prompt_tx, mut install_prompt_rx) = mpsc::unbounded_channel::<bool>();

    // Spawn event handling task
    let event_tx = tx.clone();
    tokio::spawn(async move {
        let mut reader = EventStream::new();
        let mut current_mode = AppMode::Selection;
        let mut show_overlay = false;
        let mut show_install_prompt = false;
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
                // Receive install prompt state updates
                Some(install_prompt) = install_prompt_rx.recv() => {
                    show_install_prompt = install_prompt;
                }
                Some(Ok(event)) = reader.next() => {
                    if let Some(msg) = handle_event_simple(current_mode, show_overlay, show_install_prompt, event) {
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
                        let _ = install_prompt_tx.send(model.show_install_prompt);
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
                                let _ = install_prompt_tx.send(model.show_install_prompt);
                            } else {
                                // Regular prompt - check if backend is available before spawning
                                let backend = get_backend(&model);
                                if !backends::is_available(backend.command_name()) {
                                    // Backend not available - show install prompt
                                    let _ = tx.send(Message::ShowInstallPrompt {
                                        backend: backend.name().to_string(),
                                        url: backend.install_url().to_string(),
                                        install_cmd: backend.install_command(),
                                    });
                                } else {
                                    // Backend available - spawn subprocess
                                    // Create cancellation token for this stream
                                    let cancel_token = tokio_util::sync::CancellationToken::new();
                                    model.current_stream_token = Some(cancel_token.clone());

                                    let stream_tx = tx.clone();
                                    model.update(msg);
                                    // Send updated mode and overlay state to event handler
                                    let _ = mode_tx.send(model.current_mode);
                                    let _ = overlay_tx.send(model.show_agent_router);
                                    let _ = install_prompt_tx.send(model.show_install_prompt);

                                    tokio::spawn(async move {
                                        if let Err(e) = spawn_and_stream(backend, prompt, stream_tx, cancel_token).await {
                                            // Error already sent via channel
                                            eprintln!("Streaming error: {}", e);
                                        }
                                    });
                                }
                            }
                        }
                    }
                    Message::ConfirmInstall => {
                        // Run installation if install command is available
                        if let (Some(install_cmd), install_choice) = (
                            &model.install_prompt_cmd,
                            model.install_prompt_choice
                        ) {
                            if install_choice == InstallChoice::OpenInstallPage {
                                if !install_cmd.is_empty() {
                                    // Run the installation command
                                    let cmd = install_cmd.clone();
                                    let install_tx = tx.clone();
                                    tokio::spawn(async move {
                                        let result = tokio::process::Command::new(&cmd[0])
                                            .args(&cmd[1..])
                                            .output()
                                            .await;

                                        match result {
                                            Ok(output) if output.status.success() => {
                                                let _ = install_tx.send(Message::InstallationComplete {
                                                    success: true,
                                                    message: "Installation completed successfully".to_string(),
                                                });
                                            }
                                            Ok(output) => {
                                                let stderr = String::from_utf8_lossy(&output.stderr);
                                                let _ = install_tx.send(Message::InstallationComplete {
                                                    success: false,
                                                    message: format!("Installation failed: {}", stderr),
                                                });
                                            }
                                            Err(e) => {
                                                let _ = install_tx.send(Message::InstallationComplete {
                                                    success: false,
                                                    message: format!("Failed to run installation: {}", e),
                                                });
                                            }
                                        }
                                    });
                                } else if let Some(url) = &model.install_prompt_url {
                                    // Fallback to opening URL if no install command
                                    let _ = opener::open(url);
                                    model.update(msg);
                                }
                            } else {
                                model.update(msg);
                            }
                        } else {
                            model.update(msg);
                        }

                        let _ = mode_tx.send(model.current_mode);
                        let _ = overlay_tx.send(model.show_agent_router);
                        let _ = install_prompt_tx.send(model.show_install_prompt);
                    }
                    Message::CancelStream => {
                        model.update(msg);
                        let _ = mode_tx.send(model.current_mode);
                        let _ = overlay_tx.send(model.show_agent_router);
                        let _ = install_prompt_tx.send(model.show_install_prompt);
                    }
                    _ => {
                        model.update(msg);
                        // Send updated mode and overlay state to event handler after every state change
                        let _ = mode_tx.send(model.current_mode);
                        let _ = overlay_tx.send(model.show_agent_router);
                        let _ = install_prompt_tx.send(model.show_install_prompt);
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

fn handle_event_simple(
    mode: AppMode,
    show_overlay: bool,
    show_install_prompt: bool,
    event: Event,
) -> Option<Message> {
    if let Event::Key(key) = event
        && key.kind == KeyEventKind::Press
    {
        return handle_key_simple(mode, show_overlay, show_install_prompt, key);
    }
    None
}

fn handle_key_simple(
    mode: AppMode,
    show_overlay: bool,
    show_install_prompt: bool,
    key: KeyEvent,
) -> Option<Message> {
    // Install prompt takes highest precedence
    if show_install_prompt {
        return match key.code {
            KeyCode::Up | KeyCode::Char('k') => Some(Message::NavigateInstallChoice),
            KeyCode::Down | KeyCode::Char('j') => Some(Message::NavigateInstallChoice),
            KeyCode::Enter => Some(Message::ConfirmInstall),
            KeyCode::Esc => Some(Message::CancelInstall),
            _ => None,
        };
    }

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

    // If streaming, only allow Esc to cancel
    if mode == AppMode::Streaming {
        return match key.code {
            KeyCode::Esc => Some(Message::CancelStream),
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
    cancel_token: tokio_util::sync::CancellationToken,
) -> Result<()> {
    // Get stream from backend
    let mut stream = backend.spawn_stream(prompt, cancel_token.clone());

    // Consume stream and send events to UI
    loop {
        tokio::select! {
            // Cancel branch - takes priority
            _ = cancel_token.cancelled() => {
                // Stream will be dropped here, cleaning up handles
                // Note: Child process killing happens via Drop in the stream
                break;
            }
            // Stream consumption
            Some(event) = stream.next() => {
                let _ = tx.send(Message::StreamEvent(event));
            }
            // Stream ended naturally
            else => {
                tx.send(Message::StreamComplete)?;
                break;
            }
        }
    }

    Ok(())
}
