//! Custom prompts of a session on the host come from the host's own
//! `$NORI_HOME/commands`, and their scripts run on the host in that
//! session's folder, so every terminal on the host offers the same prompts
//! wherever its own home is. Markdown prompts are still filled in here.

use super::*;
use crate::app_event::HarnessAction;
use crate::host_client::HostCall;

impl ChatWidget {
    fn host_session_id(&self) -> Option<String> {
        self.managed_conversation_id
            .filter(|_| !self.cloud_mode)
            .map(|id| id.to_string())
    }

    /// Asks the host for its custom prompts when this widget shows a host
    /// session, leaving the popup as it is when the host cannot list them; any other action, or any widget without one, is returned
    /// for the harness.
    pub(super) fn prompts_action_on_host(&self, action: HarnessAction) -> Option<HarnessAction> {
        if !matches!(action, HarnessAction::LoadCustomPrompts) || self.host_session_id().is_none() {
            return Some(action);
        }
        let tx = self.app_event_tx.clone();
        let reply = tx.host_receipt(HostCall::ListPrompts);
        tokio::spawn(async move {
            let listed = reply.await.and_then(|reply| {
                serde_json::from_value::<nori_protocol::PromptsListResult>(reply)
                    .map_err(Into::into)
            });
            match listed {
                Ok(listed) => tx.send(AppEvent::CustomPromptsLoaded(
                    listed.prompts.into_iter().map(harness_prompt).collect(),
                )),
                // The popup keeps the prompts it has; a host that cannot list
                // them now, such as one that ended a finished child, is no
                // error to report.
                Err(error) => tracing::debug!(%error, "custom prompts unavailable from the host"),
            }
        });
        None
    }

    /// Runs a script prompt on the host when this widget shows a host
    /// session, reporting its output as [`AppEvent::ScriptExecutionComplete`].
    /// A widget without one gets the prompt back to run itself.
    pub(crate) fn run_script_on_host(
        &self,
        prompt: nori_harness::CustomPrompt,
        args: Vec<String>,
    ) -> Option<(nori_harness::CustomPrompt, Vec<String>)> {
        let Some(session_id) = self.host_session_id() else {
            return Some((prompt, args));
        };
        let tx = self.app_event_tx.clone();
        let name = prompt.name;
        let reply = tx.host_receipt(HostCall::RunPrompt {
            session_id,
            name: name.clone(),
            args,
        });
        tokio::spawn(async move {
            let result = reply
                .await
                .and_then(|reply| {
                    serde_json::from_value::<nori_protocol::PromptRunResult>(reply)
                        .map_err(Into::into)
                })
                .map(|ran| ran.stdout)
                .map_err(|error| error.to_string());
            tx.send(AppEvent::ScriptExecutionComplete { name, result });
        });
        None
    }
}

/// A host prompt as the composer knows prompts. It has no path here: the
/// host finds a script by name.
fn harness_prompt(prompt: nori_protocol::HostPrompt) -> nori_harness::CustomPrompt {
    nori_harness::CustomPrompt {
        name: prompt.name,
        path: std::path::PathBuf::new(),
        content: prompt.content,
        description: prompt.description,
        argument_hint: prompt.argument_hint,
        kind: match prompt.kind {
            nori_protocol::HostPromptKind::Markdown => nori_harness::CustomPromptKind::Markdown,
            nori_protocol::HostPromptKind::Script { interpreter } => {
                nori_harness::CustomPromptKind::Script { interpreter }
            }
        },
    }
}
