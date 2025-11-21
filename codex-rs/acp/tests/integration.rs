//! Integration tests for ACP agent communication
//!
//! These tests verify end-to-end communication with ACP agents.
//! The mock-acp-agent package from /mock-acp-agent is used for testing.

use codex_acp::AgentProcess;
use serde_json::json;
use std::time::Duration;
use tokio::time::timeout;

#[tokio::test]
#[ignore] // Requires mock-acp-agent package
async fn test_full_acp_flow_with_mock_agent() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            // Spawn mock ACP agent
            let args = vec!["../../../mock-acp-agent".to_string()];
            let mut agent = AgentProcess::spawn("node", &args, &[])
                .await
                .expect("Failed to spawn mock ACP agent");

            // Initialize agent
            let client_caps = json!({
                "tools": ["read_file", "write_file"],
                "streaming": true,
            });

            let init_result = timeout(Duration::from_secs(5), agent.initialize(client_caps))
                .await
                .expect("Initialize timed out")
                .expect("Initialize failed");

            assert!(init_result.is_object());
            println!("Agent capabilities: {init_result:?}");

            // Create a new session
            let session_id = timeout(
                Duration::from_secs(5),
                agent.new_session("/tmp".to_string(), vec![]),
            )
            .await
            .expect("Session request timed out")
            .expect("Session request failed");

            println!("Session created: {}", session_id);

            // Send a prompt
            let prompt_content = vec![json!({
                "type": "text",
                "text": "Hello, ACP agent!",
            })];

            let prompt_response = timeout(
                Duration::from_secs(10),
                agent.prompt(session_id, prompt_content),
            )
            .await
            .expect("Prompt request timed out")
            .expect("Prompt request failed");

            assert!(prompt_response.is_object());
            println!("Prompt response: {:?}", prompt_response);

            agent.kill().await.ok();
        })
        .await;
}

#[tokio::test]
async fn test_acp_protocol_validation() {
    // Verify our integration with agent-client-protocol library
    use agent_client_protocol::{InitializeRequest, V1};

    // Request must have proper fields
    let request = InitializeRequest {
        protocol_version: V1,
        client_capabilities: serde_json::from_value(json!({"test": true})).unwrap(),
        client_info: None,
        meta: None,
    };

    let serialized = serde_json::to_string(&request).unwrap();
    // Just verify it serializes successfully - the exact format is handled by the library
    assert!(!serialized.is_empty());
    assert!(serialized.contains("protocolVersion"));
}
