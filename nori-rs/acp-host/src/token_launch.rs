//! Apply the selected credential only to the ACP child, after all reusable
//! configuration is built. Codex ACP logs its JSON config at startup.
use std::borrow::Cow;
use std::sync::Arc;

use anyhow::Context;
use anyhow::Result;
use anyhow::ensure;
use nori_config::token_providers::TokenProvider;
use serde_json::json;

pub(crate) const CHILD_KEY: &str = "NORI_PROVIDER_API_KEY";

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

/// Prepare the adapter's native config with a key *reference*. The process
/// supervisor resolves the actual credential when it launches the child.
pub(crate) fn configure_codex(
    config: &mut crate::registry::AcpAgentConfig,
    binding: &TokenProvider,
) {
    let provider = json!({
        "name": binding.name,
        "base_url": binding.base_url,
        "wire_api": "responses",
        "env_key": CHILD_KEY,
        "requires_openai_auth": false,
    });
    let settings = json!({
        "features": {"goals": false},
        "model": binding.model,
        "model_provider": "nori_binding",
        "model_providers": {"nori_binding": provider},
    });
    config.launch.env_remove.extend([
        "DEFAULT_AUTH_REQUEST".to_owned(),
        "OPENAI_API_KEY".to_owned(),
        "CODEX_API_KEY".to_owned(),
        binding.env_key.clone(),
    ]);
    config
        .env
        .insert("MODEL_PROVIDER".into(), "nori_binding".into());
    config
        .env
        .insert("CODEX_CONFIG".into(), settings.to_string());
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
