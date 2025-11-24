//! Thin slice integration test for AcpModelClient
//!
//! Tests the minimal end-to-end flow: spawn → initialize → session → prompt → stream → complete

use codex_acp::AcpModelClient;
use futures::StreamExt;
use std::path::PathBuf;
use std::time::Duration;
use tokio::time::timeout;

/// Get path to the mock-acp-agent binary
fn mock_agent_binary_path() -> String {
    let test_exe = std::env::current_exe().expect("Failed to get current exe path");
    let target_debug = test_exe
        .parent() // deps
        .and_then(|p| p.parent()) // debug
        .expect("Failed to get target/debug directory");
    target_debug
        .join("mock_acp_agent")
        .to_string_lossy()
        .into_owned()
}

#[tokio::test]
#[ignore] // Requires mock-acp-agent to be built and available
async fn test_thin_slice_text_streaming() {
    // Get mock agent binary
    let binary_path = mock_agent_binary_path();

    // Create AcpModelClient
    let client = AcpModelClient::new(binary_path, vec![], PathBuf::from("/tmp"));

    // Stream a simple prompt
    let stream = client
        .stream("Hello, ACP agent!")
        .await
        .expect("Failed to start stream");

    // Collect all events with timeout
    let events: Vec<_> = timeout(Duration::from_secs(5), stream.collect())
        .await
        .expect("Stream timed out");

    // Verify we got at least one text delta
    let has_text_delta = events.iter().any(|event| match event {
        Ok(codex_acp::AcpEvent::TextDelta(_)) => true,
        _ => false,
    });
    assert!(
        has_text_delta,
        "Expected at least one TextDelta event, got: {:?}",
        events
    );

    // Verify stream completed
    let has_completed = events.iter().any(|event| match event {
        Ok(codex_acp::AcpEvent::Completed { .. }) => true,
        _ => false,
    });
    assert!(has_completed, "Expected Completed event, got: {:?}", events);
}

#[tokio::test]
async fn test_thin_slice_agent_not_found() {
    // Test error handling when agent binary doesn't exist
    let client = AcpModelClient::new(
        "/nonexistent/agent".to_string(),
        vec![],
        PathBuf::from("/tmp"),
    );

    // Stream creation succeeds (spawns thread), but events will contain errors
    let stream = client.stream("test").await;
    assert!(
        stream.is_ok(),
        "Stream should be created even if agent will fail"
    );

    // Collect the first event (should be an error)
    use futures::StreamExt;
    let mut stream = stream.unwrap();
    let first_event = stream.next().await;
    assert!(first_event.is_some(), "Should get at least one event");

    let event = first_event.unwrap();
    assert!(
        event.is_err(),
        "First event should be an error for nonexistent agent"
    );
}
