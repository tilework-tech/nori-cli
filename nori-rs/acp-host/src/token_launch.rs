//! Apply the selected credential only to the ACP child, after all reusable
//! configuration is built. Codex ACP logs its JSON config at startup.
use std::borrow::Cow;
use std::sync::Arc;

use anyhow::Context;
use anyhow::Result;
use anyhow::ensure;
use nori_config::token_providers::Preset;
use nori_config::token_providers::TokenProvider;
use serde_json::json;
use tokio::process::Command;

const CHILD_KEY: &str = "NORI_PROVIDER_API_KEY";

pub(crate) fn credential(
    binding: &TokenProvider,
    lookup: &dyn Fn(&str) -> Option<String>,
) -> Result<Arc<str>> {
    let key = lookup(&binding.env_key)
        .filter(|s| !s.trim().is_empty())
        .with_context(|| {
            format!(
                "token provider requires environment variable {}",
                binding.env_key
            )
        })?;
    ensure!(
        !key.chars().any(char::is_control),
        "token provider credential contains control characters"
    );
    Ok(Arc::from(key))
}

pub(crate) fn apply_binding(
    cmd: &mut Command,
    kind: Option<crate::registry::AgentKind>,
    binding: &TokenProvider,
) -> Result<Arc<str>> {
    let key = credential(binding, &|key| std::env::var(key).ok())?;
    if kind == Some(crate::registry::AgentKind::Pi) {
        cmd.env(&binding.env_key, key.as_ref());
        return Ok(key);
    }
    ensure!(
        kind == Some(crate::registry::AgentKind::Codex),
        "no named token provider launch adapter"
    );
    let mut provider = json!({
        "name": binding.name,
        "base_url": binding.base_url,
        "wire_api": "responses",
        "env_key": CHILD_KEY,
        "requires_openai_auth": false,
    });
    if binding.preset == Preset::Subconscious {
        provider["http_headers"] = json!({"x-subconscious-client": "codex"});
    }
    let config = json!({
        "features": {"goals": false},
        "model": binding.model,
        "model_provider": "nori_binding",
        "model_providers": {"nori_binding": provider},
    });
    cmd.env_remove("DEFAULT_AUTH_REQUEST")
        .env_remove("OPENAI_API_KEY")
        .env_remove("CODEX_API_KEY")
        .env("MODEL_PROVIDER", "nori_binding")
        .env("CODEX_CONFIG", config.to_string())
        .env(CHILD_KEY, key.as_ref());
    Ok(key)
}

pub(crate) fn redact<'a>(text: &'a str, key: Option<&str>) -> Cow<'a, str> {
    match key {
        Some(key) if key.contains(['"', '\\']) && contains_key(text, key) => {
            Cow::Borrowed("[redacted]")
        }
        Some(key) if text.contains(key) => Cow::Owned(text.replace(key, "[redacted]")),
        _ => Cow::Borrowed(text),
    }
}

/// JSON escapes quotes and backslashes in credentials.
pub(crate) fn contains_key(line: &str, key: &str) -> bool {
    line.contains(key)
        || ((key.contains('"') || key.contains('\\'))
            && serde_json::to_string(key)
                .is_ok_and(|quoted| line.contains(&quoted[1..quoted.len() - 1])))
}

/// Preserve RPC identity and successful payloads. Adapter errors can echo a
/// credential, including inside structured data consumed by error diagnostics.
pub(crate) fn redact_error(line: String, key: &str) -> String {
    if !line.contains("\"error\"") || !contains_key(&line, key) {
        return line;
    }
    if let Ok(mut packet) = serde_json::from_str::<serde_json::Value>(&line)
        && let Some(error) = packet
            .get_mut("error")
            .and_then(serde_json::Value::as_object_mut)
        && error.values().any(|v| contains_key(&v.to_string(), key))
    {
        error.insert("message".into(), "[redacted]".into());
        error.remove("data");
        packet.to_string()
    } else {
        line
    }
}
