//! `@` file mentions answered by a host's `_nori/session/find_paths`.
//!
//! The composer reports every edit of its `@` token. One search is in
//! flight at a time; the newest query typed meanwhile is sent when it
//! lands, and the queries in between are never sent.

use codex_file_search::FileMatch;

use super::HostCall;

/// Matches the `@` popup shows, as many as a local search returns.
pub(crate) const FILE_MATCHES: i32 = 20;

/// One `@` query in one session's folder, or the host's own without one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FileSearch {
    pub session_id: Option<String>,
    pub query: String,
}

impl FileSearch {
    pub fn call(&self) -> HostCall {
        HostCall::FindPaths {
            session_id: self.session_id.clone(),
            query: self.query.clone(),
            kind: nori_protocol::PathKind::Files,
            limit: FILE_MATCHES,
        }
    }
}

#[derive(Default)]
pub(crate) struct HostFileSearch {
    in_flight: bool,
    waiting: Option<FileSearch>,
    /// The search in flight or waiting, so repeating it sends nothing. Once
    /// it lands, the same search asked again is sent again.
    latest: Option<FileSearch>,
}

impl HostFileSearch {
    /// The search to send now for `search`; none while another is in
    /// flight (it waits) or when it repeats the one not yet answered.
    pub fn query(&mut self, search: FileSearch) -> Option<FileSearch> {
        if search.query.is_empty() || self.latest.as_ref() == Some(&search) {
            return None;
        }
        self.latest = Some(search.clone());
        if self.in_flight {
            self.waiting = Some(search);
            return None;
        }
        self.in_flight = true;
        Some(search)
    }

    /// The search in flight was answered or lost; the one that waited for
    /// it, to send now.
    pub fn finished(&mut self) -> Option<FileSearch> {
        let next = self.waiting.take();
        self.in_flight = next.is_some();
        if next.is_none() {
            self.latest = None;
        }
        next
    }
}

/// The popup's matches from a `_nori/session/find_paths` reply.
pub(crate) fn file_matches(reply: serde_json::Value) -> anyhow::Result<Vec<FileMatch>> {
    let result: nori_protocol::FindPathsResult = serde_json::from_value(reply)?;
    Ok(result
        .matches
        .into_iter()
        .map(|found| FileMatch {
            score: 0,
            path: found.path,
            indices: found.indices.map(|indices| {
                indices
                    .into_iter()
                    .filter_map(|index| u32::try_from(index).ok())
                    .collect()
            }),
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use serde_json::json;

    use super::*;

    fn search(session: &str, query: &str) -> FileSearch {
        FileSearch {
            session_id: Some(session.into()),
            query: query.into(),
        }
    }

    #[test]
    fn one_search_is_in_flight_and_only_the_newest_waiting_query_follows() {
        let mut searches = HostFileSearch::default();
        assert_eq!(searches.query(search("one", "m")), Some(search("one", "m")));
        assert_eq!(searches.query(search("one", "ma")), None);
        assert_eq!(searches.query(search("one", "mai")), None);
        assert_eq!(searches.finished(), Some(search("one", "mai")));
        assert_eq!(searches.finished(), None);
        assert_eq!(
            searches.query(search("one", "main")),
            Some(search("one", "main"))
        );
    }

    #[test]
    fn a_query_repeated_before_its_answer_is_sent_once_and_after_it_again() {
        let mut searches = HostFileSearch::default();
        assert_eq!(
            searches.query(search("one", "ma")),
            Some(search("one", "ma"))
        );
        assert_eq!(searches.query(search("one", "ma")), None);
        assert_eq!(searches.finished(), None);
        assert_eq!(
            searches.query(search("one", "ma")),
            Some(search("one", "ma"))
        );
        assert_eq!(searches.finished(), None);
        assert_eq!(
            searches.query(search("two", "ma")),
            Some(search("two", "ma"))
        );
    }

    #[test]
    fn host_matches_become_popup_matches() {
        let matches = file_matches(json!({
            "root": "/work/service",
            "matches": [{"path": "src/main.rs", "indices": [4, 5]}, {"path": "Cargo.toml"}],
        }))
        .unwrap();
        assert_eq!(
            serde_json::to_value(matches).unwrap(),
            json!([
                {"score": 0, "path": "src/main.rs", "indices": [4, 5]},
                {"score": 0, "path": "Cargo.toml"},
            ])
        );
    }
}
