//! Integration tests for ACP wire API support in ModelClient

use std::sync::Arc;

use codex_app_server_protocol::AuthMode;
use codex_core::ContentItem;
use codex_core::ModelClient;
use codex_core::ModelProviderInfo;
use codex_core::Prompt;
use codex_core::ResponseEvent;
use codex_core::ResponseItem;
use codex_core::WireApi;
use codex_otel::otel_event_manager::OtelEventManager;
use codex_protocol::ConversationId;
use codex_protocol::protocol::SessionSource;
use core_test_support::load_default_config_for_test;
use futures::StreamExt;
use tempfile::TempDir;

#[tokio::test]
async fn test_acp_stream_with_mock_agent() {
    // Create ACP provider for mock-acp-agent
    let provider = ModelProviderInfo {
        name: "mock-acp".into(),
        base_url: None, // ACP uses subprocess, not HTTP
        env_key: None,
        env_key_instructions: None,
        experimental_bearer_token: None,
        wire_api: WireApi::Acp,
        query_params: None,
        http_headers: None,
        env_http_headers: None,
        request_max_retries: Some(0),
        stream_max_retries: Some(0),
        stream_idle_timeout_ms: Some(5_000),
        requires_openai_auth: false,
    };

    // Load default config
    let codex_home = TempDir::new().expect("Failed to create temp dir");
    let mut config = load_default_config_for_test(&codex_home);
    config.model = "mock-model".to_string(); // Use model name registered in ACP registry
    config.model_provider_id = provider.name.clone();
    config.model_provider = provider.clone();
    let effort = config.model_reasoning_effort;
    let summary = config.model_reasoning_summary;
    let config = Arc::new(config);

    let conversation_id = ConversationId::new();

    let otel_event_manager = OtelEventManager::new(
        conversation_id,
        config.model.as_str(),
        config.model_family.slug.as_str(),
        None,
        Some("test@test.com".to_string()),
        Some(AuthMode::ChatGPT),
        false,
        "test".to_string(),
    );

    // Create ModelClient
    let client = ModelClient::new(
        Arc::clone(&config),
        None, // no auth manager needed for mock
        otel_event_manager,
        provider,
        effort,
        summary,
        conversation_id,
        SessionSource::Exec,
    );

    // Create simple prompt
    let mut prompt = Prompt::default();
    prompt.input = vec![ResponseItem::Message {
        id: None,
        role: "user".to_string(),
        content: vec![ContentItem::InputText {
            text: "Hello".to_string(),
        }],
    }];

    // Stream response
    let mut stream = client.stream(&prompt).await.expect("Stream should start");

    // Collect events
    let mut events = Vec::new();
    while let Some(event_result) = stream.next().await {
        let event = event_result.expect("Event should not be error");
        events.push(event);
    }

    // Verify we got the expected messages from mock agent
    let text_deltas: Vec<String> = events
        .iter()
        .filter_map(|e| {
            if let ResponseEvent::OutputTextDelta(text) = e {
                Some(text.clone())
            } else {
                None
            }
        })
        .collect();

    // Mock agent sends "Test message 1" and "Test message 2"
    assert!(
        text_deltas.contains(&"Test message 1".to_string()),
        "Should receive 'Test message 1' from mock agent. Got: {:?}",
        text_deltas
    );
    assert!(
        text_deltas.contains(&"Test message 2".to_string()),
        "Should receive 'Test message 2' from mock agent. Got: {:?}",
        text_deltas
    );

    // Verify we got a Completed event
    let completed = events
        .iter()
        .any(|e| matches!(e, ResponseEvent::Completed { .. }));
    assert!(completed, "Should receive Completed event");
}
