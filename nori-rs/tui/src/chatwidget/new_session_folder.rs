//! `/new DIR`: start the next session in a folder named from the session on
//! screen, which Tab completes. The host this terminal owns runs on this machine, so the folder
//! is checked here and a missing one never leaves the terminal.

use super::ChatWidget;
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
