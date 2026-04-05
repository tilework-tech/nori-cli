//! Tests for hook execution in the reducer loop.
//!
//! Verifies that `run_reducer_loop` fires lifecycle hooks
//! (pre_agent_response, pre_tool_call, post_tool_call) when processing
//! the corresponding ACP session update notifications.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use nori_protocol::ClientEvent;
use nori_protocol::ClientEventNormalizer;
use nori_protocol::TurnLifecycle;
use nori_protocol::session_runtime::QueuedPrompt;
use nori_protocol::session_runtime::SessionRuntime;
use sacp::schema as acp;
use tokio::sync::Mutex;
use tokio::sync::mpsc;

use super::*;
use crate::backend::session_reducer::InboundEvent;

fn simple_prompt() -> QueuedPrompt {
    QueuedPrompt {
        text: "hello".to_string(),
        images: Vec::new(),
    }
}

/// Helper: create a shell script that writes a marker file when executed.
fn create_marker_hook(dir: &std::path::Path, name: &str, marker_name: &str) -> PathBuf {
    let script = dir.join(name);
    let marker = dir.join(marker_name);
    std::fs::write(
        &script,
        format!(
            "#!/bin/bash\necho \"$NORI_HOOK_EVENT\" > '{}'",
            marker.display()
        ),
    )
    .unwrap();
    script
}

/// Helper: create a hook that captures tool-related env vars.
fn create_tool_info_hook(dir: &std::path::Path, name: &str, marker_name: &str) -> PathBuf {
    let script = dir.join(name);
    let marker = dir.join(marker_name);
    std::fs::write(
        &script,
        format!(
            "#!/bin/bash\necho \"event=$NORI_HOOK_EVENT tool=$NORI_HOOK_TOOL_NAME\" > '{}'",
            marker.display()
        ),
    )
    .unwrap();
    script
}

/// Spawn a reducer loop with the given hook config and return the
/// reducer_tx plus a mutable backend_event_rx for draining lifecycle events.
fn spawn_reducer_loop_with_hooks(
    hooks: ReducerHookConfig,
) -> (mpsc::Sender<InboundEvent>, mpsc::Receiver<BackendEvent>) {
    let (event_tx, _event_rx) = mpsc::channel(64);
    let (backend_event_tx, backend_event_rx) = mpsc::channel::<BackendEvent>(64);
    let (reducer_tx, reducer_rx) = mpsc::channel::<InboundEvent>(256);
    let session_runtime = Arc::new(Mutex::new(SessionRuntime::new()));
    let client_event_normalizer = Arc::new(Mutex::new(ClientEventNormalizer::default()));

    tokio::spawn(AcpBackend::run_reducer_loop(
        reducer_rx,
        client_event_normalizer,
        backend_event_tx,
        None,
        session_runtime,
        None,
        reducer_tx.clone(),
        event_tx,
        hooks,
    ));

    (reducer_tx, backend_event_rx)
}

/// Establish prompt phase by sending PromptSubmit and draining Started.
async fn enter_prompt_phase(
    reducer_tx: &mpsc::Sender<InboundEvent>,
    backend_rx: &mut mpsc::Receiver<BackendEvent>,
) {
    reducer_tx
        .send(InboundEvent::PromptSubmit(simple_prompt(), None))
        .await
        .expect("send PromptSubmit");

    // Drain until we see TurnLifecycle::Started
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    loop {
        match tokio::time::timeout_at(deadline, backend_rx.recv()).await {
            Ok(Some(BackendEvent::Client(ClientEvent::TurnLifecycle(TurnLifecycle::Started)))) => {
                break;
            }
            Ok(Some(_)) => continue,
            _ => panic!("timed out waiting for TurnLifecycle::Started"),
        }
    }
}

// =========================================================================
// 1. pre_agent_response hook fires on first AgentMessageChunk
// =========================================================================

