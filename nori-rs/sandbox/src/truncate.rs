//! Utilities for truncating large chunks of output while preserving a prefix
//! and suffix on UTF-8 boundaries, and helpers for line/token‑based truncation
//! used across the core crate.

const APPROX_BYTES_PER_TOKEN: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TruncationPolicy {
    Bytes(usize),
    Tokens(usize),
}

impl TruncationPolicy {
    /// Returns a token budget derived from this policy.
    ///
    /// - For `Tokens`, this is the explicit token limit.
    /// - For `Bytes`, this is an approximate token budget using the global
    ///   bytes-per-token heuristic.
    pub fn token_budget(&self) -> usize {
        match self {
            TruncationPolicy::Bytes(bytes) => {
                usize::try_from(approx_tokens_from_byte_count(*bytes)).unwrap_or(usize::MAX)
            }
            TruncationPolicy::Tokens(tokens) => *tokens,
        }
    }

    /// Returns a byte budget derived from this policy.
    ///
    /// - For `Bytes`, this is the explicit byte limit.
    /// - For `Tokens`, this is an approximate byte budget using the global
    ///   bytes-per-token heuristic.
    pub fn byte_budget(&self) -> usize {
        match self {
            TruncationPolicy::Bytes(bytes) => *bytes,
            TruncationPolicy::Tokens(tokens) => approx_bytes_for_tokens(*tokens),
        }
    }
}

#[cfg(test)]
pub(crate) fn formatted_truncate_text(content: &str, policy: TruncationPolicy) -> String {
    if content.len() <= policy.byte_budget() {
        return content.to_string();
    }
    let total_lines = content.lines().count();
    let result = truncate_text(content, policy);
    format!("Total output lines: {total_lines}\n\n{result}")
}

pub(crate) fn truncate_text(content: &str, policy: TruncationPolicy) -> String {
    match policy {
        TruncationPolicy::Bytes(_) => truncate_with_byte_estimate(content, policy),
        TruncationPolicy::Tokens(_) => {
            let (truncated, _) = truncate_with_token_budget(content, policy);
            truncated
        }
    }
}
/// Truncate the middle of a UTF-8 string to at most `max_tokens` tokens,
/// preserving the beginning and the end. Returns the possibly truncated string
/// and `Some(original_token_count)` if truncation occurred; otherwise returns
/// the original string and `None`.
fn truncate_with_token_budget(s: &str, policy: TruncationPolicy) -> (String, Option<u64>) {
    if s.is_empty() {
        return (String::new(), None);
    }
    let max_tokens = policy.token_budget();

    let byte_len = s.len();
    if max_tokens > 0 && byte_len <= approx_bytes_for_tokens(max_tokens) {
        return (s.to_string(), None);
    }

    let truncated = truncate_with_byte_estimate(s, policy);
    let approx_total_usize = approx_token_count(s);
    let approx_total = u64::try_from(approx_total_usize).unwrap_or(u64::MAX);
    if truncated == s {
        (truncated, None)
    } else {
        (truncated, Some(approx_total))
    }
}

/// Truncate a string using a byte budget derived from the token budget, without
/// performing any real tokenization. This keeps the logic purely byte-based and
/// uses a bytes placeholder in the truncated output.
fn truncate_with_byte_estimate(s: &str, policy: TruncationPolicy) -> String {
    if s.is_empty() {
        return String::new();
    }

    let total_chars = s.chars().count();
    let max_bytes = policy.byte_budget();

    if max_bytes == 0 {
        // No budget to show content; just report that everything was truncated.
        let marker = format_truncation_marker(
            policy,
            removed_units_for_source(policy, s.len(), total_chars),
        );
        return marker;
    }

    if s.len() <= max_bytes {
        return s.to_string();
    }

    let total_bytes = s.len();

    let (left_budget, right_budget) = split_budget(max_bytes);

    let (removed_chars, left, right) = split_string(s, left_budget, right_budget);

    let marker = format_truncation_marker(
        policy,
        removed_units_for_source(policy, total_bytes.saturating_sub(max_bytes), removed_chars),
    );

    assemble_truncated_output(left, right, &marker)
}

