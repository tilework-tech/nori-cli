//! This terminal's role on a host it joined. A terminal that owns its host
//! has no role to follow and always controls.

use super::*;
use crate::host_client::HostConnection;
use nori_protocol::ControlRole;

/// Typed on a joined host, leaves it like Ctrl+D; its sessions keep running.
pub(crate) const DETACH_COMMAND: &str = "/detach";
/// Typed on a joined host, interrupts the turn on screen like Esc.
pub(crate) const CANCEL_COMMAND: &str = "/cancel";
/// Typed commands with arguments that a joined terminal refuses with the
/// reason their menu row shows, rather than as typed input.
pub(crate) fn refuses_with_its_own_reason(text: &str) -> bool {
    ["/new ", "/approvals "]
        .iter()
        .any(|command| text.starts_with(command))
}

/// Commands only a joined host's terminal takes, without the slash.
pub(crate) const JOINED_COMMANDS: [&str; 2] = ["detach", "cancel"];

/// Why a watcher's prompt stays in the composer.
pub(crate) const WATCHER_INPUT: &str = "Watching: another terminal controls this host. Press Alt+T to take control; your draft is kept.";
const TAKE_CONTROL_RECONNECTING: &str =
    "Reconnecting. Take control after the connection is restored.";

impl ChatWidget {
    /// Follow a joined host's standing for this terminal.
    pub(crate) fn set_host_connection(&mut self, connection: HostConnection) {
        self.host_connection = Some(connection);
        self.refresh_host_status();
        self.refresh_builtin_command_availability();
    }

    pub(crate) fn host_connection(&self) -> Option<HostConnection> {
        self.host_connection.clone()
    }

    /// The host's latest status for the session this widget shows.
    pub(crate) fn set_host_session_status(&mut self, status: nori_protocol::SessionStatus) {
        if self.host_session_status != Some(status) {
            self.host_session_status = Some(status);
            self.refresh_host_status();
        }
    }

    pub(super) fn refresh_host_status(&mut self) {
        let Some(connection) = &self.host_connection else {
            return;
        };
        let folder = self
            .follows_host_session()
            .then(|| self.config.cwd.file_name())
            .flatten()
            .map(|name| name.to_string_lossy().into_owned());
        let approvals = self
            .follows_host_session()
            .then(|| format!("Approvals: {}", self.approval_mode_name()));
        let status = host_status_line(
            connection,
            folder.as_deref(),
            &self.config.active_agent,
            self.host_session_status,
            approvals.as_deref(),
        );
        self.bottom_pane.set_host_status(Some(status));
    }

    /// Whether this widget shows a joined host's session, which runs on the
    /// host with no harness in this process.
    pub(crate) fn follows_host_session(&self) -> bool {
        self.harness_handle.is_none()
            && self.host_connection.is_some()
            && self.managed_conversation_id.is_some()
    }

    /// The surface command availability is judged for.
    pub(super) fn command_surface(&self) -> crate::command_availability::Surface {
        match &self.host_connection {
            None => crate::command_availability::Surface::Owned {
                cloud_mode: self.cloud_mode,
                agent_session_close: self.session_agent_capabilities.session_close,
            },
            Some(connection) => crate::command_availability::Surface::Joined {
                watching: connection.role == ControlRole::Watcher,
                connected: connection.connected && connection.stopped.is_none(),
            },
        }
    }

    /// Why typed input cannot go to a joined host now.
    pub(super) fn host_input_refusal(&self) -> Option<String> {
        let connection = self.host_connection.as_ref()?;
        if let Some(reason) = &connection.stopped {
            return Some(reason.clone());
        }
        if !connection.connected {
            return Some(crate::command_availability::RECONNECTING_INPUT.to_string());
        }
        (connection.role == ControlRole::Watcher).then(|| WATCHER_INPUT.to_string())
    }

    /// `/cancel`: interrupt the turn on screen, whoever started it.
    pub(super) fn cancel_host_turn(&mut self) {
        if let Some(refusal) = self.host_input_refusal() {
            self.add_error_message(format!("{CANCEL_COMMAND}: {refusal}"));
            return;
        }
        if self.can_control_external_turn() {
            self.request_external_cancel();
        } else if !self.interrupt_host_prompt() {
            self.add_info_message("Nothing is running in this session.".to_string(), None);
        }
    }

    /// Alt+T. False when this terminal owns its host and the key is not ours.
    pub(super) fn take_host_control(&mut self) -> bool {
        let Some(connection) = &self.host_connection else {
            return false;
        };
        if connection.stopped.is_some() {
            return true;
        }
        if connection.connected {
            self.app_event_tx.send(AppEvent::TakeHostControl);
        } else {
            self.add_info_message(TAKE_CONTROL_RECONNECTING.to_string(), None);
        }
        true
    }
}

/// The footer's first segment on a joined host: this terminal's standing,
/// the session's agent, and the key that changes or leaves it.
fn host_status_line(
    connection: &HostConnection,
    folder: Option<&str>,
    agent: &str,
    session: Option<nori_protocol::SessionStatus>,
    approvals: Option<&str>,
) -> Line<'static> {
    use ratatui::style::Stylize;
    let (standing, rest): (ratatui::text::Span<'static>, Vec<&str>) =
        if connection.stopped.is_some() {
            ("DISCONNECTED".red().bold(), vec!["ctrl+d detach"])
        } else if !connection.connected {
            (
                "RECONNECTING".cyan().bold(),
                vec!["draft kept", "ctrl+d detach"],
            )
        } else if connection.role == ControlRole::Watcher {
            (
                "watching · read-only".cyan(),
                vec!["alt+t take control", "ctrl+d detach"],
            )
        } else {
            ("controlling".green(), vec!["ctrl+d detach"])
        };
    let mut spans = vec![standing];
    if let Some(folder) = folder {
        spans.push(format!(" · {folder}").dim());
    }
    if !agent.is_empty() {
        spans.push(format!(" · {agent}").dim());
    }
    if let Some(status) = session {
        spans.push(format!(" · {}", crate::session_status::status_label(status)).dim());
    }
    if let Some(approvals) = approvals {
        spans.push(format!(" · {approvals}").dim());
    }
    for hint in rest {
        spans.push(format!(" · {hint}").dim());
    }
    Line::from(spans)
}