#[tokio::test]
async fn pre_agent_response_hook_fires_on_first_agent_chunk() {
    let tmp = tempfile::tempdir().unwrap();
    let hook = create_marker_hook(tmp.path(), "pre_agent.sh", "pre_agent.marker");
    let marker = tmp.path().join("pre_agent.marker");

    let (reducer_tx, mut backend_rx) = spawn_reducer_loop_with_hooks(ReducerHookConfig {
        pre_agent_response_hooks: vec![hook],
        script_timeout: Duration::from_secs(5),
        ..Default::default()
    });

    enter_prompt_phase(&reducer_tx, &mut backend_rx).await;

    // Send an agent message chunk with non-empty text
    let chunk = acp::SessionUpdate::AgentMessageChunk(acp::ContentChunk::new(
        acp::ContentBlock::Text(acp::TextContent::new("hello world")),
    ));
    reducer_tx
        .send(InboundEvent::Notification(Box::new(chunk)))
        .await
        .unwrap();

    // Give hooks time to execute
    tokio::time::sleep(Duration::from_millis(500)).await;

    assert!(
        marker.exists(),
        "pre_agent_response hook should have created marker file"
    );
    let contents = std::fs::read_to_string(&marker).unwrap();
    assert_eq!(contents.trim(), "pre_agent_response");
}

// =========================================================================
// 2. pre_agent_response hook does NOT fire on second chunk
// =========================================================================

#[tokio::test]
async fn pre_agent_response_hook_fires_only_once_per_prompt() {
    let tmp = tempfile::tempdir().unwrap();
    let script = tmp.path().join("pre_agent_count.sh");
    let marker = tmp.path().join("pre_agent_count.marker");
    std::fs::write(
        &script,
        format!("#!/bin/bash\necho 'fired' >> '{}'", marker.display()),
    )
    .unwrap();

    let (reducer_tx, mut backend_rx) = spawn_reducer_loop_with_hooks(ReducerHookConfig {
        pre_agent_response_hooks: vec![script],
        script_timeout: Duration::from_secs(5),
        ..Default::default()
    });

    enter_prompt_phase(&reducer_tx, &mut backend_rx).await;

    // Send two agent message chunks
    for text in ["first chunk", "second chunk"] {
        let chunk = acp::SessionUpdate::AgentMessageChunk(acp::ContentChunk::new(
            acp::ContentBlock::Text(acp::TextContent::new(text)),
        ));
        reducer_tx
            .send(InboundEvent::Notification(Box::new(chunk)))
            .await
            .unwrap();
    }

    tokio::time::sleep(Duration::from_millis(500)).await;

    assert!(marker.exists(), "hook should have fired at least once");
    let contents = std::fs::read_to_string(&marker).unwrap();
    let fire_count = contents.lines().filter(|l| !l.is_empty()).count();
    assert_eq!(
        fire_count, 1,
        "pre_agent_response hook should fire exactly once per prompt, got {fire_count}"
    );
}

// =========================================================================
// 3. pre_tool_call hook fires on ToolCall notification
// =========================================================================

#[tokio::test]
async fn pre_tool_call_hook_fires_on_tool_call_notification() {
    let tmp = tempfile::tempdir().unwrap();
    let hook = create_tool_info_hook(tmp.path(), "pre_tool.sh", "pre_tool.marker");
    let marker = tmp.path().join("pre_tool.marker");

    let (reducer_tx, mut backend_rx) = spawn_reducer_loop_with_hooks(ReducerHookConfig {
        pre_tool_call_hooks: vec![hook],
        script_timeout: Duration::from_secs(5),
        ..Default::default()
    });

    enter_prompt_phase(&reducer_tx, &mut backend_rx).await;

    // Send a ToolCall notification
    let tool_call = acp::SessionUpdate::ToolCall(
        acp::ToolCall::new("call-1", "Terminal")
            .kind(acp::ToolKind::Execute)
            .status(acp::ToolCallStatus::Pending),
    );
    reducer_tx
        .send(InboundEvent::Notification(Box::new(tool_call)))
        .await
        .unwrap();

    tokio::time::sleep(Duration::from_millis(500)).await;

    assert!(
        marker.exists(),
        "pre_tool_call hook should have created marker file"
    );
    let contents = std::fs::read_to_string(&marker).unwrap();
    assert!(
        contents.contains("event=pre_tool_call"),
        "hook should receive NORI_HOOK_EVENT=pre_tool_call, got: {contents}"
    );
    assert!(
        contents.contains("tool=Terminal"),
        "hook should receive NORI_HOOK_TOOL_NAME=Terminal, got: {contents}"
    );
}

