//! `_nori/host/prompts` and `_nori/session/prompts/run`: the custom prompts
//! in the host's own `$NORI_HOME/commands`. Every terminal on the host sees
//! the same list wherever its own home is; the controller runs a script
//! prompt on the host, in the session's folder, found by name only.

use nori_harness::ConversationId;
use nori_harness::CustomPrompt;
use nori_harness::CustomPromptKind;
use nori_harness::custom_prompts;
use nori_protocol::HostPrompt;
use nori_protocol::HostPromptKind;
use nori_protocol::PromptRunParams;
use nori_protocol::PromptRunResult;
use nori_protocol::PromptsListParams;
use nori_protocol::PromptsListResult;
use serde_json::Value;

use super::AcpError;
use crate::host::service::HostService;

pub(in crate::host) const LIST: &str = "_nori/host/prompts";
pub(in crate::host) const RUN: &str = "_nori/session/prompts/run";

impl HostService {
    pub(super) async fn list_prompts(&self, params: Value) -> Result<Value, AcpError> {
        let PromptsListParams { session_id: _ } =
            serde_json::from_value(params).map_err(AcpError::invalid_params)?;
        let prompts = self.discover_prompts().await;
        serde_json::to_value(PromptsListResult {
            prompts: prompts.into_iter().map(host_prompt).collect(),
        })
        .map_err(AcpError::internal)
    }

    pub(super) async fn run_prompt(&self, client: i64, params: Value) -> Result<Value, AcpError> {
        if !self.acp.connection(client)?.is_controller() {
            return Err(AcpError {
                code: -32016,
                message: "watcher is read-only; take control before changing a session".into(),
            });
        }
        let PromptRunParams {
            session_id,
            name,
            args,
        } = serde_json::from_value(params).map_err(AcpError::invalid_params)?;
        let cwd = ConversationId::from_string(&session_id)
            .ok()
            .filter(|id| !self.is_deleted(id))
            .and_then(|id| self.manager.get(&id))
            .map(|record| record.cwd)
            .ok_or_else(|| AcpError::invalid_params("unknown session"))?;
        let script = self
            .discover_prompts()
            .await
            .into_iter()
            .find(|prompt| {
                prompt.name == name && matches!(prompt.kind, CustomPromptKind::Script { .. })
            })
            .ok_or_else(|| AcpError::invalid_params(format!("no script prompt named {name}")))?;
        let stdout = custom_prompts::execute_script(
            &script,
            &args,
            self.config.script_timeout.as_duration(),
            Some(&cwd),
        )
        .await
        .map_err(|message| AcpError {
            code: -32603,
            message,
        })?;
        serde_json::to_value(PromptRunResult { stdout }).map_err(AcpError::internal)
    }

    async fn discover_prompts(&self) -> Vec<CustomPrompt> {
        custom_prompts::discover_prompts_in(&custom_prompts::commands_dir(&self.config.nori_home))
            .await
    }
}

fn host_prompt(prompt: CustomPrompt) -> HostPrompt {
    HostPrompt {
        name: prompt.name,
        description: prompt.description,
        argument_hint: prompt.argument_hint,
        kind: match prompt.kind {
            CustomPromptKind::Markdown => HostPromptKind::Markdown,
            CustomPromptKind::Script { interpreter } => HostPromptKind::Script { interpreter },
        },
        content: prompt.content,
    }
}
