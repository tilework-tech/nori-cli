//! Gemini session transcript parser
//!
//! Parses single JSON object format with messages array.
//! Aggregates token usage across all messages.

use super::TokenUsageReport;
use codex_protocol::protocol::TokenUsage;
use serde::Deserialize;
use std::fs::File;
use std::io::{self, BufReader};
use std::path::PathBuf;

#[derive(Debug, Deserialize)]
struct GeminiSession {
    messages: Vec<GeminiMessage>,
}

#[derive(Debug, Deserialize)]
struct GeminiMessage {
    tokens: Option<GeminiTokens>,
}

#[derive(Debug, Deserialize)]
struct GeminiTokens {
    input: i64,
    output: i64,
    cached: i64,
    thoughts: i64,
    #[allow(dead_code)]
    tool: i64,
    #[allow(dead_code)]
    total: i64,
}

/// Parse a Gemini session transcript and extract token usage
///
/// Returns `None` if the session contains no messages with token data.
pub fn parse_gemini_session(path: PathBuf) -> io::Result<Option<TokenUsageReport>> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    
    let session: GeminiSession = serde_json::from_reader(reader)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    
    // Aggregate tokens across all messages
    let mut total_input = 0i64;
    let mut total_cached = 0i64;
    let mut total_output = 0i64;
    let mut total_thoughts = 0i64;
    let mut has_tokens = false;
    
    for message in session.messages {
        if let Some(tokens) = message.tokens {
            total_input += tokens.input;
            total_cached += tokens.cached;
            total_output += tokens.output;
            total_thoughts += tokens.thoughts;
            has_tokens = true;
        }
    }
    
    if !has_tokens {
        return Ok(None);
    }
    
    // Map Gemini tokens to TokenUsage format
    let token_usage = TokenUsage {
        input_tokens: total_input,
        cached_input_tokens: total_cached,
        output_tokens: total_output,
        reasoning_output_tokens: total_thoughts,
        total_tokens: total_input + total_output + total_thoughts,
    };
    
    Ok(Some(TokenUsageReport { token_usage }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_parse_gemini_session_aggregates_tokens() {
        // Arrange: Use the real session-gemini.json fixture
        let fixture_path = PathBuf::from("tests/fixtures/session-gemini.json");
        
        // Act: Parse the session
        let result = parse_gemini_session(fixture_path).expect("Should parse without IO error");
        
        // Assert: Should aggregate tokens across all messages
        assert!(result.is_some(), "Should return Some when token usage exists");
        let report = result.unwrap();
        
        // From the Python analysis:
        // Aggregated tokens: input=86721, output=3838, cached=35596, thoughts=6931
        // total = input + output + thoughts = 86721 + 3838 + 6931 = 97490
        assert_eq!(report.token_usage.input_tokens, 86721);
        assert_eq!(report.token_usage.cached_input_tokens, 35596);
        assert_eq!(report.token_usage.output_tokens, 3838);
        assert_eq!(report.token_usage.reasoning_output_tokens, 6931);
        assert_eq!(report.token_usage.total_tokens, 97490);
    }
}
