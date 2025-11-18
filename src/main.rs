mod acp_runner;
mod app;
mod autocomplete;
mod backends;
mod cli;
mod commands;
mod conversation;
mod history;
mod text_utils;
mod ui;

use crate::app::{AppMode, InstallChoice, Message, Model};
use crate::autocomplete::update_autocomplete_state;
use crate::backends::BackendEvent;
use crate::cli::{Cli, agent_name_to_index, valid_agent_names};
use crate::commands::{CommandRegistry, parse_slash_command};
use crate::conversation::{ConversationEvent, render_event, should_render_event};
use crate::text_utils::wrap_text_to_width;

use _tuicore::{TerminalWriter, TuiApp};
use clap::Parser;
use color_eyre::Result;
use futures::{Stream, StreamExt};
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind};
use ratatui::prelude::CrosstermBackend;
use std::io::{self, IsTerminal, Read};
use std::pin::Pin;
use std::thread;
use tokio::sync::mpsc;
use tokio::time::{Duration, interval};

#[tokio::main]
async fn main() -> Result<()> {
    // Parse CLI arguments
    let cli = Cli::parse();

    // Validate agent name if provided
    let agent_index = if let Some(ref agent_name) = cli.agent {
        match agent_name_to_index(agent_name) {
            Some(index) => Some(index),
            None => {
                eprintln!("Error: Invalid agent name '{agent_name}'");
                eprintln!("Valid agents: {}", valid_agent_names().join(", "));
                std::process::exit(1);
            }
        }
    } else {
        None
    };

    // Read from stdin if available (piped input)
    let stdin_message = if !io::stdin().is_terminal() {
        let mut buffer = String::new();
        io::stdin().read_to_string(&mut buffer)?;
        if !buffer.trim().is_empty() {
            Some(buffer.trim().to_string())
        } else {
            None
        }
    } else {
        None
    };

    // Determine initial message (CLI arg takes precedence over stdin)
    let initial_message = cli.message.or(stdin_message);

    let mut tui_app = TuiApp::builder("nori-cli")
        .inline(20)
        .use_disk_logs(true)
        .build();

    let mut terminal = tui_app.init()?;
    run_app(&mut terminal, agent_index, initial_message).await?;

    tui_app.restore()?;
    // Optional terminal.insert_before here, for an exit status/usage message!
    Ok(())
}

