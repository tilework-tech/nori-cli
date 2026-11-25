//! `/new DIR` in the plain terminal: a session on the current agent started
//! by the host in a folder the widget has already checked exists.

use std::path::PathBuf;

use super::App;
use super::host_candidate::PickerPurpose;

impl App {
    pub(super) fn new_session_in(
        &mut self,
        folder: PathBuf,
        frame_requester: crate::tui::FrameRequester,
    ) {
        if self.chat_widget.has_harness_session() && self.refuse_at_session_cap() {
            return;
        }
        if self.new_session_return.is_some() || self.host_start_holds_input() {
            self.chat_widget.add_info_message(
                "A session is already starting; run /new again once it is on screen.".into(),
                None,
            );
            return;
        }
        let agent_name = self.config.active_agent.clone();
        let display_name = nori_harness::get_agent_display_name(&agent_name);
        if self.chat_widget.has_harness_session() {
            self.begin_host_picker(agent_name, display_name, PickerPurpose::New, Some(folder));
            self.show_pending_new_session(frame_requester);
        } else {
            // This is the session a deferred spawn would have started.
            self.deferred_spawn_pending = false;
            self.begin_host_picker(
                agent_name,
                display_name,
                PickerPurpose::Startup,
                Some(folder),
            );
        }
    }

    /// The joined host's folders for a `/new` Tab, kept only while the
    /// session that asked is on screen.
    pub(super) fn on_host_folders_found(
        &mut self,
        generation: crate::app_event::SessionGeneration,
        text: &str,
        found: serde_json::Value,
    ) {
        if generation == self.chat_widget.session_generation() {
            self.chat_widget.on_host_folders(text, found);
        }
    }
}
