//! `_nori/session/context`: any terminal reads a session's context report.
//! Usage comes from the session's own timeline and the latest reading the
//! replay window keeps; instruction files are read on the host, from the
//! session's folder, so a terminal on another machine sees what the agent
//! loads rather than what its own disk holds.

use std::path::Path;

use nori_protocol::ContextInstruction;
use nori_protocol::ContextParams;
use nori_protocol::ContextReport;
use nori_protocol::ContextUsage;
use serde_json::Value;
use serde_json::json;

use super::ManagedRemoteHost;
use super::RemoteError;
use crate::instruction_files::detect_agent_kind;
use crate::instruction_files::discover_all_instruction_files;

pub(super) const CONTEXT: &str = "_nori/session/context";

impl ManagedRemoteHost {
    pub(super) async fn context(&self, params: Value) -> Result<Value, RemoteError> {
        let ContextParams {
            session_id,
            skip_instructions,
        } = serde_json::from_value(params).map_err(|error| RemoteError::new(-32602, error))?;
        let handle = self.running(&session_id)?;
        let id = self.target(Some(&session_id))?;
        let timeline = handle
            .context_timeline()
            .await
            .map_err(|error| RemoteError::new(-32000, format!("{error:#}")))?;
        let usage = self
            .manager
            .replay_info(&id)
            .and_then(|info| info.context)
            .map(|(used, size)| ContextUsage { used, size });
        let instructions = match self.manager.get(&id) {
            Some(record) if !skip_instructions => {
                tokio::task::spawn_blocking(move || instructions(&record.agent, &record.cwd))
                    .await
                    .map_err(|error| RemoteError::new(-32000, error))?
            }
            Some(_) | None => Vec::new(),
        };
        Ok(json!(ContextReport {
            timeline,
            instructions,
            usage,
        }))
    }
}

/// The instruction files `agent` loads from `cwd`, with paths under the
/// host's home shown from `~`.
fn instructions(agent: &str, cwd: &Path) -> Vec<ContextInstruction> {
    let home = dirs::home_dir();
    discover_all_instruction_files(cwd, detect_agent_kind(agent))
        .into_iter()
        .filter(|file| file.active)
        .map(|file| ContextInstruction {
            path: match home
                .as_deref()
                .and_then(|home| file.path.strip_prefix(home).ok())
            {
                Some(rest) => format!("~{}{}", std::path::MAIN_SEPARATOR, rest.display()),
                None => file.path.display().to_string(),
            },
            tokens: file.token_count.map(|tokens| tokens.count),
        })
        .collect()
}
