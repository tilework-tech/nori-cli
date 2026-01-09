use serde::Serialize;
use tracing::error;
use tracing::warn;

#[derive(Debug, Default)]
pub struct UserNotifier {
    notify_command: Option<Vec<String>>,
}

impl UserNotifier {
    pub fn notify(&self, notification: &UserNotification) {
        if let Some(notify_command) = &self.notify_command
            && !notify_command.is_empty()
        {
            self.invoke_notify(notify_command, notification)
        }
    }

    fn invoke_notify(&self, notify_command: &[String], notification: &UserNotification) {
        let Ok(json) = serde_json::to_string(&notification) else {
            error!("failed to serialise notification payload");
            return;
        };

        let mut command = std::process::Command::new(&notify_command[0]);
        if notify_command.len() > 1 {
            command.args(&notify_command[1..]);
        }
        command.arg(json);

        // Fire-and-forget – we do not wait for completion.
        if let Err(e) = command.spawn() {
            warn!("failed to spawn notifier '{}': {e}", notify_command[0]);
        }
    }

    pub fn new(notify: Option<Vec<String>>) -> Self {
        Self {
            notify_command: notify,
        }
    }
}

/// User can configure a program that will receive notifications. Each
/// notification is serialized as JSON and passed as an argument to the
/// program.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum UserNotification {
    #[serde(rename_all = "kebab-case")]
    AgentTurnComplete {
        thread_id: String,
        turn_id: String,
        cwd: String,

        /// Messages that the user sent to the agent to initiate the turn.
        input_messages: Vec<String>,

        /// The last message sent by the assistant in the turn.
        last_assistant_message: Option<String>,
    },

    /// Notification sent when an approval request arrives, optionally after
    /// an idle period. This allows external notification handlers to alert
    /// the user when the agent needs their attention.
    #[serde(rename_all = "kebab-case")]
    ApprovalRequested {
        /// The type of approval request: "exec", "edit", or "elicitation"
        request_type: String,

        /// Human-readable message describing the approval request
        message: String,

        /// For exec requests, the command being requested
        #[serde(skip_serializing_if = "Option::is_none")]
        command: Option<String>,

        /// For edit requests, the list of file paths being modified
        #[serde(skip_serializing_if = "Option::is_none")]
        file_paths: Option<Vec<String>>,

        /// How long the system was idle before this request, in seconds.
        /// Present when the idle threshold was exceeded.
        #[serde(skip_serializing_if = "Option::is_none")]
        idle_duration_secs: Option<u64>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;

    #[test]
    fn test_user_notification() -> Result<()> {
        let notification = UserNotification::AgentTurnComplete {
            thread_id: "b5f6c1c2-1111-2222-3333-444455556666".to_string(),
            turn_id: "12345".to_string(),
            cwd: "/Users/example/project".to_string(),
            input_messages: vec!["Rename `foo` to `bar` and update the callsites.".to_string()],
            last_assistant_message: Some(
                "Rename complete and verified `cargo build` succeeds.".to_string(),
            ),
        };
        let serialized = serde_json::to_string(&notification)?;
        assert_eq!(
            serialized,
            r#"{"type":"agent-turn-complete","thread-id":"b5f6c1c2-1111-2222-3333-444455556666","turn-id":"12345","cwd":"/Users/example/project","input-messages":["Rename `foo` to `bar` and update the callsites."],"last-assistant-message":"Rename complete and verified `cargo build` succeeds."}"#
        );
        Ok(())
    }

    #[test]
    fn test_approval_requested_exec_notification() -> Result<()> {
        let notification = UserNotification::ApprovalRequested {
            request_type: "exec".to_string(),
            message: "Approval needed for command execution".to_string(),
            command: Some("rm -rf /tmp/test".to_string()),
            file_paths: None,
            idle_duration_secs: Some(5),
        };
        let serialized = serde_json::to_string(&notification)?;
        assert!(serialized.contains(r#""type":"approval-requested""#));
        assert!(serialized.contains(r#""request-type":"exec""#));
        assert!(serialized.contains(r#""command":"rm -rf /tmp/test""#));
        assert!(serialized.contains(r#""idle-duration-secs":5"#));
        Ok(())
    }

    #[test]
    fn test_approval_requested_edit_notification() -> Result<()> {
        let notification = UserNotification::ApprovalRequested {
            request_type: "edit".to_string(),
            message: "Approval needed for file edits".to_string(),
            command: None,
            file_paths: Some(vec![
                "/path/to/file1.rs".to_string(),
                "/path/to/file2.rs".to_string(),
            ]),
            idle_duration_secs: None,
        };
        let serialized = serde_json::to_string(&notification)?;
        assert!(serialized.contains(r#""type":"approval-requested""#));
        assert!(serialized.contains(r#""request-type":"edit""#));
        assert!(serialized.contains(r#""file-paths""#));
        assert!(serialized.contains(r#"/path/to/file1.rs"#));
        Ok(())
    }
}
