mod app;
mod autocomplete;
mod backends;
mod commands;
mod conversation;
mod ui;

use crate::app::{AppMode, InstallChoice, Message, Model};
use crate::autocomplete::update_autocomplete_state;
use crate::backends::{AgentBackend, claude::ClaudeBackend, codex::CodexBackend};
use crate::commands::{CommandRegistry, parse_slash_command};
use crate::conversation::{ConversationEvent, render_event, should_render_event};

use color_eyre::Result;
use crossterm::cursor::MoveTo;
use crossterm::event::{Event, EventStream, KeyCode, KeyEvent, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use futures::StreamExt;
use ratatui::{DefaultTerminal, TerminalOptions, Viewport};
use tokio::sync::mpsc;
use tokio::time::{Duration, interval};

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;

    // Get starting cursor position before creating viewport
    let viewport_start_row = crossterm::cursor::position()?.1;

    // Setup terminal with inline viewport (20 lines at bottom for input/autocomplete/instructions)
    enable_raw_mode()?;
    let mut terminal = ratatui::init_with_options(TerminalOptions {
        viewport: Viewport::Inline(20),
    });

    let result = run_app(&mut terminal).await;

    // Restore terminal
    ratatui::restore();
    disable_raw_mode()?;

    // Move cursor below the viewport and clear any remaining artifacts
    use crossterm::terminal::Clear;
    execute!(
        std::io::stdout(),
        MoveTo(0, viewport_start_row + 20),
        Clear(crossterm::terminal::ClearType::FromCursorDown)
    )?;

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
    let (ctrl_c_tx, mut ctrl_c_rx) = mpsc::unbounded_channel::<Option<std::time::Instant>>();
    let (autocomplete_tx, mut autocomplete_rx) = mpsc::unbounded_channel::<bool>();

    // Spawn event handling task
    let event_tx = tx.clone();
    tokio::spawn(async move {
        let mut reader = EventStream::new();
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
                Some(Ok(event)) = reader.next() => {
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
                        let prompt = model.textarea.lines().join("\n");
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
                                        model.textarea = {
                                            let mut textarea = tui_textarea::TextArea::default();
                                            textarea.set_cursor_line_style(ratatui::style::Style::default());
                                            textarea
                                        };
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

                                // Check if backend is available before spawning
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
                                    let _ = ctrl_c_tx.send(model.last_ctrl_c_time);

                                    tokio::spawn(async move {
                                        if let Err(e) = spawn_and_stream(backend, prompt, stream_tx, cancel_token).await {
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
                        let input = model.textarea.lines().join("\n");
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
                        let input = model.textarea.lines().join("\n");
                        update_autocomplete_state(&mut model, &input, &command_registry);
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
                // Increment loading frame for animation (only used when not using codex components)
                if model.current_mode == AppMode::Streaming && !model.use_codex_components {
                    model.loading_frame = model.loading_frame.wrapping_add(1);
                }
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
    if let Event::Key(key) = event
        && key.kind == KeyEventKind::Press
    {
        return handle_key_simple(
            mode,
            show_overlay,
            show_install_prompt,
            show_autocomplete,
            last_ctrl_c_time,
            key,
        );
    }
    None
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

fn get_backend(model: &Model) -> Box<dyn AgentBackend + Send> {
    match model.selected_agent_index {
        Some(0) => Box::new(ClaudeBackend::new()),
        Some(1) => Box::new(CodexBackend::new()),
        _ => Box::new(ClaudeBackend::new()), // Default
    }
}

/// Wrap text to fit within the specified width, preserving styling from spans.
/// Returns a vector of Lines, each fitting within the width constraint.
fn wrap_text_to_width(
    text: &ratatui::text::Text,
    width: usize,
) -> Vec<ratatui::text::Line<'static>> {
    use ratatui::text::{Line, Span};
    use unicode_width::UnicodeWidthStr;

    let mut wrapped_lines = Vec::new();

    // If width is too small, just return the original line
    if width < 10 {
        for line in &text.lines {
            let owned_spans: Vec<Span> = line
                .spans
                .iter()
                .map(|s| Span::styled(s.content.to_string(), s.style))
                .collect();
            wrapped_lines.push(Line::from(owned_spans));
        }
        return wrapped_lines;
    }

    for line in &text.lines {
        let mut current_line_spans = Vec::new();
        let mut current_width = 0;

        for span in &line.spans {
            let span_text = span.content.as_ref();

            // Handle standalone space spans (don't split them)
            if span_text.trim().is_empty() && !span_text.is_empty() {
                current_line_spans.push(Span::styled(span_text.to_string(), span.style));
                current_width += span_text.width();
                continue;
            }

            // Split span text by words to wrap properly
            let words: Vec<&str> = span_text.split_whitespace().collect();

            for (i, word) in words.iter().enumerate() {
                let word_width = word.width();
                let space_width = if i < words.len() - 1 { 1 } else { 0 };

                // If this word would exceed width, start new line
                if current_width + word_width > width && current_width > 0 {
                    wrapped_lines.push(Line::from(current_line_spans.clone()));
                    current_line_spans.clear();
                    current_width = 0;
                }

                // If word itself is too long (longer than width), split it character by character
                if word_width > width {
                    let mut remaining = word.to_string();
                    while !remaining.is_empty() {
                        let mut chunk = String::new();
                        let mut chunk_width = 0;

                        for ch in remaining.chars() {
                            let ch_width = ch.to_string().width();
                            if chunk_width + ch_width > width && !chunk.is_empty() {
                                break;
                            }
                            chunk.push(ch);
                            chunk_width += ch_width;
                        }

                        if !chunk.is_empty() {
                            if current_width > 0 {
                                wrapped_lines.push(Line::from(current_line_spans.clone()));
                                current_line_spans.clear();
                            }
                            current_line_spans.push(Span::styled(chunk.clone(), span.style));
                            wrapped_lines.push(Line::from(current_line_spans.clone()));
                            current_line_spans.clear();
                            current_width = 0;

                            remaining = remaining[chunk.len()..].to_string();
                        } else {
                            break;
                        }
                    }
                } else {
                    // Add the word normally
                    let word_str = if space_width > 0 {
                        format!("{word} ")
                    } else {
                        word.to_string()
                    };

                    current_line_spans.push(Span::styled(word_str, span.style));
                    current_width += word_width + space_width;
                }
            }
        }

        // Add remaining spans as a line
        if !current_line_spans.is_empty() {
            wrapped_lines.push(Line::from(current_line_spans));
        } else if wrapped_lines.is_empty() {
            // Empty line - preserve it
            wrapped_lines.push(Line::from(""));
        }
    }

    // If no lines were created, return at least one empty line
    if wrapped_lines.is_empty() {
        wrapped_lines.push(Line::from(""));
    }

    wrapped_lines
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
                let _ = tx.send(Message::StreamEvent(event.clone()));
                // End stream immediately on ResultSummary to prevent hanging
                if matches!(event, ConversationEvent::ResultSummary { .. }) {
                    tx.send(Message::StreamComplete)?;
                    break;
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