// =========================================================================
// 4. post_tool_call hook fires on ToolCallUpdate with Completed status
// =========================================================================

#[tokio::test]
async fn post_tool_call_hook_fires_on_completed_tool_call_update() {
    let tmp = tempfile::tempdir().unwrap();
    let hook = create_tool_info_hook(tmp.path(), "post_tool.sh", "post_tool.marker");
    let marker = tmp.path().join("post_tool.marker");

    let (reducer_tx, mut backend_rx) = spawn_reducer_loop_with_hooks(ReducerHookConfig {
        post_tool_call_hooks: vec![hook],
        script_timeout: Duration::from_secs(5),
        ..Default::default()
    });

    enter_prompt_phase(&reducer_tx, &mut backend_rx).await;

    // Send a ToolCallUpdate with Completed status
    let update = acp::SessionUpdate::ToolCallUpdate(acp::ToolCallUpdate::new(
        "call-1",
        acp::ToolCallUpdateFields::new()
            .title("Terminal")
            .kind(acp::ToolKind::Execute)
            .status(acp::ToolCallStatus::Completed)
            .raw_output(serde_json::json!({"stdout": "hello\n"})),
    ));
    reducer_tx
        .send(InboundEvent::Notification(Box::new(update)))
        .await
        .unwrap();

    tokio::time::sleep(Duration::from_millis(500)).await;

    assert!(
        marker.exists(),
        "post_tool_call hook should have created marker file"
    );
    let contents = std::fs::read_to_string(&marker).unwrap();
    assert!(
        contents.contains("event=post_tool_call"),
        "hook should receive NORI_HOOK_EVENT=post_tool_call, got: {contents}"
    );
    assert!(
        contents.contains("tool=Terminal"),
        "hook should receive NORI_HOOK_TOOL_NAME=Terminal, got: {contents}"
    );
}

// =========================================================================
// 5. post_tool_call hook does NOT fire on non-Completed ToolCallUpdate
// =========================================================================

#[tokio::test]
async fn post_tool_call_hook_does_not_fire_on_in_progress_update() {
    let tmp = tempfile::tempdir().unwrap();
    let hook = create_marker_hook(tmp.path(), "post_tool_noop.sh", "post_tool_noop.marker");
    let marker = tmp.path().join("post_tool_noop.marker");

    let (reducer_tx, mut backend_rx) = spawn_reducer_loop_with_hooks(ReducerHookConfig {
        post_tool_call_hooks: vec![hook],
        script_timeout: Duration::from_secs(5),
        ..Default::default()
    });

    enter_prompt_phase(&reducer_tx, &mut backend_rx).await;

    // Send a ToolCallUpdate with InProgress status (NOT Completed)
    let update = acp::SessionUpdate::ToolCallUpdate(acp::ToolCallUpdate::new(
        "call-1",
        acp::ToolCallUpdateFields::new()
            .title("Terminal")
            .status(acp::ToolCallStatus::InProgress),
    ));
    reducer_tx
        .send(InboundEvent::Notification(Box::new(update)))
        .await
        .unwrap();

    tokio::time::sleep(Duration::from_millis(300)).await;

    assert!(
        !marker.exists(),
        "post_tool_call hook should NOT fire on InProgress updates"
    );
}

// =========================================================================
// 6. pre_agent_response resets on new PromptSubmit
// =========================================================================

