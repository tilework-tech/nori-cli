//! Prompt history of a session on the host goes through
//! `_nori/host/history/*`, so every terminal on that host records into,
//! recalls from and searches one history in the host's own home. The
//! widget relearns the history's size when its session is configured and
//! after each turn, so prompts another terminal sent are recalled too. Up
//! still fetches one entry at a time, and only when it is not yet cached.

use super::*;
use crate::app_event::HarnessAction;
use crate::app_event_sender::HostReply;
use crate::host_client::HostCall;

impl ChatWidget {
    /// Sends a history action to the host when this widget shows a host
    /// session; any other action, or any widget without one, is returned
    /// for the harness.
    pub(super) fn history_action_on_host(&self, action: HarnessAction) -> Option<HarnessAction> {
        let Some(session_id) = self
            .managed_conversation_id
            .filter(|_| !self.cloud_mode)
            .map(|id| id.to_string())
        else {
            return Some(action);
        };
        match action {
            HarnessAction::AddHistory(text) => self.app_event_tx.send(AppEvent::HostRequest {
                call: HostCall::AddHistory { session_id, text },
                reply: HostReply::ReportError(self.app_event_tx.clone()),
            }),
            HarnessAction::HistoryEntry { log_id, offset } => {
                self.ask_host_for_history(
                    HostCall::HistoryEntry { log_id, offset },
                    move |reply| {
                        let result: nori_protocol::HistoryEntryResult =
                            serde_json::from_value(reply)?;
                        Ok(AppEvent::HistoryEntryLoaded {
                            log_id,
                            offset,
                            entry: result.entry.map(harness_entry),
                        })
                    },
                );
            }
            HarnessAction::SearchHistory { max_results } => {
                let max_results = i32::try_from(max_results).unwrap_or(i32::MAX);
                self.ask_host_for_history(HostCall::SearchHistory { max_results }, |reply| {
                    let result: nori_protocol::HistorySearchResult = serde_json::from_value(reply)?;
                    Ok(AppEvent::HistorySearchLoaded(
                        result.entries.into_iter().map(harness_entry).collect(),
                    ))
                });
            }
            other => return Some(other),
        }
        None
    }

    /// Relearn how many prompts the host's history holds, for a widget that
    /// shows a host session. A host that cannot answer leaves the widget
    /// with the size it last knew, which is no error to report.
    pub(super) fn refresh_host_history(&self) {
        if self.cloud_mode || self.managed_conversation_id.is_none() {
            return;
        }
        let tx = self.app_event_tx.clone();
        let reply = tx.host_receipt(HostCall::HistoryMetadata);
        tokio::spawn(async move {
            let metadata = reply.await.and_then(|reply| {
                serde_json::from_value::<nori_protocol::HistoryMetadata>(reply).map_err(Into::into)
            });
            match metadata {
                Ok(nori_protocol::HistoryMetadata {
                    log_id,
                    entry_count,
                }) => tx.send(AppEvent::HistoryMetadataLoaded {
                    log_id,
                    entry_count,
                }),
                Err(error) => tracing::debug!(%error, "prompt history size unavailable"),
            }
        });
    }

    pub(crate) fn on_history_metadata_loaded(&mut self, log_id: i64, entry_count: i64) {
        let (Ok(log_id), Ok(entry_count)) = (u64::try_from(log_id), usize::try_from(entry_count))
        else {
            return;
        };
        self.bottom_pane
            .refresh_history_metadata(log_id, entry_count);
    }

    fn ask_host_for_history(
        &self,
        call: HostCall,
        landed: impl FnOnce(serde_json::Value) -> anyhow::Result<AppEvent> + Send + 'static,
    ) {
        let tx = self.app_event_tx.clone();
        let reply = tx.host_receipt(call);
        tokio::spawn(async move {
            match reply.await.and_then(landed) {
                Ok(event) => tx.send(event),
                Err(error) => tx.send(AppEvent::HarnessActionFailed(format!(
                    "Could not read prompt history from the host: {error}"
                ))),
            }
        });
    }
}

fn harness_entry(record: nori_protocol::HistoryRecord) -> nori_harness::HistoryEntry {
    nori_harness::HistoryEntry {
        session_id: record.session_id,
        ts: u64::try_from(record.ts).unwrap_or_default(),
        text: record.text,
    }
}
