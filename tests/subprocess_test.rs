use futures::StreamExt;
use nori_cli::backends::AgentBackend;
use nori_cli::backends::mock::MockBackend;
use nori_cli::conversation::ConversationEvent;
use tokio_util::sync::CancellationToken;

#[tokio::test]
async fn test_mock_backend_streams_events() {
    let backend = MockBackend::new();
    let cancel_token = CancellationToken::new();
    let mut stream = backend.spawn_stream("test prompt".to_string(), cancel_token);

    let mut events = Vec::new();
    while let Some(event) = stream.next().await {
        events.push(event);
    }

    // MockBackend should yield at least some events
    assert!(!events.is_empty());

    // First events should be assistant messages
    assert!(matches!(
        events[0],
        ConversationEvent::AssistantMessage { .. }
    ));
}

#[tokio::test]
async fn test_mock_backend_completes() {
    let backend = MockBackend::new();
    let cancel_token = CancellationToken::new();
    let mut stream = backend.spawn_stream("test".to_string(), cancel_token.clone());

    let mut event_count = 0;
    while let Some(event) = stream.next().await {
        event_count += 1;
        if matches!(event, ConversationEvent::AssistantMessage { .. }) {
            break;
        }
    }

    cancel_token.cancel();
    while let Some(_event) = stream.next().await {
        event_count += 1;
    }

    assert!(
        event_count > 0,
        "mock backend should produce at least one event before completing"
    );
}
