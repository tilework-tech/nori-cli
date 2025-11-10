use nori_cli::backends::mock::MockBackend;
use nori_cli::backends::AgentBackend;
use nori_cli::conversation::{parse_jsonl_event, ConversationEvent};
use tokio::io::{AsyncBufReadExt, BufReader};

#[tokio::test]
async fn test_mock_backend_spawns_process() {
    let backend = MockBackend;
    let mut child = backend
        .spawn_process("test prompt".to_string())
        .await
        .unwrap();

    // Read stdout
    let stdout = child.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout).lines();

    let mut lines = Vec::new();
    while let Some(line) = reader.next_line().await.unwrap() {
        lines.push(line);
    }

    // MockBackend should output JSONL
    assert!(!lines.is_empty());

    // First line should be parseable JSON
    let _: serde_json::Value = serde_json::from_str(&lines[0]).unwrap();
}

#[tokio::test]
async fn test_parse_assistant_message_events() {
    let backend = MockBackend;
    let mut child = backend.spawn_process("test".to_string()).await.unwrap();

    let stdout = child.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout).lines();

    let mut events = Vec::new();
    while let Some(line) = reader.next_line().await.unwrap() {
        if let Some(event) = parse_jsonl_event(&line) {
            events.push(event);
        }
    }

    assert!(!events.is_empty());
    assert!(matches!(
        events[0],
        ConversationEvent::AssistantMessage { .. }
    ));
}