async fn run_app(
    terminal: &mut ratatui::Terminal<CrosstermBackend<TerminalWriter>>,
    agent_index: Option<usize>,
    initial_message: Option<String>,
) -> Result<()> {
    let mut model = Model::default();
    if let Ok(size) = terminal.size() {
        model.terminal_size = (size.width, size.height);
    }

    // Set agent index if provided via CLI
    if let Some(index) = agent_index {
        model.selected_agent_index = index;
    }

    // Pre-fill textarea with initial message if provided
    if let Some(message) = initial_message {
        model.textarea.set_text(&message);
    }

    let (tx, mut rx) = mpsc::unbounded_channel::<Message>();

    // Create command registry
    let command_registry = CommandRegistry::default();

    // Create channels for syncing state with event handler
    let (mode_tx, mut mode_rx) = mpsc::unbounded_channel::<AppMode>();
    let (overlay_tx, mut overlay_rx) = mpsc::unbounded_channel::<bool>();
    let (install_prompt_tx, mut install_prompt_rx) = mpsc::unbounded_channel::<bool>();
    let (ctrl_c_tx, mut ctrl_c_rx) = mpsc::unbounded_channel::<Option<std::time::Instant>>();
    let (autocomplete_tx, mut autocomplete_rx) = mpsc::unbounded_channel::<bool>();

    // Spawn event handling task
    let event_tx = tx.clone();
    let (event_thread_tx, mut event_thread_rx) = mpsc::unbounded_channel::<Event>();
    thread::spawn(move || {
        loop {
            if let Ok(true) = event::poll(Duration::from_millis(10))
                && let Ok(event) = event::read()
            {
                let _ = event_thread_tx.send(event);
            }
        }
    });
    tokio::spawn(async move {
        let mut current_mode = AppMode::Selection;
        let mut show_overlay = false;
        let mut show_install_prompt = false;
        let mut last_ctrl_c_time: Option<std::time::Instant> = None;
        let mut show_autocomplete = false;
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
                // Receive ctrl-c timestamp updates
                Some(time) = ctrl_c_rx.recv() => {
                    last_ctrl_c_time = time;
                }
                // Receive autocomplete state updates
                Some(autocomplete) = autocomplete_rx.recv() => {
                    show_autocomplete = autocomplete;
                }
                Some(event) = event_thread_rx.recv() => {
                    if let Some(msg) = handle_event_simple(current_mode, show_overlay, show_install_prompt, show_autocomplete, last_ctrl_c_time, event) {
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
                        // Only render if event should be visible based on debug mode
                        if should_render_event(event, model.show_debug_events) {
                            // Render event to scrollback using insert_before
                            let line = render_event(event);

                            // Get terminal width for wrapping (full width, insert_before handles its own area)
                            let width = terminal.size()?.width.saturating_sub(2) as usize; // Account for potential borders

                            // Convert Line to wrapped lines based on terminal width
                            use ratatui::text::Text;
                            let text = Text::from(line.clone());
                            let wrapped_lines = wrap_text_to_width(&text, width);

                            // Insert each wrapped line separately (insert_before only handles one line at a time)
                            for wrapped_line in wrapped_lines {
                                terminal.insert_before(1, |buf| {
                                    use ratatui::widgets::{Paragraph, Widget};
                                    Paragraph::new(wrapped_line.clone()).render(buf.area, buf);
                                })?;
                            }
                        }

                        model.update(msg);
                        let _ = mode_tx.send(model.current_mode);
                        let _ = overlay_tx.send(model.show_agent_router);
                        let _ = install_prompt_tx.send(model.show_install_prompt);
                        let _ = ctrl_c_tx.send(model.last_ctrl_c_time);
                        let _ = autocomplete_tx.send(model.show_autocomplete);
                    }
                    Message::CommitInlineEntry { id } => {
                        if let Some(committed) = model.commit_inline_entry(id) {
                            let lines = committed.lines.clone();
                            let height = committed.height;
                            terminal.insert_before(height, move |buf| {
                                use ratatui::widgets::{Paragraph, Widget};
                                use ratatui::text::Text;
                                Paragraph::new(Text::from(lines.clone())).render(buf.area, buf);
                            })?;
                            model.response_events.push(committed.event);
                        }

                        let _ = mode_tx.send(model.current_mode);
                        let _ = overlay_tx.send(model.show_agent_router);
                        let _ = install_prompt_tx.send(model.show_install_prompt);
                        let _ = ctrl_c_tx.send(model.last_ctrl_c_time);
                        let _ = autocomplete_tx.send(model.show_autocomplete);
                    }
                    Message::ClearTextarea => {
                        let was_set = model.last_ctrl_c_time.is_some();
                        model.update(msg);
                        let is_now_none = model.last_ctrl_c_time.is_none();

                        // If timestamp went from Some to None, second Ctrl-C occurred
                        if was_set && is_now_none {
                            let _ = tx.send(Message::Quit);
                        }

                        let _ = mode_tx.send(model.current_mode);
                        let _ = overlay_tx.send(model.show_agent_router);
                        let _ = install_prompt_tx.send(model.show_install_prompt);
                        let _ = ctrl_c_tx.send(model.last_ctrl_c_time);
                    }
                    Message::SubmitInput => {
                        // Extract prompt from textarea
                        let prompt = model.textarea.text().to_string();
                        if !prompt.trim().is_empty() {
                            // Check if this is a slash command
                            if let Some(command_name) = parse_slash_command(&prompt) {
                                // Execute slash command
                                let events_before = model.response_events.len();
                                match command_registry.execute(&command_name, &mut model) {
                                    Ok(()) => {
                                        // Command executed successfully
                                        // Special handling for exit command
                                        if command_name == "exit" {
                                            let _ = tx.send(Message::Quit);
                                        }

                                        // Render any StatusMessage events that were added
                                        for event in &model.response_events[events_before..] {
                                            if matches!(event, ConversationEvent::StatusMessage { .. }) {
                                                let line = render_event(event);
                                                let width = terminal.size()?.width.saturating_sub(2) as usize;
                                                use ratatui::text::Text;
                                                let text = Text::from(line.clone());
                                                let wrapped_lines = wrap_text_to_width(&text, width);
                                                for wrapped_line in wrapped_lines {
                                                    terminal.insert_before(1, |buf| {
                                                        use ratatui::widgets::{Paragraph, Widget};
                                                        Paragraph::new(wrapped_line.clone()).render(buf.area, buf);
                                                    })?;
                                                }
                                            }
                                        }

                                        // Clear textarea after successful command
                                        model.clear_textarea();
                                    }
                                    Err(err) => {
                                        // Command execution failed - show error
                                        let _ = tx.send(Message::Error(format!("{err}\nAvailable commands: /exit, /switch-model")));
                                    }
                                }
                                // Send updated mode and overlay state to event handler
                                let _ = mode_tx.send(model.current_mode);
                                let _ = overlay_tx.send(model.show_agent_router);
                                let _ = install_prompt_tx.send(model.show_install_prompt);
                                let _ = ctrl_c_tx.send(model.last_ctrl_c_time);
                            } else {
                                // Regular prompt - render user message to scrollback first
                                let user_event = ConversationEvent::UserMessage {
                                    text: prompt.clone(),
                                };
                                let line = render_event(&user_event);
                                let width = terminal.size()?.width.saturating_sub(2) as usize;

                                use ratatui::text::Text;
                                let text = Text::from(line.clone());
                                let wrapped_lines = wrap_text_to_width(&text, width);

                                for wrapped_line in wrapped_lines {
                                    terminal.insert_before(1, |buf| {
                                        use ratatui::widgets::{Paragraph, Widget};
                                        Paragraph::new(wrapped_line.clone()).render(buf.area, buf);
                                    })?;
                                }

                                // Ensure we have a backend for the current agent
                                // This will reuse the existing backend if the agent hasn't changed,
                                // or create a new one (dropping the old one) if it has
                                let backend = model.ensure_backend_for_current_agent();

                                // Check availability first
                                let backend_name = backend.name().to_string();
                                let backend_url = backend.install_url().to_string();
                                let backend_install_cmd = backend.install_command();
                                let backend_command_name = backend.command_name().to_string();

                                if !backends::is_available(&backend_command_name) {
                                    // Backend not available - show install prompt
                                    let _ = tx.send(Message::ShowInstallPrompt {
                                        backend: backend_name,
                                        url: backend_url,
                                        install_cmd: backend_install_cmd,
                                    });
                                } else {
                                    // Backend available - spawn stream
                                    // Create cancellation token for this stream
                                    let cancel_token = tokio_util::sync::CancellationToken::new();

                                    // Get the stream from the backend
                                    // The backend remains in Model and persists across prompts
                                    let stream = {
                                        let backend = model.ensure_backend_for_current_agent();
                                        backend.spawn_stream(prompt.clone(), cancel_token.clone())
                                    }; // backend reference dropped here

                                    model.current_stream_token = Some(cancel_token.clone());

                                    let stream_tx = tx.clone();
                                    model.update(msg);
                                    // Send updated mode and overlay state to event handler
                                    let _ = mode_tx.send(model.current_mode);
                                    let _ = overlay_tx.send(model.show_agent_router);
                                    let _ = install_prompt_tx.send(model.show_install_prompt);
                                    let _ = ctrl_c_tx.send(model.last_ctrl_c_time);

                                    tokio::spawn(async move {
                                        if let Err(e) = spawn_and_stream(stream, stream_tx, cancel_token).await {
                                            // Error already sent via channel
                                            eprintln!("Streaming error: {e}");
                                        }
                                    });
                                }
                            }
                        }
                    }
                    Message::ConfirmInstall => {
                        // Handle install confirmation based on selected choice
                        match model.install_prompt_choice {
                            InstallChoice::RunInstallation => {
                                // Run the installation command
                                if let Some(install_cmd) = &model.install_prompt_cmd
                                    && !install_cmd.is_empty() {
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
                                                        message: format!("Installation failed: {stderr}"),
                                                    });
                                                }
                                                Err(e) => {
                                                    let _ = install_tx.send(Message::InstallationComplete {
                                                        success: false,
                                                        message: format!("Failed to run installation: {e}"),
                                                    });
                                                }
                                            }
                                        });
                                    }
                            }
                            InstallChoice::OpenInstallPage => {
                                // Open the installation page in browser
                                if let Some(url) = &model.install_prompt_url {
                                    let _ = opener::open(url);
                                }
                                model.update(msg);
                            }
                            InstallChoice::Cancel => {
                                // Cancel is handled by CancelInstall message
                                model.update(msg);
                            }
                        }

                        let _ = mode_tx.send(model.current_mode);
                        let _ = overlay_tx.send(model.show_agent_router);
                        let _ = install_prompt_tx.send(model.show_install_prompt);
                        let _ = ctrl_c_tx.send(model.last_ctrl_c_time);
                    }
                    Message::CancelStream => {
                        let events_before = model.response_events.len();
                        model.update(msg);

                        // Render the StatusMessage that was added by the cancel
                        for event in &model.response_events[events_before..] {
                            if matches!(event, ConversationEvent::StatusMessage { .. }) {
                                let line = render_event(event);
                                let width = terminal.size()?.width.saturating_sub(2) as usize;
                                use ratatui::text::Text;
                                let text = Text::from(line.clone());
                                let wrapped_lines = wrap_text_to_width(&text, width);
                                for wrapped_line in wrapped_lines {
                                    terminal.insert_before(1, |buf| {
                                        use ratatui::widgets::{Paragraph, Widget};
                                        Paragraph::new(wrapped_line.clone()).render(buf.area, buf);
                                    })?;
                                }
                            }
                        }

                        let _ = mode_tx.send(model.current_mode);
                        let _ = overlay_tx.send(model.show_agent_router);
                        let _ = install_prompt_tx.send(model.show_install_prompt);
                        let _ = ctrl_c_tx.send(model.last_ctrl_c_time);
                        let _ = autocomplete_tx.send(model.show_autocomplete);
                    }
                    Message::KeyPress(_) => {
                        // Update model (which updates textarea)
                        model.update(msg);
                        // Update autocomplete state based on new textarea content
                        let input = model.textarea.text().to_string();
                        update_autocomplete_state(&mut model, &input, &command_registry);
                        // Send state updates
                        let _ = mode_tx.send(model.current_mode);
                        let _ = overlay_tx.send(model.show_agent_router);
                        let _ = install_prompt_tx.send(model.show_install_prompt);
                        let _ = ctrl_c_tx.send(model.last_ctrl_c_time);
                        let _ = autocomplete_tx.send(model.show_autocomplete);
                    }
                    Message::InputChanged => {
                        // Explicitly update autocomplete state (called after other updates)
                        let input = model.textarea.text().to_string();
                        update_autocomplete_state(&mut model, &input, &command_registry);
                        let _ = autocomplete_tx.send(model.show_autocomplete);
                    }
                    Message::TerminalResize { width: _w, height: _h } => {
                        terminal.autoresize()?;
                        model.update(msg);
                        // Send updated mode and overlay state to event handler
                        let _ = mode_tx.send(model.current_mode);
                        let _ = overlay_tx.send(model.show_agent_router);
                        let _ = install_prompt_tx.send(model.show_install_prompt);
                        let _ = ctrl_c_tx.send(model.last_ctrl_c_time);
                        let _ = autocomplete_tx.send(model.show_autocomplete);
                    }
                    Message::MouseEvent(_mouse_event) => {
                        // TODO: Handle mouse interactions (scrolling, clicking, etc.)
                        // For now, just update model and send state updates
                        model.update(msg);
                        let _ = mode_tx.send(model.current_mode);
                        let _ = overlay_tx.send(model.show_agent_router);
                        let _ = install_prompt_tx.send(model.show_install_prompt);
                        let _ = ctrl_c_tx.send(model.last_ctrl_c_time);
                        let _ = autocomplete_tx.send(model.show_autocomplete);
                    }
                    _ => {
                        model.update(msg);
                        // Send updated mode and overlay state to event handler after every state change
                        let _ = mode_tx.send(model.current_mode);
                        let _ = overlay_tx.send(model.show_agent_router);
                        let _ = install_prompt_tx.send(model.show_install_prompt);
                        let _ = ctrl_c_tx.send(model.last_ctrl_c_time);
                        let _ = autocomplete_tx.send(model.show_autocomplete);
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
    show_autocomplete: bool,
    last_ctrl_c_time: Option<std::time::Instant>,
    event: Event,
) -> Option<Message> {
    match event {
        Event::Key(key) if key.kind == KeyEventKind::Press => handle_key_simple(
            mode,
            show_overlay,
            show_install_prompt,
            show_autocomplete,
            last_ctrl_c_time,
            key,
        ),
        Event::Resize(width, height) => Some(Message::TerminalResize { width, height }),
        Event::Mouse(mouse_event) => Some(Message::MouseEvent(mouse_event)),
        _ => None,
    }
}

