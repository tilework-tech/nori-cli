//! What the App shows when its host's standing changes.

use super::App;
use super::host_link::HostUpdate;
use crate::host_client::HostConnection;
use nori_protocol::ControlRole;

impl App {
    /// `connection` is the link's standing after `update`. Only a joined
    /// host sends updates; an owned one always leaves this terminal in control.
    pub(super) fn on_host_update(&mut self, update: HostUpdate, connection: HostConnection) {
        let connection_role = connection.role;
        self.chat_widget.set_host_connection(connection);
        match update {
            HostUpdate::Ready { reconnected: false } => {}
            HostUpdate::Ready { reconnected: true } => {
                let message = match connection_role {
                    ControlRole::Controller => "Reconnected to the host.",
                    ControlRole::Watcher => {
                        "Reconnected to the host. Watching; Alt+T takes control."
                    }
                };
                self.chat_widget.add_info_message(message.to_string(), None);
            }
            HostUpdate::RoleChanged(ControlRole::Controller) => self
                .chat_widget
                .add_info_message("This terminal now controls the host.".to_string(), None),
            HostUpdate::RoleChanged(ControlRole::Watcher) => self.chat_widget.add_info_message(
                "Another terminal took control. Watching; Alt+T takes it back.".to_string(),
                None,
            ),
            HostUpdate::Disconnected(reason) => self.chat_widget.add_error_message(format!(
                "Lost the connection to the host ({reason}). Reconnecting; your draft is kept."
            )),
            HostUpdate::Stopped(reason) => self.chat_widget.add_error_message(reason),
        }
    }
}
