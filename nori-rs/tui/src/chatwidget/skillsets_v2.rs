//! Skillset picker flow for skillsets v2 (`[features] skillsets_v2`): list,
//! layer, and unlayer through the `nori-skillsets` crate. Without the feature
//! the v1 flow in `pickers.rs` runs instead.

use super::*;
use crate::nori::skillset_picker_v2;
use crate::nori::skillset_picker_v2::SkillsetProject;

impl ChatWidget {
    /// `/switch-skillset` with skillsets v2: list skillsets for the session's
    /// skillset project and open the picker.
    pub(crate) fn handle_switch_skillset_command_v2(&mut self) {
        let Some(project) = self.skillset_project_or_report() else {
            return;
        };
        let tx = self.app_event_tx.clone();
        tokio::spawn(async move {
            let result = skillset_picker_v2::list_skillsets(project.path().to_path_buf()).await;
            tx.send(AppEvent::SkillsetV2ListResult { project, result });
        });
    }

    /// `/switch-skillset <name>` with skillsets v2: layer the named skillset
    /// without showing the picker.
    pub(crate) fn handle_switch_skillset_command_with_name_v2(&mut self, name: &str) {
        let Some(project) = self.skillset_project_or_report() else {
            return;
        };
        self.on_change_skillset_request(name.to_string(), project.path().to_path_buf(), true);
    }

    /// The session's skillset project, or `None` after reporting why skillsets
    /// cannot be used. Dismisses the picker flow so a deferred per-session
    /// agent spawn still starts.
    fn skillset_project_or_report(&mut self) -> Option<SkillsetProject> {
        let problem = match nori_skillsets::SkillsetsCli::resolve() {
            Err(error) => error.to_string(),
            Ok(_) => match skillset_picker_v2::current_skillset_project(&self.config.cwd) {
                Some(project) => return Some(project),
                None => "Cannot find a home directory to layer skillsets into.".to_string(),
            },
        };
        self.add_error_message(problem);
        self.app_event_tx.send(AppEvent::SkillsetPickerDismissed);
        None
    }

    /// Handle the result of listing skillsets.
    ///
    /// When there is nothing to pick (an error or an empty listing) the picker
    /// flow is dismissed so a deferred per-session agent spawn still starts.
    pub(crate) fn on_skillset_v2_list_result(
        &mut self,
        project: SkillsetProject,
        result: Result<nori_skillsets::SkillsetList, String>,
    ) {
        if let Ok(nori_skillsets::SkillsetList {
            warning: Some(warning),
            ..
        }) = &result
        {
            self.add_to_history(history_cell::new_warning_event(format!(
                "Showing layered skillsets only; published releases could not be read: {warning}"
            )));
        }
        match result {
            Ok(list) if !list.skillsets.is_empty() => {
                // "No Skillset" and the worktree warning only matter in the
                // per-session startup flow.
                let is_per_session = self.config.skillset_per_session;
                let auto_worktree_off = is_per_session && !self.config.auto_worktree.is_enabled();

                let on_dismiss: SelectionAction = Box::new(|tx| {
                    tx.send(AppEvent::SkillsetPickerDismissed);
                });
                let params = skillset_picker_v2::skillset_picker_params(
                    list,
                    project,
                    Some(on_dismiss),
                    is_per_session,
                    auto_worktree_off,
                );
                self.bottom_pane.show_selection_view(params);
            }
            Ok(list) => {
                self.add_info_message(skillset_picker_v2::empty_list_message(&list), None);
                self.app_event_tx.send(AppEvent::SkillsetPickerDismissed);
            }
            Err(error) => {
                self.add_error_message(format!("Failed to list skillsets: {error}"));
                self.app_event_tx.send(AppEvent::SkillsetPickerDismissed);
            }
        }
    }

    /// Layer (`layer == true`) or unlayer `name` in `project`.
    pub(crate) fn on_change_skillset_request(
        &mut self,
        name: String,
        project: PathBuf,
        layer: bool,
    ) {
        let tx = self.app_event_tx.clone();
        tokio::spawn(async move {
            let result = skillset_picker_v2::change_skillset(project, name.clone(), layer).await;
            tx.send(AppEvent::SkillsetChangeResult {
                name,
                layer,
                result,
            });
        });
    }

    /// Handle the result of layering or unlayering a skillset.
    pub(crate) fn on_skillset_change_result(
        &mut self,
        name: &str,
        layer: bool,
        result: Result<nori_skillsets::LayerChange, String>,
    ) {
        match result {
            Ok(change) => {
                self.add_to_history(history_cell::new_skillset_changed_event(
                    name,
                    layer,
                    &change.layers,
                ));
                for note in change.notes {
                    self.add_to_history(history_cell::new_warning_event(note));
                }
                self.request_redraw();
            }
            Err(message) => {
                let verb = if layer { "layer" } else { "unlayer" };
                self.add_error_message(format!("Failed to {verb} skillset '{name}': {message}"));
            }
        }
    }
}
