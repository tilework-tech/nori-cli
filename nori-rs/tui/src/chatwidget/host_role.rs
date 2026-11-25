//! This terminal's role on a host it joined. A terminal that owns its host
//! has no role to follow and always controls.

use super::*;
use crate::host_client::HostConnection;
use nori_protocol::ControlRole;

/// Why a watcher's prompt stays in the composer.
pub(crate) const WATCHER_INPUT: &str = "Watching: another terminal controls this host. Press Alt+T to take control; your draft is kept.";
const TAKE_CONTROL_RECONNECTING: &str =
    "Reconnecting. Take control after the connection is restored.";

impl ChatWidget {
    /// Follow a joined host's standing for this terminal.
    pub(crate) fn set_host_connection(&mut self, connection: HostConnection) {
        self.host_connection = Some(connection);
        self.refresh_builtin_command_availability();
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
