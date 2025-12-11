//! OSS provider utilities shared between TUI and exec.
//!
//! When the `oss-providers` feature is enabled, this module provides full support for
//! Ollama and LM Studio providers. When disabled, stub implementations are provided
//! that return `None` or errors, matching the behavior when providers are unavailable.

use codex_core::LMSTUDIO_OSS_PROVIDER_ID;
use codex_core::OLLAMA_OSS_PROVIDER_ID;
use codex_core::config::Config;

/// Returns the default model for a given OSS provider.
///
/// When `oss-providers` feature is disabled, always returns `None`.
#[cfg(feature = "oss-providers")]
pub fn get_default_model_for_oss_provider(provider_id: &str) -> Option<&'static str> {
    match provider_id {
        LMSTUDIO_OSS_PROVIDER_ID => Some(codex_lmstudio::DEFAULT_OSS_MODEL),
        OLLAMA_OSS_PROVIDER_ID => Some(codex_ollama::DEFAULT_OSS_MODEL),
        _ => None,
    }
}

/// Returns the default model for a given OSS provider.
///
/// Stub implementation when `oss-providers` feature is disabled - always returns `None`.
#[cfg(not(feature = "oss-providers"))]
pub fn get_default_model_for_oss_provider(_provider_id: &str) -> Option<&'static str> {
    None
}

/// Ensures the specified OSS provider is ready (models downloaded, service reachable).
///
/// When `oss-providers` feature is disabled, returns an error for known providers.
#[cfg(feature = "oss-providers")]
pub async fn ensure_oss_provider_ready(
    provider_id: &str,
    config: &Config,
) -> Result<(), std::io::Error> {
    match provider_id {
        LMSTUDIO_OSS_PROVIDER_ID => {
            codex_lmstudio::ensure_oss_ready(config)
                .await
                .map_err(|e| std::io::Error::other(format!("OSS setup failed: {e}")))?;
        }
        OLLAMA_OSS_PROVIDER_ID => {
            codex_ollama::ensure_oss_ready(config)
                .await
                .map_err(|e| std::io::Error::other(format!("OSS setup failed: {e}")))?;
        }
        _ => {
            // Unknown provider, skip setup
        }
    }
    Ok(())
}

/// Ensures the specified OSS provider is ready (models downloaded, service reachable).
///
/// Stub implementation when `oss-providers` feature is disabled - returns error for known providers.
#[cfg(not(feature = "oss-providers"))]
pub async fn ensure_oss_provider_ready(
    provider_id: &str,
    _config: &Config,
) -> Result<(), std::io::Error> {
    match provider_id {
        LMSTUDIO_OSS_PROVIDER_ID | OLLAMA_OSS_PROVIDER_ID => Err(std::io::Error::other(
            "OSS providers are not available in this build (oss-providers feature disabled)",
        )),
        _ => {
            // Unknown provider, skip setup
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "oss-providers")]
    #[test]
    fn test_get_default_model_for_provider_lmstudio() {
        let result = get_default_model_for_oss_provider(LMSTUDIO_OSS_PROVIDER_ID);
        assert_eq!(result, Some(codex_lmstudio::DEFAULT_OSS_MODEL));
    }

    #[cfg(feature = "oss-providers")]
    #[test]
    fn test_get_default_model_for_provider_ollama() {
        let result = get_default_model_for_oss_provider(OLLAMA_OSS_PROVIDER_ID);
        assert_eq!(result, Some(codex_ollama::DEFAULT_OSS_MODEL));
    }

    #[test]
    fn test_get_default_model_for_provider_unknown() {
        let result = get_default_model_for_oss_provider("unknown-provider");
        assert_eq!(result, None);
    }

    /// Test that stub returns None for known providers when feature is disabled.
    #[cfg(not(feature = "oss-providers"))]
    #[test]
    fn test_get_default_model_stub_returns_none() {
        assert_eq!(
            get_default_model_for_oss_provider(LMSTUDIO_OSS_PROVIDER_ID),
            None
        );
        assert_eq!(
            get_default_model_for_oss_provider(OLLAMA_OSS_PROVIDER_ID),
            None
        );
    }

    /// Test that ensure_oss_provider_ready returns error for known providers when disabled.
    #[cfg(not(feature = "oss-providers"))]
    #[tokio::test]
    async fn test_ensure_oss_provider_ready_stub_returns_error() {
        use codex_core::config::Config;
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let config = Config::load_from_base_config_with_overrides(
            codex_core::config::ConfigToml::default(),
            codex_core::config::ConfigOverrides::default(),
            temp_dir.path().to_path_buf(),
        )
        .unwrap();

        let result = ensure_oss_provider_ready(LMSTUDIO_OSS_PROVIDER_ID, &config).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not available"));

        let result = ensure_oss_provider_ready(OLLAMA_OSS_PROVIDER_ID, &config).await;
        assert!(result.is_err());

        // Unknown provider should still succeed
        let result = ensure_oss_provider_ready("unknown-provider", &config).await;
        assert!(result.is_ok());
    }
}
