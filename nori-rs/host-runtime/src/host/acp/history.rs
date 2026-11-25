//! `_nori/host/history/*`: the prompts terminals send, kept in the host's
//! own `NORI_HOME`, so every terminal on the host recalls and searches the
//! same list wherever its own home is. Recording is the controller's, for a
//! session the host knows; anyone may read.

use nori_harness::ConversationId;
use nori_harness::HistoryEntry;
use nori_harness::message_history;
use nori_protocol::HistoryAddParams;
use nori_protocol::HistoryEntryParams;
use nori_protocol::HistoryEntryResult;
use nori_protocol::HistoryMetadata;
use nori_protocol::HistoryRecord;
use nori_protocol::HistorySearchParams;
use nori_protocol::HistorySearchResult;
use serde_json::Value;
use serde_json::json;

use super::AcpError;
use crate::host::service::HostService;

pub(in crate::host) const ADD: &str = "_nori/host/history/add";
pub(in crate::host) const ENTRY: &str = "_nori/host/history/entry";
pub(in crate::host) const SEARCH: &str = "_nori/host/history/search";
pub(in crate::host) const METADATA: &str = "_nori/host/history/metadata";

impl HostService {
    pub(super) async fn history(
        &self,
        client: i64,
        method: &str,
        params: Value,
    ) -> Result<Value, AcpError> {
        let home = self.config.nori_home.clone();
        match method {
            ADD => {
                if !self.acp.connection(client)?.is_controller() {
                    return Err(AcpError {
                        code: -32016,
                        message: "watcher is read-only; take control before changing a session"
                            .into(),
                    });
                }
                let HistoryAddParams { session_id, text } =
                    serde_json::from_value(params).map_err(AcpError::invalid_params)?;
                let session = ConversationId::from_string(&session_id)
                    .ok()
                    .filter(|session| self.manager.get(session).is_some())
                    .ok_or_else(|| AcpError {
                        code: -32002,
                        message: "unknown session id".into(),
                    })?;
                message_history::append_entry(
                    &text,
                    &session,
                    &home,
                    self.config.history_persistence,
                )
                .await
                .map_err(AcpError::internal)?;
                Ok(json!({}))
            }
            ENTRY => {
                let HistoryEntryParams { log_id, offset } =
                    serde_json::from_value(params).map_err(AcpError::invalid_params)?;
                let (Ok(log_id), Ok(offset)) = (u64::try_from(log_id), usize::try_from(offset))
                else {
                    return Err(AcpError::invalid_params(
                        "logId and offset must not be negative",
                    ));
                };
                let entry = tokio::task::spawn_blocking(move || {
                    message_history::lookup(log_id, offset, &home)
                })
                .await
                .map_err(AcpError::internal)?;
                to_value(HistoryEntryResult {
                    entry: entry.map(record),
                })
            }
            SEARCH => {
                let HistorySearchParams { max_results } =
                    serde_json::from_value(params).map_err(AcpError::invalid_params)?;
                let max_results = usize::try_from(max_results)
                    .map_err(|_| AcpError::invalid_params("maxResults must not be negative"))?;
                let entries = tokio::task::spawn_blocking(move || {
                    message_history::search_entries(&home, max_results)
                })
                .await
                .map_err(AcpError::internal)?;
                to_value(HistorySearchResult {
                    entries: entries.into_iter().map(record).collect(),
                })
            }
            METADATA => {
                let (log_id, entry_count) = message_history::history_metadata(&home).await;
                to_value(HistoryMetadata {
                    log_id: i64::try_from(log_id).map_err(AcpError::internal)?,
                    entry_count: i64::try_from(entry_count).map_err(AcpError::internal)?,
                })
            }
            _ => Err(AcpError {
                code: -32601,
                message: "method not found".into(),
            }),
        }
    }
}

fn record(entry: HistoryEntry) -> HistoryRecord {
    HistoryRecord {
        session_id: entry.session_id,
        ts: i64::try_from(entry.ts).unwrap_or(i64::MAX),
        text: entry.text,
    }
}

fn to_value(result: impl serde::Serialize) -> Result<Value, AcpError> {
    serde_json::to_value(result).map_err(AcpError::internal)
}
