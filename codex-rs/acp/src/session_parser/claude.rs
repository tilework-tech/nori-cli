//! Claude session transcript parser
//!
//! Parses JSONL format with `usage` objects in assistant message entries.
//! Extracts token usage from the last assistant message.

use super::TokenUsageReport;
use codex_protocol::protocol::TokenUsage;
use serde::Deserialize;
use std::fs::File;
use std::io::BufReader;
use std::io::{self};
use std::path::PathBuf;

#[derive(Debug, Deserialize)]
struct ClaudeEvent {
    #[serde(rename = "type")]
    event_type: String,
    message: Option<ClaudeMessage>,
}

#[derive(Debug, Deserialize)]
struct ClaudeMessage {
    usage: Option<ClaudeUsage>,
}

#[derive(Debug, Deserialize)]
struct ClaudeUsage {
    input_tokens: i64,
    #[serde(default)]
    cache_creation_input_tokens: i64,
    #[serde(default)]
    cache_read_input_tokens: i64,
    output_tokens: i64,
}

/// Parse a Claude session transcript and extract token usage
///
/// Returns `None` if the session contains no assistant messages with usage data.
pub fn parse_claude_session(path: PathBuf) -> io::Result<Option<TokenUsageReport>> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);

    let mut last_usage: Option<ClaudeUsage> = None;

    // Use streaming JSONL parser
    let deserializer = serde_json::Deserializer::from_reader(reader);
    let stream = deserializer.into_iter::<ClaudeEvent>();

    for event_result in stream {
        let event = event_result.map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

        // Look for assistant events with usage data
        if event.event_type == "assistant"
            && let Some(message) = event.message
                && let Some(usage) = message.usage {
                    last_usage = Some(usage);
                }
    }

    Ok(last_usage.map(|usage| {
        TokenUsageReport {
            token_usage: TokenUsage {
                input_tokens: usage.input_tokens,
                cached_input_tokens: usage.cache_creation_input_tokens
                    + usage.cache_read_input_tokens,
                output_tokens: usage.output_tokens,
                reasoning_output_tokens: 0, // Claude sessions don't track reasoning separately in this format
                total_tokens: usage.input_tokens + usage.output_tokens,
            },
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_parse_claude_session_extracts_usage_from_last_message() {
        // Arrange: Use the real session-claude.jsonl fixture
        let fixture_path = PathBuf::from("tests/fixtures/session-claude.jsonl");

        // Act: Parse the session
        let result = parse_claude_session(fixture_path).expect("Should parse without IO error");

        // Assert: Should extract the last assistant message's usage
        assert!(result.is_some(), "Should return Some when usage exists");
        let report = result.unwrap();

        // From the Python analysis, last usage:
        // input_tokens=1, cache_creation_input_tokens=437, cache_read_input_tokens=27215, output_tokens=1
        // cached_input_tokens = 437 + 27215 = 27652
        // total_tokens = 1 + 1 = 2
        assert_eq!(report.token_usage.input_tokens, 1);
        assert_eq!(report.token_usage.cached_input_tokens, 27652);
        assert_eq!(report.token_usage.output_tokens, 1);
        assert_eq!(report.token_usage.reasoning_output_tokens, 0);
        assert_eq!(report.token_usage.total_tokens, 2);
    }
}