fn handle_key_simple(
    mode: AppMode,
    show_overlay: bool,
    show_install_prompt: bool,
    show_autocomplete: bool,
    _last_ctrl_c_time: Option<std::time::Instant>,
    key: KeyEvent,
) -> Option<Message> {
    // Check for Ctrl-C FIRST (even with overlays/install prompt open)
    // This ensures double Ctrl-C always works to exit
    if key.code == KeyCode::Char('c')
        && key
            .modifiers
            .contains(crossterm::event::KeyModifiers::CONTROL)
    {
        return Some(Message::ClearTextarea);
    }

    // Install prompt takes highest precedence
    if show_install_prompt {
        return match key.code {
            KeyCode::Up | KeyCode::Char('k') => Some(Message::NavigateInstallChoicePrevious),
            KeyCode::Down | KeyCode::Char('j') => Some(Message::NavigateInstallChoiceNext),
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

    // If autocomplete is visible, handle navigation and selection
    if show_autocomplete {
        return match key.code {
            KeyCode::Down | KeyCode::Char('j') if key.modifiers.is_empty() => {
                Some(Message::AutocompleteDown)
            }
            KeyCode::Up | KeyCode::Char('k') if key.modifiers.is_empty() => {
                Some(Message::AutocompleteUp)
            }
            KeyCode::Tab | KeyCode::Enter if key.modifiers.is_empty() => {
                Some(Message::AutocompleteSelect)
            }
            KeyCode::Esc => Some(Message::CloseAutocomplete),
            _ => {
                // Allow typing to continue filtering - forward to KeyPress and trigger InputChanged
                Some(Message::KeyPress(key))
            }
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

async fn spawn_and_stream(
    mut stream: Pin<Box<dyn Stream<Item = BackendEvent> + Send>>,
    tx: mpsc::UnboundedSender<Message>,
    cancel_token: tokio_util::sync::CancellationToken,
) -> Result<()> {
    // Stream is already created by the caller

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
                match event {
                    BackendEvent::Conversation(event) => {
                        let result_summary = matches!(event, ConversationEvent::ResultSummary { .. });
                        let _ = tx.send(Message::StreamEvent(event));
                        if result_summary {
                            tx.send(Message::StreamComplete)?;
                            break;
                        }
                    }
                    BackendEvent::InlineBegin { id, kind } => {
                        let _ = tx.send(Message::BeginInlineEntry { id, kind });
                    }
                    BackendEvent::InlineUpdate { id, update } => {
                        let _ = tx.send(Message::UpdateInlineEntry { id, update });
                    }
                    BackendEvent::InlineCommit { id } => {
                        let _ = tx.send(Message::CommitInlineEntry { id });
                    }
                    BackendEvent::InlineAbort { id } => {
                        let _ = tx.send(Message::AbortInlineEntry { id });
                    }
                }
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
