//! Recognising API keys by their public prefix, so a key can be shown without
//! revealing any of its secret part.

/// Public key prefixes, longest first so the most specific one matches. Each
/// is a fixed constant the issuer puts on every key, so it carries no secret.
const PREFIXES: &[(&str, &str)] = &[
    ("sk-ant-admin01-", "Anthropic"),
    ("sk-ant-api03-", "Anthropic"),
    ("sk-ant-oat01-", "Anthropic"),
    ("sk-svcacct-", "OpenAI"),
    ("sk-or-v1-", "OpenRouter"),
    ("sk-admin-", "OpenAI"),
    ("sk-proj-", "OpenAI"),
    ("AIza", "Google"),
    ("xai-", "xAI"),
    ("gsk_", "Groq"),
    // Shared by OpenAI's older keys and several other providers.
    ("sk-", ""),
];

/// The known public prefix `key` starts with, and the issuer it names (empty
/// when the prefix is shared between issuers).
pub fn key_prefix(key: &str) -> Option<(&'static str, &'static str)> {
    PREFIXES
        .iter()
        .copied()
        .find(|(prefix, _)| key.starts_with(prefix))
}

/// `key` with its known public prefix kept and every other character replaced
/// by `*`. Keys without a known prefix are masked entirely.
pub fn masked_key(key: &str) -> String {
    let prefix = key_prefix(key).map_or("", |(prefix, _)| prefix);
    let hidden = key[prefix.len()..].chars().count();
    format!("{prefix}{}", "*".repeat(hidden))
}
