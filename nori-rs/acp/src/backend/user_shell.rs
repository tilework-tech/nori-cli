use std::path::Path;
use std::time::Instant;

use codex_protocol::protocol::Event;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::ExecCommandBeginEvent;
use codex_protocol::protocol::ExecCommandEndEvent;
use codex_protocol::protocol::ExecCommandOutputDeltaEvent;
use codex_protocol::protocol::ExecCommandSource;
use codex_protocol::protocol::ExecOutputStream;
use codex_protocol::protocol::TaskCompleteEvent;
use codex_protocol::protocol::TaskStartedEvent;
use tokio::process::Command;
use tokio::sync::mpsc;

pub(crate) async fn run_user_shell_command(
    event_tx: &mpsc::Sender<Event>,
    id: &str,
    cwd: &Path,
    command: String,
) {
    let argv = shell_command_argv(&command);
    let parsed_cmd = codex_core::parse_command::parse_command(&argv);
    let call_id = format!("user-shell-{id}");
    let started = Instant::now();

    let _ = event_tx
        .send(Event {
            id: id.to_string(),
            msg: EventMsg::TaskStarted(TaskStartedEvent {
                model_context_window: None,
            }),
        })
        .await;

    let _ = event_tx
        .send(Event {
            id: id.to_string(),
            msg: EventMsg::ExecCommandBegin(ExecCommandBeginEvent {
                call_id: call_id.clone(),
                process_id: None,
                turn_id: id.to_string(),
                command: argv.clone(),
                cwd: cwd.to_path_buf(),
                parsed_cmd: parsed_cmd.clone(),
                source: ExecCommandSource::UserShell,
                interaction_input: None,
            }),
        })
        .await;

    let output = Command::new(&argv[0])
        .args(&argv[1..])
        .current_dir(cwd)
        .output()
        .await;

    let (stdout_bytes, stderr_bytes, exit_code) = match output {
        Ok(output) => (
            output.stdout,
            output.stderr,
            output.status.code().unwrap_or(1),
        ),
        Err(err) => (Vec::new(), err.to_string().into_bytes(), 1),
    };

    if !stdout_bytes.is_empty() {
        let _ = event_tx
            .send(Event {
                id: id.to_string(),
                msg: EventMsg::ExecCommandOutputDelta(ExecCommandOutputDeltaEvent {
                    call_id: call_id.clone(),
                    stream: ExecOutputStream::Stdout,
                    chunk: stdout_bytes.clone(),
                }),
            })
            .await;
    }
    if !stderr_bytes.is_empty() {
        let _ = event_tx
            .send(Event {
                id: id.to_string(),
                msg: EventMsg::ExecCommandOutputDelta(ExecCommandOutputDeltaEvent {
                    call_id: call_id.clone(),
                    stream: ExecOutputStream::Stderr,
                    chunk: stderr_bytes.clone(),
                }),
            })
            .await;
    }

    let stdout = String::from_utf8_lossy(&stdout_bytes).to_string();
    let stderr = String::from_utf8_lossy(&stderr_bytes).to_string();
    let aggregated_output = format!("{stdout}{stderr}");
    let _ = event_tx
        .send(Event {
            id: id.to_string(),
            msg: EventMsg::ExecCommandEnd(ExecCommandEndEvent {
                call_id,
                process_id: None,
                turn_id: id.to_string(),
                command: argv,
                cwd: cwd.to_path_buf(),
                parsed_cmd,
                source: ExecCommandSource::UserShell,
                interaction_input: None,
                stdout,
                stderr,
                aggregated_output: aggregated_output.clone(),
                exit_code,
                duration: started.elapsed(),
                formatted_output: aggregated_output,
            }),
        })
        .await;

    let _ = event_tx
        .send(Event {
            id: id.to_string(),
            msg: EventMsg::TaskComplete(TaskCompleteEvent {
                last_agent_message: None,
            }),
        })
        .await;
}

#[cfg(windows)]
fn shell_command_argv(command: &str) -> Vec<String> {
    vec!["cmd".to_string(), "/C".to_string(), command.to_string()]
}

#[cfg(not(windows))]
fn shell_command_argv(command: &str) -> Vec<String> {
    let shell = std::env::var("SHELL")
        .ok()
        .filter(|shell| !shell.trim().is_empty())
        .unwrap_or_else(|| "/bin/sh".to_string());
    vec![shell, "-lc".to_string(), command.to_string()]
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[tokio::test]
    async fn user_shell_command_runs_and_finishes_with_task_complete() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let (event_tx, mut event_rx) = mpsc::channel(8);

        run_user_shell_command(
            &event_tx,
            "shell-test",
            temp_dir.path(),
            "printf nori-shell".to_string(),
        )
        .await;
        drop(event_tx);

        let mut saw_stdout = false;
        let mut saw_complete = false;
        while let Some(event) = event_rx.recv().await {
            if let EventMsg::ExecCommandOutputDelta(ev) = &event.msg
                && ev.stream == ExecOutputStream::Stdout
                && ev.chunk == b"nori-shell"
            {
                saw_stdout = true;
            }
            if let EventMsg::ExecCommandEnd(ev) = &event.msg {
                assert_eq!(ev.source, ExecCommandSource::UserShell);
                assert_eq!(ev.cwd, temp_dir.path());
                assert_eq!(ev.stdout, "nori-shell");
                assert_eq!(ev.exit_code, 0);
            }
            if let EventMsg::TaskComplete(ev) = &event.msg {
                assert_eq!(ev.last_agent_message, None);
                saw_complete = true;
            }
        }

        assert!(saw_stdout);
        assert!(saw_complete);
    }
}
