use std::path::Path;
use tokio::process::Command;
use tokio::sync::mpsc;

use super::BackendEvent;
use nori_protocol::NoriEvent;
use nori_protocol::SessionEvent;
use nori_protocol::UserShellEvent;
use nori_protocol::UserShellStream;

pub(crate) async fn run_user_shell_command(
    event_tx: &mpsc::Sender<BackendEvent>,
    id: &str,
    cwd: &Path,
    command: String,
) {
    let argv = shell_command_argv(&command);

    let _ = event_tx
        .send(BackendEvent::Public(SessionEvent::Nori(
            NoriEvent::UserShell(UserShellEvent::Started {
                operation_id: id.to_string(),
                command,
                cwd: cwd.to_path_buf(),
            }),
        )))
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
            .send(BackendEvent::Public(SessionEvent::Nori(
                NoriEvent::UserShell(UserShellEvent::Output {
                    operation_id: id.to_string(),
                    stream: UserShellStream::Stdout,
                    chunk: stdout_bytes.clone(),
                }),
            )))
            .await;
    }
    if !stderr_bytes.is_empty() {
        let _ = event_tx
            .send(BackendEvent::Public(SessionEvent::Nori(
                NoriEvent::UserShell(UserShellEvent::Output {
                    operation_id: id.to_string(),
                    stream: UserShellStream::Stderr,
                    chunk: stderr_bytes.clone(),
                }),
            )))
            .await;
    }

    let _ = event_tx
        .send(BackendEvent::Public(SessionEvent::Nori(
            NoriEvent::UserShell(UserShellEvent::Finished {
                operation_id: id.to_string(),
                exit_code,
            }),
        )))
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
    async fn user_shell_command_emits_typed_lifecycle() {
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
            if let BackendEvent::Public(SessionEvent::Nori(NoriEvent::UserShell(
                UserShellEvent::Output { stream, chunk, .. },
            ))) = &event
                && *stream == UserShellStream::Stdout
                && chunk
                    .windows(b"nori-shell".len())
                    .any(|w| w == b"nori-shell")
            {
                saw_stdout = true;
            }
            if let BackendEvent::Public(SessionEvent::Nori(NoriEvent::UserShell(
                UserShellEvent::Finished { exit_code, .. },
            ))) = &event
            {
                assert_eq!(*exit_code, 0);
                saw_complete = true;
            }
        }

        assert!(saw_stdout);
        assert!(saw_complete);
    }
}
