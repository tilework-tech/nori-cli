//! `@` file mentions in the plain terminal. A session on the host this
//! terminal owns is searched by the host in that session's folder, so
//! switching sessions switches where matches come from. With no session on
//! screen, a joined host that serves `find_paths` searches its own folder;
//! otherwise (cloud mode, or before the first session starts) the launch
//! folder is searched locally.

use codex_file_search::FileMatch;
use nori_harness::ConversationId;

use super::App;
use crate::app_event::AppEvent;
use crate::host_client::FileSearch;
use crate::host_client::HostFileSearch;

/// Where one `@` query is searched.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Route {
    Local(String),
    Host(FileSearch),
}

#[derive(Default)]
pub(crate) struct FileMentions {
    host: HostFileSearch,
}

impl FileMentions {
    /// Where `query`, typed in the session on screen, is searched now;
    /// none while a host search is in flight (it waits) or repeats. With no
    /// session on screen, a joined host that finds paths searches its own
    /// folder.
    pub(super) fn route(
        &mut self,
        foreground: Option<ConversationId>,
        joined_host_finds_paths: bool,
        query: String,
    ) -> Option<Route> {
        match foreground {
            None if !joined_host_finds_paths => Some(Route::Local(query)),
            foreground => self
                .host
                .query(FileSearch {
                    session_id: foreground.map(|id| id.to_string()),
                    query,
                })
                .map(Route::Host),
        }
    }

    /// A host search landed: the popup matches when it was for the session
    /// still on screen, and the search that waited for it.
    pub(super) fn landed(
        &mut self,
        search: &FileSearch,
        foreground: Option<ConversationId>,
        result: Result<serde_json::Value, String>,
    ) -> (Option<Vec<FileMatch>>, Option<FileSearch>) {
        let current = foreground.map(|id| id.to_string()) == search.session_id;
        let matches = result
            .ok()
            .filter(|_| current)
            .and_then(|reply| crate::host_client::file_matches(reply).ok());
        (matches, self.host.finished())
    }
}

impl App {
    pub(super) fn start_file_search(&mut self, query: String) {
        if query.is_empty() {
            return;
        }
        let joined_host_finds_paths = self.is_joined()
            && self
                .chat_widget
                .host_connection()
                .is_some_and(|connection| connection.features.find_paths);
        match self.file_mentions.route(
            self.chat_widget.managed_conversation_id(),
            joined_host_finds_paths,
            query,
        ) {
            Some(Route::Local(query)) => self.file_search.on_user_query(query),
            Some(Route::Host(search)) => self.send_host_file_search(search),
            None => {}
        }
    }

    pub(super) fn on_host_file_search_landed(
        &mut self,
        search: FileSearch,
        result: Result<serde_json::Value, String>,
    ) {
        let (matches, next) =
            self.file_mentions
                .landed(&search, self.chat_widget.managed_conversation_id(), result);
        if let Some(matches) = matches {
            self.chat_widget
                .apply_file_search_result(search.query, matches);
        }
        if let Some(next) = next {
            self.send_host_file_search(next);
        }
    }

    fn send_host_file_search(&self, search: FileSearch) {
        let tx = self.app_event_tx.clone();
        let reply = tx.host_receipt(search.call());
        tokio::spawn(async move {
            let result = reply.await.map_err(|error| error.to_string());
            tx.send(AppEvent::HostFileSearchLanded { search, result });
        });
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use serde_json::json;

    use super::*;

    fn host(id: ConversationId, query: &str) -> Option<Route> {
        Some(Route::Host(FileSearch {
            session_id: Some(id.to_string()),
            query: query.into(),
        }))
    }

    fn reply(path: &str) -> Result<serde_json::Value, String> {
        Ok(json!({"root": "/srv", "matches": [{"path": path}]}))
    }

    fn paths(matches: Option<Vec<FileMatch>>) -> Option<Vec<String>> {
        matches.map(|matches| matches.into_iter().map(|found| found.path).collect())
    }

    #[test]
    fn a_host_session_is_searched_by_the_host_and_a_widget_without_one_locally() {
        let mut mentions = FileMentions::default();
        let one = ConversationId::new();
        assert_eq!(
            mentions.route(None, false, "ma".into()),
            Some(Route::Local("ma".into()))
        );
        assert_eq!(
            mentions.route(Some(one), false, "ma".into()),
            host(one, "ma")
        );
    }

    #[test]
    fn one_search_is_in_flight_and_the_newest_query_follows_it() {
        let mut mentions = FileMentions::default();
        let one = ConversationId::new();
        let first = mentions.route(Some(one), false, "l".into());
        assert_eq!(first, host(one, "l"));
        assert_eq!(mentions.route(Some(one), false, "le".into()), None);
        assert_eq!(mentions.route(Some(one), false, "lex".into()), None);
        let Some(Route::Host(first)) = first else {
            unreachable!()
        };
        let (matches, next) = mentions.landed(&first, Some(one), reply("src/lexer.rs"));
        assert_eq!(paths(matches), Some(vec!["src/lexer.rs".to_owned()]));
        assert_eq!(next.map(Route::Host), host(one, "lex"));
    }

    #[test]
    fn switching_sessions_searches_the_new_one_and_drops_the_old_ones_matches() {
        let mut mentions = FileMentions::default();
        let (one, two) = (ConversationId::new(), ConversationId::new());
        let Some(Route::Host(first)) = mentions.route(Some(one), false, "ma".into()) else {
            panic!("the host searches session one");
        };
        assert_eq!(mentions.route(Some(two), false, "ma".into()), None);
        let (matches, next) = mentions.landed(&first, Some(two), reply("one/main.rs"));
        assert_eq!(paths(matches), None, "session one's files never reach two");
        assert_eq!(next.map(Route::Host), host(two, "ma"));
    }
}
