use std::process::Command;
use std::sync::Mutex;
use std::time::Duration;

use futures::StreamExt;
use nori_cli::acp_runner::{AcpAgentConfig, AcpAgentRunner};
use nori_cli::conversation::ConversationEvent;
use tempfile::tempdir;
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;

const MOCK_AGENT_COMMAND: &str = "target/debug/mock_acp_agent";

static TEST_GUARD: once_cell::sync::Lazy<Mutex<()>> = once_cell::sync::Lazy::new(|| Mutex::new(()));

fn acquire_test_guard<'a>() -> std::sync::MutexGuard<'a, ()> {
    TEST_GUARD
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn set_test_env(key: &str, value: impl AsRef<std::ffi::OsStr>) {
    unsafe {
        std::env::set_var(key, value);
    }
}

fn remove_test_env(key: &str) {
    unsafe {
        std::env::remove_var(key);
    }
}

fn build_mock_agent() {
    let status = Command::new("cargo")
        .env("CARGO_TARGET_DIR", "target")
        .args(["build", "--manifest-path", "mock-acp-agent/Cargo.toml"])
        .status()
        .expect("Failed to build mock agent");
    assert!(
        status.success(),
        "Mock agent build failed with status {status:?}"
    );
}

fn mock_agent_config() -> AcpAgentConfig {
    AcpAgentConfig {
        name: "mock",
        command: MOCK_AGENT_COMMAND,
        args: vec![],
        install_url: "",
        install_command: None,
    }
}

#[tokio::test(flavor = "current_thread")]
async fn test_acp_handshake_succeeds() {
    let _guard = acquire_test_guard();
    build_mock_agent();
    let temp_dir = tempdir().unwrap();
    let mut runner = AcpAgentRunner::new(mock_agent_config(), temp_dir.path().to_path_buf());
    let cancel_token = CancellationToken::new();

    let result = runner
        .spawn_stream("test prompt".to_string(), cancel_token.clone())
        .await;

    let stream = result.unwrap_or_else(|err| panic!("spawn_stream should succeed: {err}"));
    cancel_token.cancel();
    drop(stream);
}

#[tokio::test(flavor = "current_thread")]
async fn test_session_updates_are_streamed() {
    let _guard = acquire_test_guard();
    build_mock_agent();
    let temp_dir = tempdir().unwrap();
    let mut runner = AcpAgentRunner::new(mock_agent_config(), temp_dir.path().to_path_buf());
    let cancel_token = CancellationToken::new();

    let mut stream = runner
        .spawn_stream("collect updates".to_string(), cancel_token.clone())
        .await
        .expect("spawn_stream should succeed");

    // Find both test messages, skipping debug events
    let mut found_messages = Vec::new();

    for _ in 0..20 { // Allow up to 20 events to find both messages
        let event = timeout(Duration::from_secs(5), stream.next())
            .await
            .expect("timed out waiting for event")
            .expect("stream closed before event");

        if let ConversationEvent::AssistantMessage { ref text } = event {
            if text == "Test message 1" || text == "Test message 2" {
                found_messages.push(event);
                if found_messages.len() == 2 {
                    break;
                }
            }
        }
    }

    assert_eq!(found_messages.len(), 2, "Did not find both test messages");

    assert!(
        matches!(
            &found_messages[0],
            ConversationEvent::AssistantMessage {
                text
            } if text == "Test message 1"
        ),
        "first message mismatch: {found_messages:?}"
    );

    assert!(
        matches!(
            &found_messages[1],
            ConversationEvent::AssistantMessage {
                text
            } if text == "Test message 2"
        ),
        "second message mismatch: {found_messages:?}"
    );

    cancel_token.cancel();
}

#[tokio::test(flavor = "current_thread")]
async fn test_agent_calls_read_text_file() {
    let _guard = acquire_test_guard();
    build_mock_agent();
    let temp_dir = tempdir().unwrap();
    let file_path = temp_dir.path().join("sample.txt");
    std::fs::write(&file_path, "Hello from file").expect("write sample file");
    set_test_env("MOCK_AGENT_REQUEST_FILE", &file_path);

    let mut runner = AcpAgentRunner::new(mock_agent_config(), temp_dir.path().to_path_buf());
    let cancel_token = CancellationToken::new();

    let mut stream = runner
        .spawn_stream("request file".to_string(), cancel_token.clone())
        .await
        .expect("spawn_stream should succeed");

    // Skip debug events and find the two test messages
    let mut found_messages = 0;
    for _ in 0..20 {
        // Allow more events due to debug logging
        let event = timeout(Duration::from_secs(5), stream.next())
            .await
            .unwrap_or_else(|_| panic!("timed out waiting for event"))
            .unwrap_or_else(|| panic!("stream closed"));

        match event {
            ConversationEvent::AssistantMessage { ref text }
                if text == "Test message 1" || text == "Test message 2" =>
            {
                found_messages += 1;
                if found_messages == 2 {
                    break;
                }
            }
            _ => continue, // Skip debug events
        }
    }
    assert_eq!(found_messages, 2, "Did not find both test messages");

    // Now find the file content message
    let mut file_content_event = None;
    for _ in 0..10 {
        let event = timeout(Duration::from_secs(5), stream.next())
            .await
            .expect("timed out waiting for file read event")
            .expect("stream closed without file read event");

        if let ConversationEvent::AssistantMessage { ref text } = event {
            if text.contains("Read file content: Hello from file") {
                file_content_event = Some(event);
                break;
            }
        }
    }

    assert!(
        file_content_event.is_some(),
        "Did not find file content message"
    );

    cancel_token.cancel();
    remove_test_env("MOCK_AGENT_REQUEST_FILE");
}

#[tokio::test(flavor = "current_thread")]
async fn test_cancellation_stops_stream() {
    let _guard = acquire_test_guard();
    build_mock_agent();
    set_test_env("MOCK_AGENT_STREAM_UNTIL_CANCEL", "1");
    let temp_dir = tempdir().unwrap();
    let mut runner = AcpAgentRunner::new(mock_agent_config(), temp_dir.path().to_path_buf());
    let cancel_token = CancellationToken::new();

    let mut stream = runner
        .spawn_stream("long running".to_string(), cancel_token.clone())
        .await
        .expect("spawn_stream should succeed");

    // Wait for at least one event to confirm streaming started
    timeout(Duration::from_secs(5), stream.next())
        .await
        .expect("timed out waiting for stream start");

    cancel_token.cancel();

    timeout(Duration::from_secs(5), async {
        while stream.next().await.is_some() {}
    })
    .await
    .expect("timed out waiting for stream shutdown");

    remove_test_env("MOCK_AGENT_STREAM_UNTIL_CANCEL");
}

#[tokio::test(flavor = "current_thread")]
async fn test_spawn_failure_returns_error() {
    let _guard = acquire_test_guard();
    let bad_config = AcpAgentConfig {
        name: "bad",
        command: "definitely-missing-binary",
        args: vec![],
        install_url: "",
        install_command: None,
    };

    let temp_dir = tempdir().unwrap();
    let mut runner = AcpAgentRunner::new(bad_config, temp_dir.path().to_path_buf());
    let cancel_token = CancellationToken::new();

    match runner
        .spawn_stream("prompt".to_string(), cancel_token)
        .await
    {
        Ok(_) => panic!("expected spawning to fail"),
        Err(msg) => assert!(
            msg.contains("Failed to spawn agent"),
            "unexpected error message: {msg}"
        ),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn test_initialization_timeout() {
    let _guard = acquire_test_guard();
    build_mock_agent();
    set_test_env("MOCK_AGENT_HANG", "1");
    let temp_dir = tempdir().unwrap();
    let mut runner = AcpAgentRunner::new(mock_agent_config(), temp_dir.path().to_path_buf());
    let cancel_token = CancellationToken::new();

    match runner
        .spawn_stream("prompt".to_string(), cancel_token)
        .await
    {
        Ok(_) => panic!("expected initialization to time out"),
        Err(msg) => assert!(
            msg.contains("Initialization timeout"),
            "unexpected error message: {msg}"
        ),
    }
    remove_test_env("MOCK_AGENT_HANG");
}
