//! `!` commands. A session on the host runs them on that host, through
//! `_nori/session/shell`, in the session's folder; a widget with no host
//! session runs them in process. Either way the command, its output and its
//! exit code arrive as the session's `user_shell` events, which every widget
//! following the session shows as a command cell, live or replayed.

use std::collections::HashMap;

use nori_protocol::UserShellEvent;
use nori_protocol::UserShellStream;

use super::*;
use crate::app_event::HarnessAction;
use crate::host_client::HostCall;
use crate::presentation::ToolKind;
use crate::presentation::ToolPhase;
use crate::presentation::ToolSnapshot;

/// `!` commands that have started and not yet finished, by operation.
#[derive(Debug, Default)]
pub(super) struct UserShellRuns(HashMap<String, Run>);

#[derive(Debug)]
struct Run {
    command: String,
    output: Vec<u8>,
}

impl UserShellRuns {
    /// The command cell to show after `event`: running once it starts, and
    /// done with its output and exit code once it finishes. Output is held
    /// until then, so stdout and stderr show in the order they arrived.
    pub(super) fn apply(&mut self, event: UserShellEvent) -> Option<ToolSnapshot> {
        match event {
            UserShellEvent::Started {
                operation_id,
                command,
                cwd: _,
            } => {
                let snapshot = snapshot(&operation_id, &command, ToolPhase::InProgress, None);
                self.0.insert(
                    operation_id,
                    Run {
                        command,
                        output: Vec::new(),
                    },
                );
                Some(snapshot)
            }
            UserShellEvent::Output {
                operation_id,
                stream: UserShellStream::Stdout | UserShellStream::Stderr,
                chunk,
            } => {
                if let Some(run) = self.0.get_mut(&operation_id) {
                    run.output.extend_from_slice(&chunk);
                }
                None
            }
            UserShellEvent::Finished {
                operation_id,
                exit_code,
            } => {
                let run = self.0.remove(&operation_id)?;
                let phase = if exit_code == 0 {
                    ToolPhase::Completed
                } else {
                    ToolPhase::Failed
                };
                let output = String::from_utf8_lossy(&run.output).into_owned();
                Some(snapshot(
                    &operation_id,
                    &run.command,
                    phase,
                    Some(serde_json::json!({"stdout": output, "exit_code": exit_code})),
                ))
            }
        }
    }
}

fn snapshot(
    operation_id: &str,
    command: &str,
    phase: ToolPhase,
    raw_output: Option<serde_json::Value>,
) -> ToolSnapshot {
    ToolSnapshot {
        call_id: format!("user-shell-{operation_id}"),
        title: command.to_owned(),
        kind: ToolKind::Execute,
        phase,
        locations: Vec::new(),
        // The command rides in the title, not as a command invocation, so a
        // read-only `!ls` is never folded into the agent's exploring group:
        // the user asked to see its output.
        invocation: None,
        artifacts: Vec::new(),
        raw_input: None,
        raw_output,
        owner_request_id: None,
        details: Default::default(),
    }
}

impl ChatWidget {
    /// Sends a `!` command for a host session to the host; any other action,
    /// or any widget without a host session, is returned for the harness.
    pub(super) fn shell_action_on_host(&self, action: HarnessAction) -> Option<HarnessAction> {
        let Some(session_id) = self
            .managed_conversation_id
            .filter(|_| !self.cloud_mode)
            .map(|id| id.to_string())
        else {
            return Some(action);
        };
        match action {
            HarnessAction::RunUserShell(command) => {
                let tx = self.app_event_tx.clone();
                let generation = self.session_generation;
                // A joined host older than `_nori/session/shell` cannot run it,
                // and the command must never reach the agent as a prompt.
                if self
                    .host_connection
                    .as_ref()
                    .is_some_and(|connection| !connection.features.shell)
                {
                    tx.send(AppEvent::HarnessActionFailed(format!(
                        "!{command}: {}",
                        crate::command_availability::NOT_ON_THIS_HOST
                    )));
                    tx.send(AppEvent::UserShellRefused {
                        generation,
                        command,
                    });
                    return None;
                }
                let reply = tx.host_receipt(HostCall::Shell {
                    session_id,
                    command: command.clone(),
                });
                tokio::spawn(async move {
                    if let Err(error) = reply.await {
                        tx.send(AppEvent::HarnessActionFailed(format!(
                            "Could not run !{command}: {error}"
                        )));
                        tx.send(AppEvent::UserShellRefused {
                            generation,
                            command,
                        });
                    }
                });
                None
            }
            other => Some(other),
        }
    }

    /// The host refused `command`: put it back in the prompt so it is not
    /// lost, unless something new has been typed there since.
    pub(crate) fn on_user_shell_refused(&mut self, command: String) {
        if self.composer_is_empty() {
            self.set_composer_text(format!("!{command}"));
        }
    }
}
