use std::collections::HashMap;
use std::process::Command;
use std::sync::Mutex;
use std::time::Duration;

use futures::StreamExt;
use nori_cli::acp_runner::{AcpAgentConfig, AcpAgentRunner};
use nori_cli::backends::BackendEvent;
use nori_cli::conversation::ConversationEvent;
use nori_cli::history::{InlineEntryId, InlineEntryKind, InlineEntryUpdate};
use tempfile::tempdir;
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;

/// Helper to track inline entries and convert them to ConversationEvents
#[derive(Default)]
struct InlineTracker {
    entries: HashMap<InlineEntryId, String>,
}

impl InlineTracker {
    fn handle_event(&mut self, event: &BackendEvent) -> Option<ConversationEvent> {
        match event {
            BackendEvent::InlineBegin { id, kind } => {
                match kind {
                    InlineEntryKind::AssistantMessage => {
                        self.entries.insert(id.clone(), String::new());
                    }
                }
                None
            }
            BackendEvent::InlineUpdate { id, update } => {
                if let Some(buffer) = self.entries.get_mut(id) {
                    let InlineEntryUpdate::AppendText(text) = update;
                    buffer.push_str(text);
                }
                None
            }
            BackendEvent::InlineCommit { id } => self
                .entries
                .remove(id)
                .map(|text| ConversationEvent::AssistantMessage { text }),
            BackendEvent::InlineAbort { id } => {
                self.entries.remove(id);
                None
            }
            _ => None,
        }
    }
}

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

    // Find both test messages, handling inline events
    let mut found_messages = Vec::new();
    let mut tracker = InlineTracker::default();

    for _ in 0..30 {
        // Allow up to 30 events to find both messages
        let event = timeout(Duration::from_secs(5), stream.next())
            .await
            .expect("timed out waiting for event")
            .expect("stream closed before event");

        // Handle inline events and convert to ConversationEvent
        if let Some(conv_event) = tracker.handle_event(&event) {
            if let ConversationEvent::AssistantMessage { ref text } = conv_event {
                // The inline tracker combines all chunks, so we'll get one message with both
                if text.contains("Test message 1") && text.contains("Test message 2") {
                    found_messages.push(text.clone());
                    break;
                }
            }
        }

        // Also check for direct ConversationEvent (for backward compatibility)
        if let BackendEvent::Conversation(ConversationEvent::AssistantMessage { ref text }) = event
        {
            if text == "Test message 1" || text == "Test message 2" {
                found_messages.push(text.clone());
                if found_messages.len() == 2 {
                    break;
                }
            }
        }
    }

    assert!(!found_messages.is_empty(), "Did not find test messages");
    assert!(
        found_messages[0].contains("Test message 1"),
        "Missing 'Test message 1'"
    );
    assert!(
        found_messages[0].contains("Test message 2"),
        "Missing 'Test message 2'"
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

    // Skip debug events and find the messages using inline tracker
    let mut tracker = InlineTracker::default();
    let mut found_initial_messages = false;
    let mut found_file_content = false;

    for _ in 0..30 {
        // Allow more events due to debug logging and inline events
        let event = timeout(Duration::from_secs(5), stream.next())
            .await
            .unwrap_or_else(|_| panic!("timed out waiting for event"))
            .unwrap_or_else(|| panic!("stream closed"));

        // Handle inline events
        if let Some(conv_event) = tracker.handle_event(&event) {
            if let ConversationEvent::AssistantMessage { ref text } = conv_event {
                if text.contains("Test message 1") && text.contains("Test message 2") {
                    found_initial_messages = true;
                }
                if text.contains("Read file content: Hello from file") {
                    found_file_content = true;
                    break;
                }
            }
        }

        // Also check for direct ConversationEvent (for backward compatibility)
        match event {
            BackendEvent::Conversation(ConversationEvent::AssistantMessage { ref text }) => {
                if (text == "Test message 1" || text == "Test message 2") && !found_initial_messages
                {
                    // Skip for now, we're looking for the combined message
                } else if text.contains("Read file content: Hello from file") {
                    found_file_content = true;
                    break;
                }
            }
            _ => continue,
        }
    }

    assert!(found_initial_messages, "Did not find initial test messages");
    assert!(found_file_content, "Did not find file content message");

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
