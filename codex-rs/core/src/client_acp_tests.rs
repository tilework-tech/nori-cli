//! Unit tests for ACP wire API implementation
//!
//! Note: Most ACP provider tests have moved to the codex-acp crate.
//! This file contains only core-specific ACP integration tests.

#[cfg(test)]
mod tests {
    #[test]
    fn test_acp_get_full_url_returns_empty() {
        use crate::ModelProviderInfo;
        use crate::WireApi;

        let provider = ModelProviderInfo {
            name: "test-acp".into(),
            base_url: None,
            env_key: None,
            env_key_instructions: None,
            experimental_bearer_token: None,
            wire_api: WireApi::Acp,
            query_params: None,
            http_headers: None,
            env_http_headers: None,
            request_max_retries: None,
            stream_max_retries: None,
            stream_idle_timeout_ms: None,
            requires_openai_auth: false,
        };

        let url = provider.get_full_url(&None);
        assert_eq!(url, "", "ACP provider should return empty URL");
    }

    #[test]
    fn test_mock_acp_model_has_family() {
        use crate::model_family::find_family_for_model;

        let family = find_family_for_model("mock-acp");
        assert!(
            family.is_some(),
            "mock-acp model should have a model family"
        );
    }

    #[test]
    fn test_gemini_acp_model_has_family() {
        use crate::model_family::find_family_for_model;

        let family = find_family_for_model("gemini-acp");
        assert!(
            family.is_some(),
            "gemini-acp model should have a model family"
        );
    }

    #[test]
    fn test_claude_acp_model_has_family() {
        use crate::model_family::find_family_for_model;

        let family = find_family_for_model("claude-acp");
        assert!(
            family.is_some(),
            "claude-acp model should have a model family"
        );
    }
}
