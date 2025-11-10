use nori_cli::backends::AgentBackend;
use nori_cli::backends::mock::MockBackend;
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
async fn test_parse_agent_message_events() {
    let backend = MockBackend;
    let mut child = backend.spawn_process("test".to_string()).await.unwrap();

    let stdout = child.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout).lines();

    let mut messages = Vec::new();
    while let Some(line) = reader.next_line().await.unwrap() {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&line) {
            if json.get("type").and_then(|v| v.as_str()) == Some("agent_message") {
                if let Some(content) = json.get("content").and_then(|v| v.as_str()) {
                    messages.push(content.to_string());
                }
            }
        }
    }

    assert!(!messages.is_empty());
}
