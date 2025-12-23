//! Codex session transcript parser
//!
//! Parses JSONL format with `event_msg` events containing `token_count` payloads.
//! Uses `last_token_usage` field for accurate per-turn token tracking.

use super::TokenUsageReport;
use codex_protocol::protocol::TokenUsage;
use serde::Deserialize;
use std::fs::File;
use std::io::{self, BufReader};
use std::path::PathBuf;

#[derive(Debug, Deserialize)]
struct CodexEvent {
    #[serde(rename = "type")]
    event_type: String,
    payload: Option<CodexPayload>,
}

#[derive(Debug, Deserialize)]
struct CodexPayload {
    #[serde(rename = "type")]
    payload_type: Option<String>,
    info: Option<TokenCountInfo>,
}

#[derive(Debug, Deserialize)]
struct TokenCountInfo {
    last_token_usage: Option<TokenUsage>,
}

/// Parse a Codex session transcript and extract token usage
///
/// Returns `None` if the session contains no token_count events.
pub fn parse_codex_session(path: PathBuf) -> io::Result<Option<TokenUsageReport>> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    
    let mut last_token_usage: Option<TokenUsage> = None;
    
    // Use streaming JSONL parser
    let deserializer = serde_json::Deserializer::from_reader(reader);
    let stream = deserializer.into_iter::<CodexEvent>();
    
    for event_result in stream {
        let event = event_result.map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        
        // Look for event_msg events with token_count payload
        if event.event_type == "event_msg" {
            if let Some(payload) = event.payload {
                if payload.payload_type.as_deref() == Some("token_count") {
                    if let Some(info) = payload.info {
                        if let Some(usage) = info.last_token_usage {
                            last_token_usage = Some(usage);
                        }
                    }
                }
            }
        }
    }
    
    Ok(last_token_usage.map(|token_usage| TokenUsageReport { token_usage }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_parse_codex_session_extracts_last_token_usage() {
        // Arrange: Use the real session-codex.jsonl fixture
        let fixture_path = PathBuf::from("tests/fixtures/session-codex.jsonl");
        
        // Act: Parse the session
        let result = parse_codex_session(fixture_path).expect("Should parse without IO error");
        
        // Assert: Should extract the last token_count event's last_token_usage
        assert!(result.is_some(), "Should return Some when token usage exists");
        let report = result.unwrap();
        
        // From the last token_count event in session-codex.jsonl:
        // "last_token_usage":{"input_tokens":8612,"cached_input_tokens":0,"output_tokens":218,"reasoning_output_tokens":128,"total_tokens":8830}
        assert_eq!(report.token_usage.input_tokens, 8612);
        assert_eq!(report.token_usage.cached_input_tokens, 0);
        assert_eq!(report.token_usage.output_tokens, 218);
        assert_eq!(report.token_usage.reasoning_output_tokens, 128);
        assert_eq!(report.token_usage.total_tokens, 8830);
    }
}
