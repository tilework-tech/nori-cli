//! `_nori/session/find_paths`: `@` file mentions and `/new` folder
//! completion, answered from the host's filesystem so a client on another
//! machine sees the session's own folder.

use std::num::NonZeroUsize;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use anyhow::Context;
use nori_harness::ConversationId;
use nori_protocol::FindPathsParams;
use nori_protocol::FindPathsResult;
use nori_protocol::PathKind;
use nori_protocol::PathMatch;
use serde_json::Value;

use super::AcpError;
use crate::host::service::HostService;

pub(in crate::host) const FIND_PATHS: &str = "_nori/session/find_paths";

const DEFAULT_LIMIT: i32 = 20;
const MAX_LIMIT: i32 = 50;
/// Searched for before matches escaping the root are dropped, so dropping
/// them rarely leaves fewer than `limit`.
const SEARCH_LIMIT: NonZeroUsize = NonZeroUsize::new(MAX_LIMIT as usize).unwrap();
const SEARCH_THREADS: NonZeroUsize = NonZeroUsize::new(2).unwrap();

impl HostService {
    pub(super) async fn find_paths(&self, params: Value) -> Result<Value, AcpError> {
        let FindPathsParams {
            session_id,
            query,
            kind,
            limit,
        } = serde_json::from_value(params).map_err(AcpError::invalid_params)?;
        let root = match session_id {
            Some(id) => ConversationId::from_string(&id)
                .ok()
                .filter(|id| !self.is_deleted(id))
                .and_then(|id| self.manager.get(&id))
                .map(|record| record.cwd)
                .ok_or_else(|| AcpError::invalid_params("unknown session"))?,
            None => self.config.cwd.clone(),
        };
        let limit = usize::try_from(limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT))
            .map_err(AcpError::internal)?;
        let result = tokio::task::spawn_blocking(move || search(root, &query, kind, limit))
            .await
            .map_err(AcpError::internal)??;
        serde_json::to_value(result).map_err(AcpError::internal)
    }
}

/// `typed` as a folder on this host: `~` and `~/…` are under the host's
/// home and other relative folders under `base`. `~user` is left as typed,
/// relative, and so names no folder.
pub(in crate::host) fn host_folder(typed: &str, base: &Path) -> PathBuf {
    let home = || dirs::home_dir().unwrap_or_default();
    match typed.strip_prefix('~') {
        Some("") => home(),
        Some(rest) if rest.starts_with('/') => home().join(rest.trim_start_matches('/')),
        Some(_) => PathBuf::from(typed),
        None => base.join(typed),
    }
}

/// At most `limit` matches for `query` under `root`, which is reported
/// canonicalized.
fn search(
    root: PathBuf,
    query: &str,
    kind: PathKind,
    limit: usize,
) -> anyhow::Result<FindPathsResult> {
    let root = root
        .canonicalize()
        .with_context(|| format!("could not open {}", root.display()))?;
    let matches = match kind {
        PathKind::Files if query.is_empty() => Vec::new(),
        PathKind::Files => {
            let found = codex_file_search::run(
                query,
                SEARCH_LIMIT,
                &root,
                vec![".git".to_owned()],
                SEARCH_THREADS,
                Arc::new(AtomicBool::new(false)),
                true,
                true,
            )?;
            found
                .matches
                .into_iter()
                // The search follows links, which may lead out of the root.
                .filter(|found| {
                    root.join(&found.path)
                        .canonicalize()
                        .is_ok_and(|path| path.starts_with(&root))
                })
                .take(limit)
                .map(|found| PathMatch {
                    path: found.path,
                    indices: found.indices.map(|indices| {
                        indices
                            .into_iter()
                            .filter_map(|index| i32::try_from(index).ok())
                            .collect()
                    }),
                })
                .collect()
        }
        PathKind::Folders => {
            // Matches the terminal's own `/new` completion, so a folder
            // completes the same way on a local and a remote host.
            let (parent, prefix) = query
                .rsplit_once('/')
                .map_or(("", query), |(parent, prefix)| {
                    (&query[..=parent.len()], prefix)
                });
            let directory = host_folder(parent, &root);
            let mut names: Vec<String> = directory
                .is_absolute()
                .then(|| std::fs::read_dir(directory.components().collect::<PathBuf>()))
                .into_iter()
                .flatten()
                .flatten()
                .flatten()
                .filter(|entry| entry.path().is_dir())
                .filter_map(|entry| entry.file_name().into_string().ok())
                // `/new` arguments split on whitespace, so such names cannot be typed.
                .filter(|name| !name.contains(char::is_whitespace))
                .filter(|name| {
                    name.starts_with(prefix) && (prefix.starts_with('.') || !name.starts_with('.'))
                })
                .collect();
            names.sort();
            names
                .into_iter()
                .take(limit)
                .map(|name| PathMatch {
                    path: format!("{parent}{name}/"),
                    indices: None,
                })
                .collect()
        }
    };
    Ok(FindPathsResult { root, matches })
}
