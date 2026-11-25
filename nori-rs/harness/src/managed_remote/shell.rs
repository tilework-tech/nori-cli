//! `_nori/session/shell`: the controller asks the host to run a `!` command
//! in one session. It runs on the host, in that session's folder, as the
//! host's user, with no sandbox and no approval, as `!` always has. The
//! reply means the command started; every terminal following the session
//! sees it as that session's `user_shell` events: the command, its output,
//! and its exit code.

use nori_protocol::ShellParams;
use serde_json::Value;
use serde_json::json;

use super::ManagedRemoteHost;
use super::RemoteError;

pub(super) const SHELL: &str = "_nori/session/shell";

impl ManagedRemoteHost {
    pub(super) async fn shell(&self, params: Value) -> Result<Value, RemoteError> {
        let ShellParams {
            session_id,
            command,
        } = serde_json::from_value(params).map_err(|error| RemoteError::new(-32602, error))?;
        let command = command.trim();
        if command.is_empty() {
            return Err(RemoteError::new(-32602, "the command is empty"));
        }
        self.running(&session_id)?
            .run_user_shell(command.to_owned())
            .await
            .map_err(|error| RemoteError::new(-32000, format!("{error:#}")))?;
        Ok(json!({}))
    }
}
