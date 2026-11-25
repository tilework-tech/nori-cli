//! What the App shows when its host's standing changes.

use super::App;
use super::host_link::HostLink;
use super::host_link::HostUpdate;
use crate::host_client::HostConnection;
use crate::tui;
use nori_protocol::ControlRole;

impl App {
    /// Only a joined host sends updates; an owned one always leaves this
    /// terminal in control.
    pub(super) fn on_host_update(
        &mut self,
        update: HostUpdate,
        tui: &mut tui::Tui,
        host: &mut HostLink,
    ) {
        let connection = host.connection();
        match update {
            HostUpdate::Sessions(list) => self.on_joined_sessions(list, tui, &connection),
            HostUpdate::Session(params) => self.on_joined_session_event(params),
            HostUpdate::Ready { reconnected } => {
                let role = connection.role;
                self.apply_host_connection(connection);
                if reconnected {
                    let message = match role {
                        ControlRole::Controller => "Reconnected to the host.",
                        ControlRole::Watcher => {
                            "Reconnected to the host. Watching; Alt+T takes control."
                        }
                    };
                    self.chat_widget.add_info_message(message.to_string(), None);
                }
            }
            HostUpdate::RoleChanged(role) => {
                self.apply_host_connection(connection);
                let message = match role {
                    ControlRole::Controller => "This terminal now controls the host.",
                    ControlRole::Watcher => {
                        "Another terminal took control. Watching; Alt+T takes it back."
                    }
                };
                self.chat_widget.add_info_message(message.to_string(), None);
            }
            HostUpdate::Disconnected(reason) => {
                self.apply_host_connection(connection);
                self.joined_connection_lost();
                self.chat_widget.add_error_message(format!(
                    "Lost the connection to the host ({reason}). Reconnecting; your draft is kept."
                ));
            }
            HostUpdate::Stopped(reason) => {
                self.apply_host_connection(connection);
                self.chat_widget.add_error_message(reason);
            }
        }
    }

    /// Every session this terminal shows follows the host's standing, so a
    /// session brought on screen later already knows it.
    fn apply_host_connection(&mut self, connection: HostConnection) {
        for (_, parked) in self.session_deck.parked_sessions_mut() {
            parked.widget.set_host_connection(connection.clone());
        }
        self.chat_widget.set_host_connection(connection);
    }
}
