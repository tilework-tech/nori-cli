//! The context report of a session on the host comes from
//! `_nori/session/context`, so `/context` shows that session's usage and the
//! instruction files on the host's filesystem, and the footer shows the
//! session's latest usage as soon as it comes on screen, even when the
//! update that carried it has left the replay window.

use super::*;
use crate::host_client::HostCall;
use crate::presentation::session_runtime::SessionUsageState;

impl ChatWidget {
    fn host_context_session(&self) -> Option<String> {
        self.managed_conversation_id
            .filter(|_| !self.cloud_mode)
            .map(|id| id.to_string())
    }

    /// Ask the host for this widget's session report: the full report when
    /// the user asked, only the usage when the session came on screen.
    /// False for a widget without a host session.
    pub(super) fn request_host_context(&self, asked: bool) -> bool {
        let Some(session_id) = self.host_context_session() else {
            return false;
        };
        let tx = self.app_event_tx.clone();
        let reply = tx.host_receipt(HostCall::SessionContext {
            session_id: session_id.clone(),
            skip_instructions: !asked,
        });
        tokio::spawn(async move {
            let result = reply
                .await
                .and_then(|reply| {
                    serde_json::from_value::<nori_protocol::ContextReport>(reply)
                        .map_err(Into::into)
                })
                .map_err(|error| error.to_string());
            tx.send(AppEvent::HostContextLoaded {
                session_id,
                result,
                asked,
            });
        });
        true
    }

    /// A host session's report arrived. Only the widget showing that session
    /// keeps it. Its usage fills the footer only when no live reading has
    /// arrived since, and the report is printed only when the user asked.
    pub(crate) fn on_host_context_loaded(
        &mut self,
        session_id: &str,
        result: Result<nori_protocol::ContextReport, String>,
        asked: bool,
    ) {
        if self.host_context_session().as_deref() != Some(session_id) {
            return;
        }
        let report = match result {
            Ok(report) => report,
            Err(error) if asked => {
                self.add_error_message(format!("Could not read context snapshot: {error}"));
                return;
            }
            Err(error) => {
                tracing::debug!(%error, "context unavailable from the host");
                return;
            }
        };
        if let Some(usage) = report.usage
            && !self.bottom_pane.has_session_usage()
        {
            self.bottom_pane.set_session_usage(Some(SessionUsageState {
                used_tokens: usage.used,
                total_tokens: usage.size,
                cost_display: None,
            }));
        }
        if asked {
            self.insert_context_output(report.into());
        }
    }
}
