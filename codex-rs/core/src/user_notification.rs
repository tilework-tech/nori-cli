use serde::Serialize;
use tracing::debug;
use tracing::error;
use tracing::warn;

/// User notifier that sends OS-level desktop notifications.
///
/// Supports two modes:
/// 1. **Native notifications**: Uses `notify-rust` to send desktop
///    notifications directly, with support for click-to-focus on X11.
///    Enabled by setting `use_native: true` in the constructor.
/// 2. **External script**: Invokes a user-configured command with JSON payload.
#[derive(Debug, Default)]
pub struct UserNotifier {
    /// External command to invoke for notifications (legacy mode).
    notify_command: Option<Vec<String>>,
    /// Whether to use native notifications when no external command is configured.
    use_native: bool,
    /// Process ID for window focus (used in click handlers on X11 Linux).
    #[cfg_attr(
        not(all(target_os = "linux", not(target_env = "musl"))),
        allow(dead_code)
    )]
    process_id: Option<u32>,
}

impl UserNotifier {
    /// Send a notification using the configured method.
    ///
    /// If an external command is configured, uses that (legacy behavior).
    /// If native notifications are enabled and no external command is configured,
    /// sends a native desktop notification.
    ///
    /// Note: If `notify_command` is `Some` but empty, no notification is sent.
    /// This allows tests to disable notifications by setting `config.notify = Some(vec![])`.
    pub fn notify(&self, notification: &UserNotification) {
        if let Some(notify_command) = &self.notify_command {
            // External command is configured - use it if non-empty, otherwise skip entirely
            if !notify_command.is_empty() {
                self.invoke_notify(notify_command, notification)
            }
            // Empty notify_command means notifications are explicitly disabled
        } else if self.use_native {
            // No external command configured - use native notifications if enabled
            self.send_native(notification);
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

    /// Send a native desktop notification using notify-rust.
    fn send_native(&self, notification: &UserNotification) {
        use notify_rust::Notification;

        let title = notification.title();
        let body = notification.body();

        debug!("Sending native notification: {title}");

        // Build the notification
        let mut notif = Notification::new();
        notif.summary(title).body(&body).appname("Nori");

        // On Linux with X11, we can add a click action to focus the terminal
        #[cfg(all(target_os = "linux", not(target_env = "musl")))]
        {
            // Check if we're on X11 (not Wayland) by checking XDG_SESSION_TYPE
            let is_x11 = std::env::var("XDG_SESSION_TYPE")
                .map(|v| v == "x11")
                .unwrap_or(false)
                || std::env::var("DISPLAY").is_ok() && std::env::var("WAYLAND_DISPLAY").is_err();

            if is_x11 && let Some(pid) = self.process_id {
                // Add action for click-to-focus
                notif.action("default", "Focus Terminal");
                notif.hint(notify_rust::Hint::Resident(true));

                // Spawn a thread to handle the click action
                std::thread::spawn(move || {
                    if let Ok(handle) = notif.show() {
                        handle.wait_for_action(|action| {
                            if action == "default" {
                                focus_window_by_pid(pid);
                            }
                        });
                    }
                });
                return;
            }
        }

        // Default: just show the notification without click handling
        // Note: macOS click-to-focus could be implemented via AppleScript in the future
        if let Err(e) = notif.show() {
            warn!("failed to show native notification: {e}");
        }
    }

    /// Create a new UserNotifier.
    ///
    /// # Arguments
    /// * `notify` - Optional external command for notifications (legacy mode)
    /// * `use_native` - Whether to use native desktop notifications when no command is configured
    pub fn new(notify: Option<Vec<String>>, use_native: bool) -> Self {
        Self {
            notify_command: notify,
            use_native,
            process_id: Some(std::process::id()),
        }
    }
}

/// Focus a window by process ID using wmctrl or xdotool (X11 only).
#[cfg(all(target_os = "linux", not(target_env = "musl")))]
fn focus_window_by_pid(pid: u32) {
    use std::process::Command;

    debug!("Attempting to focus window for PID {pid}");

    // Try wmctrl first (more reliable for focusing)
    // wmctrl -i -a <window_id> can focus by window ID
    // We need to find the window ID from the PID first
    let wmctrl_result = Command::new("wmctrl")
        .args(["-l", "-p"])
        .output()
        .ok()
        .and_then(|output| {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                // Find line containing our PID
                for line in stdout.lines() {
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    if parts.len() >= 3
                        && let Ok(window_pid) = parts[2].parse::<u32>()
                        && window_pid == pid
                    {
                        // Found our window, activate it
                        let window_id = parts[0];
                        return Command::new("wmctrl")
                            .args(["-i", "-a", window_id])
                            .status()
                            .ok();
                    }
                }
            }
            None
        });

    if wmctrl_result.is_some() {
        debug!("Successfully focused window using wmctrl");
        return;
    }

    // Fallback to xdotool
    let xdotool_result = Command::new("xdotool")
        .args(["search", "--pid", &pid.to_string(), "--onlyvisible"])
        .output()
        .ok()
        .and_then(|output| {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                if let Some(window_id) = stdout.lines().next() {
                    return Command::new("xdotool")
                        .args(["windowactivate", window_id.trim()])
                        .status()
                        .ok();
                }
            }
            None
        });

    if xdotool_result.is_some() {
        debug!("Successfully focused window using xdotool");
    } else {
        debug!("Could not focus window - wmctrl and xdotool both failed");
    }
}

/// Maximum length for command strings in notification body before truncation.
const MAX_COMMAND_LENGTH: usize = 100;

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

impl UserNotification {
    /// Returns a human-readable title for the notification.
    pub fn title(&self) -> &'static str {
        match self {
            UserNotification::AgentTurnComplete { .. } => "Nori: Task Complete",
            UserNotification::AwaitingApproval { .. } => "Nori: Approval Required",
            UserNotification::Idle { .. } => "Nori: Session Idle",
        }
    }

    /// Returns a human-readable body for the notification.
    pub fn body(&self) -> String {
        match self {
            UserNotification::AgentTurnComplete {
                last_assistant_message,
                input_messages,
                cwd,
                ..
            } => {
                if let Some(msg) = last_assistant_message {
                    truncate_string(msg, MAX_COMMAND_LENGTH)
                } else if let Some(first_input) = input_messages.first() {
                    format!(
                        "Completed: {}",
                        truncate_string(first_input, MAX_COMMAND_LENGTH)
                    )
                } else {
                    format!("Task completed in {cwd}")
                }
            }
            UserNotification::AwaitingApproval { command, cwd, .. } => {
                let truncated_command = truncate_string(command, MAX_COMMAND_LENGTH);
                format!("{truncated_command}\nin {cwd}")
            }
            UserNotification::Idle { .. } => "Session has been idle".to_string(),
        }
    }
}

/// Truncates a string to max_len bytes, adding "..." if truncated.
/// Ensures we don't slice in the middle of a multi-byte UTF-8 character.
fn truncate_string(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        // Find the last valid char boundary at or before max_len
        let boundary = s
            .char_indices()
            .take_while(|(i, _)| *i < max_len)
            .last()
            .map(|(i, c)| i + c.len_utf8())
            .unwrap_or(0);
        format!("{}...", &s[..boundary])
    }
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

    #[test]
    fn test_truncate_handles_multibyte_unicode() {
        let s = "echo \u{1F600}".repeat(50);
        let truncated = truncate_string(&s, 100);
        assert!(truncated.ends_with("..."));
        assert!(truncated.len() <= 107); // 100 bytes + "..."
    }
}
