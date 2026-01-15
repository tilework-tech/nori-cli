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

    /// Notification sent when the system is waiting for user approval.
    #[serde(rename_all = "kebab-case")]
    AwaitingApproval {
        call_id: String,
        command: String,
        cwd: String,
    },

    /// Notification sent when the system has been idle for a period of time.
    #[serde(rename_all = "kebab-case")]
    Idle {
        session_id: String,
        idle_duration_secs: u64,
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
    fn test_awaiting_approval_notification() -> Result<()> {
        let notification = UserNotification::AwaitingApproval {
            call_id: "call-123".to_string(),
            command: "rm -rf /tmp/test".to_string(),
            cwd: "/home/user/project".to_string(),
        };
        let serialized = serde_json::to_string(&notification)?;
        assert_eq!(
            serialized,
            r#"{"type":"awaiting-approval","call-id":"call-123","command":"rm -rf /tmp/test","cwd":"/home/user/project"}"#
        );
        Ok(())
    }

    #[test]
    fn test_idle_notification() -> Result<()> {
        let notification = UserNotification::Idle {
            session_id: "session-456".to_string(),
            idle_duration_secs: 5,
        };
        let serialized = serde_json::to_string(&notification)?;
        assert_eq!(
            serialized,
            r#"{"type":"idle","session-id":"session-456","idle-duration-secs":5}"#
        );
        Ok(())
    }
}
