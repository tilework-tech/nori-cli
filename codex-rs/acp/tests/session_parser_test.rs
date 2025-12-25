use codex_acp::session_parser::AgentKind;
use codex_acp::session_parser::ParseError;
use codex_acp::session_parser::parse_claude_session;
use codex_acp::session_parser::parse_codex_session;
use codex_acp::session_parser::parse_gemini_session;
use std::io::Write;
use std::path::Path;

#[tokio::test]
async fn test_parse_codex_session() {
    // Tests run from codex-rs/acp, session files are two levels up in worktree root
    let path = Path::new("./tests/fixtures/session-codex.jsonl");
    let report = parse_codex_session(path)
        .await
        .expect("should parse codex session");

    assert_eq!(report.agent_kind, AgentKind::Codex);
    assert_eq!(report.token_usage.input_tokens, 38890);
    assert_eq!(report.token_usage.cached_input_tokens, 15744);
    assert_eq!(report.token_usage.output_tokens, 1084);
    assert_eq!(report.token_usage.reasoning_output_tokens, 704);
    assert_eq!(report.token_usage.total_tokens, 39974);

    // Session ID should be derived from filename
    assert!(
        !report.session_id.is_empty(),
        "session ID should not be empty"
    );

    // Should have model context window from Codex metadata
    assert_eq!(report.model_context_window, Some(258400));
}

#[tokio::test]
async fn test_parse_gemini_session() {
    // Tests run from codex-rs/acp, session files are two levels up in worktree root
    let path = Path::new("./tests/fixtures/session-gemini.json");
    let report = parse_gemini_session(path)
        .await
        .expect("should parse gemini session");

    assert_eq!(report.agent_kind, AgentKind::Gemini);
    assert_eq!(report.session_id, "d126c5e7-62ae-471a-8a5e-2cf6ddac8a9b");

    // Aggregated token totals from research: 86,721 input, 35,596 cached, 3,838 output, 6,931 thoughts
    assert_eq!(report.token_usage.input_tokens, 86721);
    assert_eq!(report.token_usage.cached_input_tokens, 35596);
    assert_eq!(report.token_usage.output_tokens, 3838);
    assert_eq!(report.token_usage.reasoning_output_tokens, 6931); // thoughts map to reasoning

    // Gemini format does not include model_context_window in JSON
    assert_eq!(report.model_context_window, None);

    assert!(
        report.transcript_path.ends_with("session-gemini.json"),
        "transcript path should end with session-gemini.json"
    );
}

#[tokio::test]
async fn test_parse_claude_session() {
    // Tests run from codex-rs/acp, session files are two levels up in worktree root
    let path = Path::new("./tests/fixtures/session-claude.jsonl");
    let report = parse_claude_session(path)
        .await
        .expect("should parse claude session");

    assert_eq!(report.agent_kind, AgentKind::Claude);
    assert_eq!(report.session_id, "ccded934-ae45-4ef6-9271-950657d2161a");

    // Aggregated token totals: 301 input, 105798 cache_creation, 518739 cache_read, 1613 output
    assert_eq!(report.token_usage.input_tokens, 301);
    // Claude has both cache_creation and cache_read; we map cache_read to cached_input_tokens
    assert_eq!(report.token_usage.cached_input_tokens, 518739);
    assert_eq!(report.token_usage.output_tokens, 1613);
    // Claude doesn't have separate reasoning tokens (set to 0)
    assert_eq!(report.token_usage.reasoning_output_tokens, 0);

    // Claude format does not include model_context_window
    assert_eq!(report.model_context_window, None);

    assert!(
        report.transcript_path.ends_with("session-claude.jsonl"),
        "transcript path should end with session-claude.jsonl"
    );
}

#[tokio::test]
async fn test_parse_empty_file() {
    let temp_file = tempfile::NamedTempFile::new().expect("create temp file");
    // Leave file empty

    let result = parse_codex_session(temp_file.path()).await;
    assert!(matches!(result, Err(ParseError::EmptyFile)));
}

#[tokio::test]
async fn test_parse_missing_session_id() {
    let mut temp_file = tempfile::NamedTempFile::new().expect("create temp file");
    // Write valid JSON but without session ID
    writeln!(temp_file, r#"{{"messages":[]}}"#).expect("write to temp file");
    temp_file.flush().expect("flush temp file");

    let result = parse_gemini_session(temp_file.path()).await;
    assert!(matches!(result, Err(ParseError::MissingSessionId)));
}