fn split_string(s: &str, beginning_bytes: usize, end_bytes: usize) -> (usize, &str, &str) {
    if s.is_empty() {
        return (0, "", "");
    }

    let len = s.len();
    let tail_start_target = len.saturating_sub(end_bytes);
    let mut prefix_end = 0usize;
    let mut suffix_start = len;
    let mut removed_chars = 0usize;
    let mut suffix_started = false;

    for (idx, ch) in s.char_indices() {
        let char_end = idx + ch.len_utf8();
        if char_end <= beginning_bytes {
            prefix_end = char_end;
            continue;
        }

        if idx >= tail_start_target {
            if !suffix_started {
                suffix_start = idx;
                suffix_started = true;
            }
            continue;
        }

        removed_chars = removed_chars.saturating_add(1);
    }

    if suffix_start < prefix_end {
        suffix_start = prefix_end;
    }

    let before = &s[..prefix_end];
    let after = &s[suffix_start..];

    (removed_chars, before, after)
}

fn format_truncation_marker(policy: TruncationPolicy, removed_count: u64) -> String {
    match policy {
        TruncationPolicy::Tokens(_) => format!("…{removed_count} tokens truncated…"),
        TruncationPolicy::Bytes(_) => format!("…{removed_count} chars truncated…"),
    }
}

fn split_budget(budget: usize) -> (usize, usize) {
    let left = budget / 2;
    (left, budget - left)
}

fn removed_units_for_source(
    policy: TruncationPolicy,
    removed_bytes: usize,
    removed_chars: usize,
) -> u64 {
    match policy {
        TruncationPolicy::Tokens(_) => approx_tokens_from_byte_count(removed_bytes),
        TruncationPolicy::Bytes(_) => u64::try_from(removed_chars).unwrap_or(u64::MAX),
    }
}

fn assemble_truncated_output(prefix: &str, suffix: &str, marker: &str) -> String {
    let mut out = String::with_capacity(prefix.len() + marker.len() + suffix.len() + 1);
    out.push_str(prefix);
    out.push_str(marker);
    out.push_str(suffix);
    out
}

pub(crate) fn approx_token_count(text: &str) -> usize {
    let len = text.len();
    len.saturating_add(APPROX_BYTES_PER_TOKEN.saturating_sub(1)) / APPROX_BYTES_PER_TOKEN
}

fn approx_bytes_for_tokens(tokens: usize) -> usize {
    tokens.saturating_mul(APPROX_BYTES_PER_TOKEN)
}

pub(crate) fn approx_tokens_from_byte_count(bytes: usize) -> u64 {
    let bytes_u64 = bytes as u64;
    bytes_u64.saturating_add((APPROX_BYTES_PER_TOKEN as u64).saturating_sub(1))
        / (APPROX_BYTES_PER_TOKEN as u64)
}

#[cfg(test)]
mod tests {

    use super::TruncationPolicy;
    use super::formatted_truncate_text;
    use super::split_string;
    use super::truncate_text;
    use super::truncate_with_token_budget;
    use pretty_assertions::assert_eq;

    #[test]
    fn split_string_works() {
        assert_eq!(split_string("hello world", 5, 5), (1, "hello", "world"));
        assert_eq!(split_string("abc", 0, 0), (3, "", ""));
    }

    #[test]
    fn split_string_handles_empty_string() {
        assert_eq!(split_string("", 4, 4), (0, "", ""));
    }

    #[test]
    fn split_string_only_keeps_prefix_when_tail_budget_is_zero() {
        assert_eq!(split_string("abcdef", 3, 0), (3, "abc", ""));
    }

    #[test]
    fn split_string_only_keeps_suffix_when_prefix_budget_is_zero() {
        assert_eq!(split_string("abcdef", 0, 3), (3, "", "def"));
    }

    #[test]
    fn split_string_handles_overlapping_budgets_without_removal() {
        assert_eq!(split_string("abcdef", 4, 4), (0, "abcd", "ef"));
    }

