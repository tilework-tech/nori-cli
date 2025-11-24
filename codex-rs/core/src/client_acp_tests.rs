//! Unit tests for ACP wire API implementation

#[cfg(test)]
mod tests {
    use crate::model_provider_info::WireApi;
    use crate::model_provider_info::built_in_model_providers;

    #[test]
    fn test_mock_acp_provider_exists() {
        let providers = built_in_model_providers();
        let mock_acp = providers.get("mock-acp");

        assert!(
            mock_acp.is_some(),
            "mock-acp provider should exist in built-in providers"
        );
    }

    #[test]
    fn test_mock_acp_provider_uses_acp_wire_api() {
        let providers = built_in_model_providers();
        let mock_acp = providers.get("mock-acp").expect("mock-acp should exist");

        assert_eq!(
            mock_acp.wire_api,
            WireApi::Acp,
            "mock-acp should use WireApi::Acp"
        );
    }

    #[test]
    fn test_gemini_acp_provider_exists() {
        let providers = built_in_model_providers();
        let gemini_acp = providers.get("gemini-acp");

        assert!(
            gemini_acp.is_some(),
            "gemini-acp provider should exist in built-in providers"
        );
    }

    #[test]
    fn test_gemini_acp_provider_uses_acp_wire_api() {
        let providers = built_in_model_providers();
        let gemini_acp = providers
            .get("gemini-acp")
            .expect("gemini-acp should exist");

        assert_eq!(
            gemini_acp.wire_api,
            WireApi::Acp,
            "gemini-acp should use WireApi::Acp"
        );
    }

    #[test]
    fn test_acp_registry_integration() {
        // Verify that the ACP registry can be called from core using model names
        let mock_config = codex_acp::get_agent_config("mock-model");
        assert!(
            mock_config.is_ok(),
            "Should be able to get config for mock-model from registry"
        );

        let config = mock_config.unwrap();
        assert_eq!(config.provider, "mock-acp");
        assert!(
            config.command.contains("mock_acp_agent"),
            "Command should contain 'mock_acp_agent'"
        );
        assert_eq!(config.args, Vec::<String>::new());
    }

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
}
