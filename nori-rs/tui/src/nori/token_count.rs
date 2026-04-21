//! Token counting for instruction files.
//!
//! Uses the best available tokenizer for each agent kind:
//! - Codex (OpenAI): exact count via `o200k_base` encoding
//! - Claude (Anthropic): approximate count via `o200k_base` as a proxy
//! - Gemini / unknown: heuristic estimate (bytes / 4)

use super::session_header::AgentKindSimple;

/// Result of counting tokens in a text string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenCount {
    /// Number of tokens.
    pub count: i64,
    /// Whether this is an approximate count (proxy tokenizer or heuristic).
    pub approximate: bool,
}

/// Count tokens for the given text using the best available tokenizer
/// for the agent kind.
pub fn count_tokens(text: &str, agent_kind: Option<AgentKindSimple>) -> TokenCount {
    match agent_kind {
        Some(AgentKindSimple::Codex) => count_tokens_bpe(text, false),
        Some(AgentKindSimple::Claude) => count_tokens_bpe(text, true),
        Some(AgentKindSimple::Gemini) | None => count_tokens_heuristic(text),
    }
}

/// Count tokens using the o200k_base BPE tokenizer.
///
/// Falls back to heuristic if the tokenizer fails to initialize.
fn count_tokens_bpe(text: &str, approximate: bool) -> TokenCount {
    match tiktoken_rs::o200k_base() {
        Ok(bpe) => {
            let tokens = bpe.encode_ordinary(text);
            TokenCount {
                count: tokens.len() as i64,
                approximate,
            }
        }
        Err(_) => count_tokens_heuristic(text),
    }
}

/// Heuristic token count: bytes / 4, rounded up.
fn count_tokens_heuristic(text: &str) -> TokenCount {
    let count = (text.len() as i64 + 3) / 4;
    TokenCount {
        count,
        approximate: true,
    }
}

/// Format a token count for display with thousands separators.
///
/// Examples: `"1,234 tokens"`, `"~1,234 tokens"`, `"1 token"`
pub fn format_token_count(tc: &TokenCount) -> String {
    let formatted_number = format_with_thousands_separator(tc.count);
    let prefix = if tc.approximate { "~" } else { "" };
    let noun = if tc.count == 1 { "token" } else { "tokens" };
    format!("{prefix}{formatted_number} {noun}")
}

fn format_with_thousands_separator(n: i64) -> String {
    if n < 0 {
        return format!("-{}", format_with_thousands_separator(-n));
    }
    let s = n.to_string();
    let mut result = String::with_capacity(s.len() + s.len() / 3);
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            result.push(',');
        }
        result.push(ch);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn heuristic_empty_string() {
        let tc = count_tokens_heuristic("");
        assert_eq!(
            tc,
            TokenCount {
                count: 0,
                approximate: true,
            }
        );
    }

    #[test]
    fn heuristic_short_string() {
        // "hello" = 5 bytes -> (5 + 3) / 4 = 2
        let tc = count_tokens_heuristic("hello");
        assert_eq!(
            tc,
            TokenCount {
                count: 2,
                approximate: true,
            }
        );
    }

    #[test]
    fn heuristic_exact_multiple() {
        // 8 bytes -> (8 + 3) / 4 = 2
        let tc = count_tokens_heuristic("12345678");
        assert_eq!(tc.count, 2);
    }

    #[test]
    fn bpe_counts_tokens() {
        // The BPE tokenizer should produce a deterministic count for a known string.
        let tc = count_tokens_bpe("Hello, world!", false);
        assert!(!tc.approximate);
        assert!(tc.count > 0, "Should count at least one token");
    }

    #[test]
    fn bpe_approximate_flag_propagated() {
        let tc = count_tokens_bpe("Hello, world!", true);
        assert!(tc.approximate);
        assert!(tc.count > 0);
    }

    #[test]
    fn count_tokens_codex_uses_bpe_exact() {
        let tc = count_tokens("Hello, world!", Some(AgentKindSimple::Codex));
        assert!(!tc.approximate, "Codex should use exact BPE");
        assert!(tc.count > 0);
    }

    #[test]
    fn count_tokens_claude_uses_bpe_approximate() {
        let tc = count_tokens("Hello, world!", Some(AgentKindSimple::Claude));
        assert!(tc.approximate, "Claude should use approximate BPE");
        assert!(tc.count > 0);
    }

    #[test]
    fn count_tokens_gemini_uses_heuristic() {
        let tc = count_tokens("Hello, world!", Some(AgentKindSimple::Gemini));
        assert!(tc.approximate, "Gemini should use heuristic");
    }

    #[test]
    fn count_tokens_unknown_uses_heuristic() {
        let tc = count_tokens("Hello, world!", None);
        assert!(tc.approximate, "Unknown agent should use heuristic");
    }

    #[test]
    fn count_tokens_codex_and_claude_agree_on_same_text() {
        // Both use o200k_base, so the count should be identical.
        let text = "This is a test of the token counting system.";
        let codex = count_tokens(text, Some(AgentKindSimple::Codex));
        let claude = count_tokens(text, Some(AgentKindSimple::Claude));
        assert_eq!(
            codex.count, claude.count,
            "Codex and Claude should produce the same count (both use o200k_base)"
        );
        assert!(!codex.approximate);
        assert!(claude.approximate);
    }

    #[test]
    fn format_token_count_singular() {
        let tc = TokenCount {
            count: 1,
            approximate: false,
        };
        assert_eq!(format_token_count(&tc), "1 token");
    }

    #[test]
    fn format_token_count_exact_plural() {
        let tc = TokenCount {
            count: 42,
            approximate: false,
        };
        assert_eq!(format_token_count(&tc), "42 tokens");
    }

    #[test]
    fn format_token_count_approximate() {
        let tc = TokenCount {
            count: 42,
            approximate: true,
        };
        assert_eq!(format_token_count(&tc), "~42 tokens");
    }

    #[test]
    fn format_token_count_thousands() {
        let tc = TokenCount {
            count: 1234,
            approximate: false,
        };
        assert_eq!(format_token_count(&tc), "1,234 tokens");
    }

    #[test]
    fn format_token_count_large_approximate() {
        let tc = TokenCount {
            count: 12345,
            approximate: true,
        };
        assert_eq!(format_token_count(&tc), "~12,345 tokens");
    }

    #[test]
    fn format_token_count_zero() {
        let tc = TokenCount {
            count: 0,
            approximate: false,
        };
        assert_eq!(format_token_count(&tc), "0 tokens");
    }

    #[test]
    fn format_thousands_separator() {
        assert_eq!(format_with_thousands_separator(0), "0");
        assert_eq!(format_with_thousands_separator(1), "1");
        assert_eq!(format_with_thousands_separator(999), "999");
        assert_eq!(format_with_thousands_separator(1000), "1,000");
        assert_eq!(format_with_thousands_separator(1234567), "1,234,567");
    }
}
