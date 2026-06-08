use nori_acp::broker::CloudSessionSummary;

#[derive(Debug, PartialEq, Eq)]
pub enum SessionChoice {
    New,
    Resume(i32),
}

pub fn format_cloud_session_list(sessions: &[CloudSessionSummary]) -> String {
    let mut out = String::from("Cloud Sessions:\n");
    for (i, session) in sessions.iter().enumerate() {
        let preview = session
            .first_message_preview
            .as_deref()
            .unwrap_or("(no preview)");
        let active = format_timestamp(&session.last_active_at);
        out.push_str(&format!(
            "  [{num}] ({source}) \"{preview}\" — last active {active}\n",
            num = i + 1,
            source = session.source,
        ));
    }
    out.push_str("  [n] Start new session\n");
    out
}

fn format_timestamp(iso_timestamp: &str) -> String {
    iso_timestamp
        .get(..16)
        .unwrap_or(iso_timestamp)
        .replace('T', " ")
}

pub fn parse_session_choice(input: &str, session_count: i32) -> Result<SessionChoice, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("empty input".to_string());
    }
    if trimmed.eq_ignore_ascii_case("n") {
        return Ok(SessionChoice::New);
    }
    match trimmed.parse::<i32>() {
        Ok(n) if n >= 1 && n <= session_count => Ok(SessionChoice::Resume(n - 1)),
        Ok(n) => Err(format!("selection {n} out of range (1-{session_count})")),
        Err(_) => Err(format!("invalid selection: {trimmed}")),
    }
}

pub fn prompt_session_selection(sessions: &[CloudSessionSummary]) -> anyhow::Result<SessionChoice> {
    use std::io::BufRead;
    use std::io::Write as IoWrite;

    eprint!("{}", format_cloud_session_list(sessions));
    eprint!("Select session (or 'n' for new): ");
    std::io::stderr().flush()?;

    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line)?;

    parse_session_choice(&line, sessions.len() as i32)
        .map_err(|e| anyhow::anyhow!("Invalid selection: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn make_session(
        session_id: &str,
        source: &str,
        first_message_preview: Option<&str>,
        status: &str,
    ) -> CloudSessionSummary {
        CloudSessionSummary {
            session_id: session_id.to_string(),
            source: source.to_string(),
            created_at: "2025-01-27T12:00:00Z".to_string(),
            last_active_at: "2025-01-27T14:30:00Z".to_string(),
            first_message_preview: first_message_preview.map(String::from),
            status: status.to_string(),
        }
    }

    #[test]
    fn format_session_list_includes_numbered_entries_and_new_option() {
        let sessions = vec![
            make_session("sess-1", "cli", Some("Fix the login bug"), "idle"),
            make_session("sess-2", "slack", None, "idle"),
        ];

        let output = format_cloud_session_list(&sessions);

        assert_eq!(
            output,
            "Cloud Sessions:\n\
             \x20 [1] (cli) \"Fix the login bug\" — last active 2025-01-27 14:30\n\
             \x20 [2] (slack) \"(no preview)\" — last active 2025-01-27 14:30\n\
             \x20 [n] Start new session\n"
        );
    }

    #[test]
    fn format_session_list_handles_empty_list() {
        let output = format_cloud_session_list(&[]);

        assert_eq!(
            output,
            "Cloud Sessions:\n\
             \x20 [n] Start new session\n"
        );
    }

    #[test]
    fn format_session_list_handles_missing_preview() {
        let sessions = vec![make_session("sess-1", "discord", None, "idle")];

        let output = format_cloud_session_list(&sessions);

        assert_eq!(
            output,
            "Cloud Sessions:\n\
             \x20 [1] (discord) \"(no preview)\" — last active 2025-01-27 14:30\n\
             \x20 [n] Start new session\n"
        );
    }

    #[test]
    fn parse_choice_selects_valid_session() {
        assert_eq!(parse_session_choice("1", 3), Ok(SessionChoice::Resume(0)));
        assert_eq!(parse_session_choice("3", 3), Ok(SessionChoice::Resume(2)));
    }

    #[test]
    fn parse_choice_selects_new_session() {
        assert_eq!(parse_session_choice("n", 3), Ok(SessionChoice::New));
        assert_eq!(parse_session_choice("N", 3), Ok(SessionChoice::New));
        assert_eq!(parse_session_choice(" n ", 3), Ok(SessionChoice::New));
    }

    #[test]
    fn parse_choice_rejects_out_of_range() {
        assert!(parse_session_choice("0", 3).is_err());
        assert!(parse_session_choice("4", 3).is_err());
        assert!(parse_session_choice("100", 3).is_err());
    }

    #[test]
    fn parse_choice_rejects_invalid_input() {
        assert!(parse_session_choice("abc", 3).is_err());
        assert!(parse_session_choice("", 3).is_err());
        assert!(parse_session_choice("  ", 3).is_err());
    }

    #[test]
    fn parse_choice_rejects_negative_numbers() {
        assert!(parse_session_choice("-1", 3).is_err());
        assert!(parse_session_choice("-100", 3).is_err());
    }

    #[test]
    fn parse_choice_works_with_zero_sessions() {
        assert_eq!(parse_session_choice("n", 0), Ok(SessionChoice::New));
        assert!(parse_session_choice("1", 0).is_err());
    }
}
