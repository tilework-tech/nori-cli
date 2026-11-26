//! Starting a failed session again, in its place in the session list.

use color_eyre::eyre::Result;
use nori_protocol::SessionStatus;

use super::App;
use crate::app_event::SessionGeneration;
use crate::chatwidget::ChatWidget;
use crate::tui;

impl App {
    /// Resume a failed session from its transcript on a new agent process of
    /// the same agent. The resumed session takes the failed one's place in the
    /// navigation order and comes on screen.
    pub(super) async fn resume_failed_session(
        &mut self,
        tui: &mut tui::Tui,
        generation: SessionGeneration,
    ) -> Result<()> {
        self.show_session(generation);
        if self.chat_widget.session_generation() != generation {
            return Ok(());
        }
        let Some(record) = self
            .chat_widget
            .managed_conversation_id()
            .and_then(|id| self.session_manager.get(&id))
            .filter(|record| record.status == SessionStatus::Failed)
        else {
            return Ok(());
        };
        let title = self.session_title(generation, super::session_overview::WHOLE_NAME);
        let mut config = self.config_for_agent(&record.agent);
        config.cwd.clone_from(&record.cwd);
        let session = match self
            .session_manager
            .resume(
                &record.conversation_id,
                crate::chatwidget::agent::agent_prepare_spec(config.clone(), None),
            )
            .await
        {
            Ok(session) => session,
            Err(error) => {
                self.chat_widget
                    .add_error_message(format!("Couldn't resume this session: {error}"));
                return Ok(());
            }
        };
        let mut init = self.chat_widget_init(
            tui.frame_requester(),
            None,
            Vec::new(),
            Some(record.agent.clone()),
            false,
            None,
        );
        init.config = config;
        self.begin_replacing_session_transcript(tui)?;
        let widget = ChatWidget::from_managed_session(init, session);
        if let Some(title) = title {
            self.session_previews
                .inherit_title(widget.session_generation(), title);
        }
        self.install_active_widget(widget);
        self.configure_new_chat_widget();
        self.chat_widget.add_info_message(
            "Resuming this session on a new agent process...".to_string(),
            None,
        );
        tui.frame_requester().schedule_frame();
        Ok(())
    }
}