    #[test]
    fn split_string_respects_utf8_boundaries() {
        assert_eq!(split_string("😀abc😀", 5, 5), (1, "😀a", "c😀"));

        assert_eq!(split_string("😀😀😀😀😀", 1, 1), (5, "", ""));
        assert_eq!(split_string("😀😀😀😀😀", 7, 7), (3, "😀", "😀"));
        assert_eq!(split_string("😀😀😀😀😀", 8, 8), (1, "😀😀", "😀😀"));
    }

    #[test]
    fn truncate_bytes_less_than_placeholder_returns_placeholder() {
        let content = "example output";

        assert_eq!(
            "Total output lines: 1\n\n…13 chars truncated…t",
            formatted_truncate_text(content, TruncationPolicy::Bytes(1)),
        );
    }

    #[test]
    fn truncate_tokens_less_than_placeholder_returns_placeholder() {
        let content = "example output";

        assert_eq!(
            "Total output lines: 1\n\nex…3 tokens truncated…ut",
            formatted_truncate_text(content, TruncationPolicy::Tokens(1)),
        );
    }

    #[test]
    fn truncate_tokens_under_limit_returns_original() {
        let content = "example output";

        assert_eq!(
            content,
            formatted_truncate_text(content, TruncationPolicy::Tokens(10)),
        );
    }

    #[test]
    fn truncate_bytes_under_limit_returns_original() {
        let content = "example output";

        assert_eq!(
            content,
            formatted_truncate_text(content, TruncationPolicy::Bytes(20)),
        );
    }

    #[test]
    fn truncate_tokens_over_limit_returns_truncated() {
        let content = "this is an example of a long output that should be truncated";

        assert_eq!(
            "Total output lines: 1\n\nthis is an…10 tokens truncated… truncated",
            formatted_truncate_text(content, TruncationPolicy::Tokens(5)),
        );
    }

    #[test]
    fn truncate_bytes_over_limit_returns_truncated() {
        let content = "this is an example of a long output that should be truncated";

        assert_eq!(
            "Total output lines: 1\n\nthis is an exam…30 chars truncated…ld be truncated",
            formatted_truncate_text(content, TruncationPolicy::Bytes(30)),
        );
    }

    #[test]
    fn truncate_bytes_reports_original_line_count_when_truncated() {
        let content =
            "this is an example of a long output that should be truncated\nalso some other line";

        assert_eq!(
            "Total output lines: 2\n\nthis is an exam…51 chars truncated…some other line",
            formatted_truncate_text(content, TruncationPolicy::Bytes(30)),
        );
    }

    #[test]
    fn truncate_tokens_reports_original_line_count_when_truncated() {
        let content =
            "this is an example of a long output that should be truncated\nalso some other line";

        assert_eq!(
            "Total output lines: 2\n\nthis is an example o…11 tokens truncated…also some other line",
            formatted_truncate_text(content, TruncationPolicy::Tokens(10)),
        );
    }

    #[test]
    fn truncate_with_token_budget_returns_original_when_under_limit() {
        let s = "short output";
        let limit = 100;
        let (out, original) = truncate_with_token_budget(s, TruncationPolicy::Tokens(limit));
        assert_eq!(out, s);
        assert_eq!(original, None);
    }

    #[test]
    fn truncate_with_token_budget_reports_truncation_at_zero_limit() {
        let s = "abcdef";
        let (out, original) = truncate_with_token_budget(s, TruncationPolicy::Tokens(0));
        assert_eq!(out, "…2 tokens truncated…");
        assert_eq!(original, Some(2));
    }

    #[test]
    fn truncate_middle_tokens_handles_utf8_content() {
        let s = "😀😀😀😀😀😀😀😀😀😀\nsecond line with text\n";
        let (out, tokens) = truncate_with_token_budget(s, TruncationPolicy::Tokens(8));
        assert_eq!(out, "😀😀😀😀…8 tokens truncated… line with text\n");
        assert_eq!(tokens, Some(16));
    }

    #[test]
    fn truncate_middle_bytes_handles_utf8_content() {
        let s = "😀😀😀😀😀😀😀😀😀😀\nsecond line with text\n";
        let out = truncate_text(s, TruncationPolicy::Bytes(20));
        assert_eq!(out, "😀😀…21 chars truncated…with text\n");
    }
}