#[tokio::test]
async fn pre_agent_response_resets_on_new_prompt() {
    let tmp = tempfile::tempdir().unwrap();
    let script = tmp.path().join("pre_agent_reset.sh");
    let marker = tmp.path().join("pre_agent_reset.marker");
    std::fs::write(
        &script,
        format!("#!/bin/bash\necho 'fired' >> '{}'", marker.display()),
    )
    .unwrap();

    let (reducer_tx, mut backend_rx) = spawn_reducer_loop_with_hooks(ReducerHookConfig {
        pre_agent_response_hooks: vec![script],
        script_timeout: Duration::from_secs(5),
        ..Default::default()
    });

    // First prompt
    enter_prompt_phase(&reducer_tx, &mut backend_rx).await;

    let chunk = acp::SessionUpdate::AgentMessageChunk(acp::ContentChunk::new(
        acp::ContentBlock::Text(acp::TextContent::new("response 1")),
    ));
    reducer_tx
        .send(InboundEvent::Notification(Box::new(chunk)))
        .await
        .unwrap();

    // Complete the turn
    reducer_tx
        .send(InboundEvent::PromptResponse {
            stop_reason: acp::StopReason::EndTurn,
        })
        .await
        .unwrap();

    tokio::time::sleep(Duration::from_millis(300)).await;

    // Second prompt
    enter_prompt_phase(&reducer_tx, &mut backend_rx).await;

    let chunk = acp::SessionUpdate::AgentMessageChunk(acp::ContentChunk::new(
        acp::ContentBlock::Text(acp::TextContent::new("response 2")),
    ));
    reducer_tx
        .send(InboundEvent::Notification(Box::new(chunk)))
        .await
        .unwrap();

    tokio::time::sleep(Duration::from_millis(500)).await;

    // The hook should have fired twice (once per prompt)
    assert!(marker.exists(), "hook should have fired");
    let contents = std::fs::read_to_string(&marker).unwrap();
    let fire_count = contents.lines().filter(|l| !l.is_empty()).count();
    assert_eq!(
        fire_count, 2,
        "pre_agent_response should fire once per prompt (2 prompts), got {fire_count}"
    );
}

// =========================================================================
// 7. post_agent_response hook fires after TurnLifecycle::Completed
// =========================================================================

#[tokio::test]
async fn post_agent_response_hook_fires_on_completed_turn() {
    let tmp = tempfile::tempdir().unwrap();
    let script = tmp.path().join("post_agent.sh");
    let marker = tmp.path().join("post_agent.marker");
    std::fs::write(
        &script,
        format!(
            "#!/bin/bash\necho \"event=$NORI_HOOK_EVENT response=$NORI_HOOK_AGENT_RESPONSE\" > '{}'",
            marker.display()
        ),
    )
    .unwrap();

    let (reducer_tx, mut backend_rx) = spawn_reducer_loop_with_hooks(ReducerHookConfig {
        post_agent_response_hooks: vec![script],
        script_timeout: Duration::from_secs(5),
        ..Default::default()
    });

    enter_prompt_phase(&reducer_tx, &mut backend_rx).await;

    // Stream agent text
    let chunk = acp::SessionUpdate::AgentMessageChunk(acp::ContentChunk::new(
        acp::ContentBlock::Text(acp::TextContent::new("hello from agent")),
    ));
    reducer_tx
        .send(InboundEvent::Notification(Box::new(chunk)))
        .await
        .unwrap();

    // Complete the turn
    reducer_tx
        .send(InboundEvent::PromptResponse {
            stop_reason: acp::StopReason::EndTurn,
        })
        .await
        .unwrap();

    tokio::time::sleep(Duration::from_millis(500)).await;

    assert!(
        marker.exists(),
        "post_agent_response hook should have created marker file"
    );
    let contents = std::fs::read_to_string(&marker).unwrap();
    assert!(
        contents.contains("event=post_agent_response"),
        "hook should receive NORI_HOOK_EVENT=post_agent_response, got: {contents}"
    );
    assert!(
        contents.contains("response=hello from agent"),
        "hook should receive NORI_HOOK_AGENT_RESPONSE with agent text, got: {contents}"
    );
}
