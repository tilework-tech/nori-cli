//! The App's event loop, shared by a host this terminal owns and one it
//! joined: terminal input, the App's own events, and the host's updates,
//! then teardown once the loop ends.

use color_eyre::eyre::Result;
use tokio::select;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio_stream::StreamExt;

use super::App;
use super::AppExitInfo;
use super::host_link::HostLink;
use crate::app_event::AppEvent;
use crate::tui;

impl App {
    pub(super) async fn drive(
        mut self,
        tui: &mut tui::Tui,
        mut app_event_rx: UnboundedReceiver<AppEvent>,
        host: &mut HostLink,
    ) -> Result<AppExitInfo> {
        let tui_events = tui.event_stream();
        tokio::pin!(tui_events);

        self.request_system_info_refresh(
            self.config.cwd.clone(),
            Some(self.config.active_agent.clone()),
            self.chat_widget.first_prompt_text(),
        );

        tui.frame_requester().schedule_frame();

        let ran = async {
            while select! {
                Some(event) = app_event_rx.recv() => {
                    self.handle_event(tui, event, host).await?
                }
                Some(event) = tui_events.next() => {
                    self.handle_tui_event(tui, event).await?
                }
                Some(event) = host.next() => {
                    let update = host.handle(event).map_err(|error| color_eyre::eyre::eyre!(error))?;
                    if let Some(update) = update {
                        self.on_host_update(update, tui, host);
                        tui.frame_requester().schedule_frame();
                    }
                    true
                }
            } {
                self.follow_foreground_session(host);
                self.report_program_status();
            }
            Ok::<(), color_eyre::eyre::Report>(())
        }
        .await;

        self.clear_program_status();
        if let Err(error) = self.return_inline_for_exit(tui) {
            tracing::warn!(%error, "could not return to the inline view on exit");
        }
        self.cancel_primary_agent_preparation();
        if let Some(agent) = self.prepared_agent.take() {
            agent.shutdown().await;
        }
        self.prepared_agent_initial_context = None;
        self.discard_candidate();
        ran?;

        // Don't clear terminal to allow exit message to remain visible
        // tui.terminal.clear()?;

        Ok(AppExitInfo {
            token_usage: self.token_usage(),
            conversation_id: self.chat_widget.conversation_id(),
            conversation_has_activity: self.chat_widget.session_stats().has_activity(),
            update_action: self.pending_update_action,
        })
    }
}
