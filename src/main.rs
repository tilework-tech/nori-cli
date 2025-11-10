use color_eyre::Result;
use crossterm::ExecutableCommand;
use crossterm::event::{Event, EventStream, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use futures::StreamExt;
use nori_cli::app::{AppMode, Message, Model};
use nori_cli::backends::{AgentBackend, claude::ClaudeBackend, codex::CodexBackend};
use nori_cli::ui;
use ratatui::DefaultTerminal;
use std::io::stdout;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::mpsc;
use tokio::time::{Duration, interval};

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;

    // Setup terminal
    enable_raw_mode()?;
    stdout().execute(EnterAlternateScreen)?;
    let mut terminal = ratatui::init();

    let result = run_app(&mut terminal).await;

    // Restore terminal
    disable_raw_mode()?;
    stdout().execute(LeaveAlternateScreen)?;
    ratatui::restore();

    result
}

async fn run_app(terminal: &mut DefaultTerminal) -> Result<()> {
    let mut model = Model::default();
    let (tx, mut rx) = mpsc::unbounded_channel::<Message>();

    // Spawn event handling task
    let event_tx = tx.clone();
    tokio::spawn(async move {
        let mut reader = EventStream::new();
        let mut current_mode = AppMode::Selection;
        loop {
            tokio::select! {
                Some(Ok(event)) = reader.next() => {
                    if let Some(msg) = handle_event_simple(current_mode, event) {
                        let _ = event_tx.send(msg.clone());
                        // Update local mode tracking
                        match &msg {
                            Message::SelectItem => current_mode = AppMode::Input,
                            Message::ExitInputMode | Message::StreamComplete | Message::Error(_) => current_mode = AppMode::Selection,
                            Message::SubmitInput => current_mode = AppMode::Streaming,
                            _ => {}
                        }
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
                    Message::SubmitInput => {
                        // Extract prompt from textarea
                        let prompt = model.textarea.lines().join("\n");
                        if !prompt.trim().is_empty() {
                            // Spawn subprocess
                            let backend = get_backend(&model);
                            let stream_tx = tx.clone();
                            model.update(msg);

                            tokio::spawn(async move {
                                if let Err(e) = spawn_and_stream(backend, prompt, stream_tx).await {
                                    // Error already sent via channel
                                    eprintln!("Streaming error: {}", e);
                                }
                            });
                        }
                    }
                    _ => model.update(msg),
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

fn handle_event_simple(mode: AppMode, event: Event) -> Option<Message> {
    if let Event::Key(key) = event {
        if key.kind == KeyEventKind::Press {
            return handle_key_simple(mode, key);
        }
    }
    None
}

fn handle_key_simple(mode: AppMode, key: KeyEvent) -> Option<Message> {
    match mode {
        AppMode::Selection => match key.code {
            KeyCode::Char('q') => Some(Message::Quit),
            KeyCode::Up | KeyCode::Char('k') => Some(Message::PreviousItem),
            KeyCode::Down | KeyCode::Char('j') => Some(Message::NextItem),
            KeyCode::Enter => Some(Message::SelectItem),
            _ => None,
        },
        AppMode::Input => {
            if key.code == KeyCode::Esc {
                Some(Message::ExitInputMode)
            } else if key.code == KeyCode::Enter && key.modifiers.contains(KeyModifiers::CONTROL) {
                Some(Message::SubmitInput)
            } else {
                // Send key event to be handled by textarea
                Some(Message::KeyPress(key))
            }
        }
        AppMode::Streaming => match key.code {
            KeyCode::Esc => Some(Message::StreamComplete),
            _ => None,
        },
    }
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
    let mut child = backend.spawn_process(prompt).await?;

    if let Some(stdout) = child.stdout.take() {
        let mut reader = BufReader::new(stdout).lines();

        while let Some(line) = reader.next_line().await? {
            // Parse JSON and extract content
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&line) {
                if let Some(event_type) = json.get("type").and_then(|v| v.as_str()) {
                    match event_type {
                        "agent_message" => {
                            if let Some(content) = json.get("content").and_then(|v| v.as_str()) {
                                tx.send(Message::StreamChunk(format!(
                                    "[agent_message] {}",
                                    content
                                )))?;
                            }
                        }
                        "file_change" => {
                            if let Some(path) = json.get("path").and_then(|v| v.as_str()) {
                                tx.send(Message::StreamChunk(format!("[file_change] {}", path)))?;
                            }
                        }
                        "command_execution" => {
                            if let Some(cmd) = json.get("command").and_then(|v| v.as_str()) {
                                tx.send(Message::StreamChunk(format!("[command] {}", cmd)))?;
                            }
                        }
                        _ => {
                            // Show other event types
                            tx.send(Message::StreamChunk(format!("[{}] {:?}", event_type, json)))?;
                        }
                    }
                }
            }
        }
    }

    // Wait for child to complete
    let status = child.wait().await?;
    if !status.success() {
        tx.send(Message::Error(format!(
            "Process exited with status: {}",
            status
        )))?;
    } else {
        tx.send(Message::StreamComplete)?;
    }

    Ok(())
}
