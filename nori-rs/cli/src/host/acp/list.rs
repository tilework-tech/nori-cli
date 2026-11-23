//! `session/list`: the host's live sessions, then the sessions recorded on
//! disk that it is not running, newest first.

use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;

use chrono::SecondsFormat;
use serde_json::Value;
use serde_json::json;

use super::AcpError;
use crate::host::service::HostService;
use crate::host::transcripts;

/// Sessions per page.
const PAGE: usize = 50;

/// Where the next page starts. Cursors are opaque to clients.
enum Cursor {
    Live(usize),
    Stored { nanos: i128, id: String },
}

impl Cursor {
    fn parse(cursor: &str) -> Option<Self> {
        if let Some(index) = cursor.strip_prefix("live:") {
            return index.parse().ok().map(Self::Live);
        }
        let (nanos, id) = cursor.strip_prefix("stored:")?.split_once(':')?;
        Some(Self::Stored {
            nanos: nanos.parse().ok()?,
            id: id.to_owned(),
        })
    }
}

fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn rfc3339(time: std::time::SystemTime) -> String {
    chrono::DateTime::<chrono::Utc>::from(time).to_rfc3339_opts(SecondsFormat::Millis, true)
}

impl HostService {
    pub(super) async fn list_sessions(&self, params: &Value) -> Result<Value, AcpError> {
        let folder = match params.get("cwd") {
            None | Some(Value::Null) => None,
            Some(Value::String(cwd)) if Path::new(cwd).is_absolute() => {
                Some(canonical(Path::new(cwd)))
            }
            Some(_) => return Err(AcpError::invalid_params("cwd must be an absolute path")),
        };
        let cursor = match params.get("cursor") {
            None | Some(Value::Null) => Cursor::Live(0),
            Some(cursor) => cursor
                .as_str()
                .and_then(Cursor::parse)
                .ok_or_else(|| AcpError::invalid_params("cursor is not one this host issued"))?,
        };
        let in_folder = |cwd: &Path| {
            folder
                .as_ref()
                .is_none_or(|folder| canonical(cwd) == *folder)
        };
        let records = self.manager.list();
        let running = records
            .iter()
            .map(|record| record.conversation_id.to_string())
            .collect::<HashSet<_>>();
        let mut rows = Vec::new();
        // Resuming after the live rows: the stored rows start from the top.
        let mut live_end = None;
        if let Cursor::Live(start) = cursor {
            let live = records
                .iter()
                .filter(|record| in_folder(&record.cwd))
                .collect::<Vec<_>>();
            for record in live.iter().skip(start) {
                if rows.len() == PAGE {
                    return Ok(json!({
                        "sessions": rows,
                        "nextCursor": format!("live:{}", start + rows.len()),
                    }));
                }
                let replay = self.manager.replay_info(&record.conversation_id);
                let updated = replay.as_ref().map(|info| {
                    std::time::SystemTime::now()
                        - std::time::Duration::from_secs(info.idle_seconds.max(0) as u64)
                });
                rows.push(json!({
                    "sessionId": record.conversation_id.to_string(),
                    "cwd": record.cwd,
                    "title": replay.as_ref().and_then(|info| info.title.clone()),
                    "updatedAt": updated.map(rfc3339),
                }));
            }
            live_end = Some(start.max(live.len()));
        }
        let after = match cursor {
            Cursor::Live(_) => None,
            Cursor::Stored { nanos, id } => Some((nanos, id)),
        };
        let mut stored = transcripts::stored_sessions(&self.config.nori_home, &running, after)
            .await
            .into_iter();
        let mut last = None;
        for session in stored.by_ref() {
            if rows.len() == PAGE {
                let next = match (&last, live_end) {
                    (Some((nanos, id)), _) => format!("stored:{nanos}:{id}"),
                    (None, Some(end)) => format!("live:{end}"),
                    (None, None) => unreachable!("a stored page starts after a stored row"),
                };
                return Ok(json!({"sessions": rows, "nextCursor": next}));
            }
            let key = (
                transcripts::nanos(session.updated_at),
                session.session_id.clone(),
            );
            let Some(row) = transcripts::stored_row(&session).await else {
                continue;
            };
            if !row["cwd"]
                .as_str()
                .is_some_and(|cwd| in_folder(Path::new(cwd)))
            {
                continue;
            }
            rows.push(row);
            last = Some(key);
        }
        Ok(json!({"sessions": rows}))
    }
}
