//! Resolves the session a user names on the command line against the host's
//! session list, so a unique start of an ID is enough.

use serde_json::Value;

/// The full ID of the one session in `listing` whose ID starts with `query`.
pub(super) fn resolve_session(listing: &Value, query: &str) -> anyhow::Result<String> {
    let query = query.trim().to_lowercase();
    let sessions: Vec<&Value> = listing["sessions"]
        .as_array()
        .into_iter()
        .flatten()
        .collect();
    let id = |row: &Value| row["session_id"].as_str().unwrap_or_default().to_string();
    if let Some(exact) = sessions.iter().find(|row| id(row).to_lowercase() == query) {
        return Ok(id(exact));
    }
    let matches: Vec<&&Value> = sessions
        .iter()
        .filter(|row| !query.is_empty() && id(row).to_lowercase().starts_with(&query))
        .collect();
    match matches.as_slice() {
        [only] => Ok(id(only)),
        [] => anyhow::bail!(
            "No session on this host has an ID starting with {query}. Run `nori host ls | cat` to see full IDs, or `nori attach` to pick from the list."
        ),
        several => {
            let candidates: Vec<String> = several
                .iter()
                .map(|row| {
                    let clean = |key: &str| {
                        row[key]
                            .as_str()
                            .unwrap_or_default()
                            .chars()
                            .map(|c| if c.is_control() { ' ' } else { c })
                            .collect::<String>()
                    };
                    let title = row["title"]
                        .as_str()
                        .map(|_| clean("title"))
                        .unwrap_or_else(|| nori_protocol::UNTITLED_SESSION_NAME.into());
                    format!("  {}  {}  {title}", id(row), clean("agent"))
                })
                .collect();
            anyhow::bail!(
                "{query} matches {} sessions; use more of the ID:\n{}",
                several.len(),
                candidates.join("\n")
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    fn listing() -> Value {
        json!({"sessions":[
            {"session_id":"3f2a9c10-0000-4000-8000-000000000001","agent":"claude","status":"ready","title":"Fix the parser"},
            {"session_id":"3f2a9c22-0000-4000-8000-000000000002","agent":"codex","status":"working","title":"Review the lexer"},
            {"session_id":"7b11d0e4-0000-4000-8000-000000000003","agent":"gemini","status":"ready"}
        ]})
    }

    #[test]
    fn a_unique_start_of_an_id_names_that_session() {
        assert_eq!(
            resolve_session(&listing(), "7b11").unwrap(),
            "7b11d0e4-0000-4000-8000-000000000003"
        );
        assert_eq!(
            resolve_session(&listing(), "3F2A9C1").unwrap(),
            "3f2a9c10-0000-4000-8000-000000000001",
            "IDs match without regard to case"
        );
    }

    #[test]
    fn a_full_id_names_its_session() {
        assert_eq!(
            resolve_session(&listing(), "3f2a9c22-0000-4000-8000-000000000002").unwrap(),
            "3f2a9c22-0000-4000-8000-000000000002"
        );
    }

    #[test]
    fn a_start_shared_by_several_sessions_lists_them() {
        let error = resolve_session(&listing(), "3f2a").unwrap_err().to_string();
        assert_eq!(
            error,
            "3f2a matches 2 sessions; use more of the ID:\n  \
             3f2a9c10-0000-4000-8000-000000000001  claude  Fix the parser\n  \
             3f2a9c22-0000-4000-8000-000000000002  codex  Review the lexer"
        );
    }

    #[test]
    fn an_id_no_session_has_says_where_to_find_one() {
        let error = resolve_session(&listing(), "zz").unwrap_err().to_string();
        assert_eq!(
            error,
            "No session on this host has an ID starting with zz. Run `nori host ls | cat` to see full IDs, or `nori attach` to pick from the list."
        );
    }

    #[test]
    fn an_empty_id_matches_nothing() {
        assert!(resolve_session(&listing(), "").is_err());
    }
}
