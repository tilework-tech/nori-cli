//! `/new DIR`: start the next session in a folder named from the session on
//! screen, which Tab completes. The host this terminal owns runs on this machine, so the folder
//! is checked here and a missing one never leaves the terminal.

use super::ChatWidget;
use crate::app_event::AppEvent;
use crate::slash_command::SlashCommand;

impl ChatWidget {
    /// Whether `text` was `/new` with a folder; it is then handled here.
    pub(super) fn handle_new_in_folder(&mut self, text: &str) -> bool {
        let Some(folder) = text
            .strip_prefix("/new ")
            .map(str::trim)
            .filter(|folder| !folder.is_empty())
        else {
            return false;
        };
        if self.cloud_mode {
            self.add_error_message(
                "/new with a folder is not available for cloud sessions.".into(),
            );
            self.set_composer_text(text.to_owned());
            return true;
        }
        if self.host_connection.is_some() {
            if !self.refuse_new_on_host(text) {
                self.start_host_session(folder);
            }
            return true;
        }
        let path = crate::session_folder::resolve(folder, &self.config.cwd);
        if !path.is_dir() {
            self.add_error_message(format!("No folder at {}", path.display()));
            self.set_composer_text(text.to_owned());
            return true;
        }
        self.dispatch_command_in(SlashCommand::New, Some(path));
        true
    }

    /// Whether the composer held `/new` with a folder, which Tab then
    /// completed like a shell, listing the folders when several match.
    pub(super) fn complete_new_session_folder(&mut self) -> bool {
        let text = self.bottom_pane.composer_text();
        if let Some(connection) = &self.host_connection
            && connection.features.find_paths
        {
            let Some(typed) = crate::session_folder::typed_folder(&text) else {
                return false;
            };
            self.complete_folder_on_host(text.clone(), typed.to_owned());
            return true;
        }
        let Some((completed, candidates)) =
            crate::session_folder::complete(&text, &self.config.cwd)
        else {
            return crate::session_folder::typed_folder(&text).is_some();
        };
        self.set_composer_text(completed);
        if !candidates.is_empty() {
            self.add_info_message(candidates.join("  "), None);
        }
        true
    }
}

/// Folders one Tab on a joined host asks for; fewer back means the list is
/// complete.
const FOLDER_MATCHES: i32 = 50;

impl ChatWidget {
    /// `/new` on a joined host refused while watching or reconnecting; the
    /// typed command stays in the composer.
    fn refuse_new_on_host(&mut self, text: &str) -> bool {
        let surface = self.command_surface();
        let Some(reason) =
            crate::command_availability::unavailable_reason(SlashCommand::New, surface)
        else {
            return false;
        };
        self.add_error_message(format!("/new: {reason}"));
        self.set_composer_text(text.to_owned());
        true
    }

    /// `/new [folder] [agent]` on a joined host: the host starts the session
    /// in a folder named from the session on screen, or from this
    /// terminal's folder, and the App shows it once the host lists it.
    pub(super) fn start_host_session(&mut self, args: &str) {
        self.start_host_session_with(args, None, None);
    }

    /// [`Self::start_host_session`] with `agent`, when the caller chose it,
    /// and `note` added to the session once it is on screen.
    pub(super) fn start_host_session_with(
        &mut self,
        args: &str,
        agent: Option<&str>,
        note: Option<String>,
    ) {
        let Some(features) = self.host_connection.as_ref().map(|c| c.features) else {
            return;
        };
        let cwd = self.config.cwd.clone();
        let base = if self.follows_host_session() {
            crate::host_client::new_session::Base::Session(&cwd)
        } else {
            crate::host_client::new_session::Base::Terminal(&cwd)
        };
        let mut params = match crate::host_client::new_session::params(
            args,
            base,
            features.session_folders,
            features.find_paths,
            features.remote,
        ) {
            Ok(params) => params,
            Err(message) => {
                self.add_error_message(message);
                return;
            }
        };
        if let Some(agent) = agent {
            params["agent"] = serde_json::json!(agent);
        }
        let tx = self.app_event_tx.clone();
        let reply = tx.host_receipt(crate::host_client::HostCall::NewSession(params));
        tokio::spawn(async move {
            match reply.await {
                Ok(started) => match started["session_id"].as_str() {
                    Some(id) => tx.send(AppEvent::JoinedSessionStarted {
                        id: id.to_owned(),
                        note,
                    }),
                    None => tx.send(AppEvent::HarnessActionFailed(
                        "The host did not say which session it started.".into(),
                    )),
                },
                Err(error) => tx.send(AppEvent::HarnessActionFailed(format!(
                    "The host could not start a session: {error}"
                ))),
            }
        });
    }

    /// Tab after `/new` on a joined host: the host lists the folders on its
    /// own disk, and the completion applies if the composer still holds
    /// `text` when they arrive.
    fn complete_folder_on_host(&mut self, text: String, typed: String) {
        let tx = self.app_event_tx.clone();
        let reply = tx.host_receipt(crate::host_client::HostCall::FindPaths {
            session_id: self.managed_conversation_id.map(|id| id.to_string()),
            query: typed,
            kind: nori_protocol::PathKind::Folders,
            limit: FOLDER_MATCHES,
        });
        let generation = self.session_generation;
        tokio::spawn(async move {
            match reply.await {
                Ok(found) => tx.send(AppEvent::HostFoldersFound {
                    generation,
                    text,
                    found,
                }),
                Err(error) => tx.send(AppEvent::HarnessActionFailed(format!(
                    "The host could not list folders: {error}"
                ))),
            }
        });
    }

    /// The host's folders for a Tab typed when the composer held `text`.
    pub(crate) fn on_host_folders(&mut self, text: &str, found: serde_json::Value) {
        if self.bottom_pane.composer_text() != text {
            return;
        }
        let Ok(found) = serde_json::from_value::<nori_protocol::FindPathsResult>(found) else {
            self.add_error_message("The host listed folders in an unknown shape.".into());
            return;
        };
        let complete =
            i32::try_from(found.matches.len()).is_ok_and(|listed| listed < FOLDER_MATCHES);
        let Some(typed) = crate::session_folder::typed_folder(text) else {
            return;
        };
        let Some((completed, candidates)) =
            crate::session_folder::complete_from_host(typed, &found.matches, complete)
        else {
            return;
        };
        self.set_composer_text(completed);
        if !candidates.is_empty() {
            self.add_info_message(candidates.join("  "), None);
        }
    }
}
