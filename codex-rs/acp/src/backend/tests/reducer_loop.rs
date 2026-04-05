//! Tests for the reducer loop wiring: verifying that `run_reducer_loop`
//! correctly bridges notifications through the reducer, forwards produced
//! `ClientEvent`s, and executes side effects.

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

/// Receive client events from the backend event channel, filtering out
/// control events, with a timeout.
async fn collect_client_events(
    rx: &mut mpsc::Receiver<BackendEvent>,
    timeout: Duration,
) -> Vec<ClientEvent> {
    let mut events = Vec::new();
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        match tokio::time::timeout_at(deadline, rx.recv()).await {
            Ok(Some(BackendEvent::Client(event))) => events.push(event),
            Ok(Some(BackendEvent::Control(_))) => continue,
            _ => break,
        }
    }
    events
}

fn simple_prompt() -> QueuedPrompt {
    QueuedPrompt {
        text: "hello".to_string(),
        images: Vec::new(),
    }
}

// =========================================================================
// 1. Notifications are bridged through the reducer and produce ClientEvents
// =========================================================================

/// When a SessionUpdate notification arrives, the reducer loop should
/// normalize it and forward the resulting ClientEvents to the TUI.
#[tokio::test]
async fn notification_bridge_forwards_client_events() {
    let (reducer_tx, reducer_rx) = mpsc::channel::<InboundEvent>(64);
    let (backend_event_tx, mut backend_event_rx) = mpsc::channel::<BackendEvent>(64);
    let session_runtime = Arc::new(Mutex::new(SessionRuntime::new()));
    let client_event_normalizer = Arc::new(Mutex::new(ClientEventNormalizer::default()));

    // Start the reducer loop in the background
    tokio::spawn(AcpBackend::run_reducer_loop(
        reducer_rx,
        Arc::clone(&client_event_normalizer),
        backend_event_tx,
        None, // no transcript
        Arc::clone(&session_runtime),
        None, // no connection
        reducer_tx.clone(),
    ));

    // First put the runtime into Prompt phase by sending PromptSubmit
    reducer_tx
        .send(InboundEvent::PromptSubmit(simple_prompt(), None))
        .await
        .unwrap();

    // Give the reducer loop time to process
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Now send a notification with agent message content
    let chunk = acp::SessionUpdate::AgentMessageChunk(acp::ContentChunk::new(
        acp::ContentBlock::Text(acp::TextContent::new("hello world")),
    ));
    reducer_tx
        .send(InboundEvent::Notification(Box::new(chunk)))
        .await
        .unwrap();

    // Collect events
    let events = collect_client_events(&mut backend_event_rx, Duration::from_millis(200)).await;

    // Should have TurnLifecycle::Started (from PromptSubmit) and a MessageDelta (from notification)
    assert!(
        events
            .iter()
            .any(|e| matches!(e, ClientEvent::TurnLifecycle(TurnLifecycle::Started))),
        "expected Started event from PromptSubmit, got: {events:?}"
    );
    assert!(
        events.iter().any(|e| matches!(
            e,
            ClientEvent::MessageDelta(delta)
            if delta.stream == nori_protocol::MessageStream::Answer
        )),
        "expected MessageDelta from agent notification, got: {events:?}"
    );
}

// =========================================================================
// 2. PromptResponse produces Completed with last_agent_message
// =========================================================================

/// When agent text chunks arrive followed by PromptResponse, the reducer
/// loop should emit TurnLifecycle::Completed with the assembled agent text.
#[tokio::test]
async fn prompt_response_produces_completed_with_last_agent_message() {
    let (reducer_tx, reducer_rx) = mpsc::channel::<InboundEvent>(64);
    let (backend_event_tx, mut backend_event_rx) = mpsc::channel::<BackendEvent>(64);
    let session_runtime = Arc::new(Mutex::new(SessionRuntime::new()));
    let client_event_normalizer = Arc::new(Mutex::new(ClientEventNormalizer::default()));

    tokio::spawn(AcpBackend::run_reducer_loop(
        reducer_rx,
        Arc::clone(&client_event_normalizer),
        backend_event_tx,
        None,
        Arc::clone(&session_runtime),
        None,
        reducer_tx.clone(),
    ));

    // Submit prompt
    reducer_tx
        .send(InboundEvent::PromptSubmit(simple_prompt(), None))
        .await
        .unwrap();

    // Stream agent text
    for text in ["hello ", "world"] {
        let chunk = acp::SessionUpdate::AgentMessageChunk(acp::ContentChunk::new(
            acp::ContentBlock::Text(acp::TextContent::new(text)),
        ));
        reducer_tx
            .send(InboundEvent::Notification(Box::new(chunk)))
            .await
            .unwrap();
    }

    // Send prompt response
    reducer_tx
        .send(InboundEvent::PromptResponse {
            stop_reason: acp::StopReason::EndTurn,
        })
        .await
        .unwrap();

    // Collect events
    let events = collect_client_events(&mut backend_event_rx, Duration::from_millis(200)).await;

    // Find Completed event and check last_agent_message
    let completed = events.iter().find_map(|e| match e {
        ClientEvent::TurnLifecycle(TurnLifecycle::Completed {
            last_agent_message, ..
        }) => Some(last_agent_message.clone()),
        _ => None,
    });

    assert!(
        completed.is_some(),
        "expected TurnLifecycle::Completed, got: {events:?}"
    );
    assert_eq!(
        completed.unwrap().as_deref(),
        Some("hello world"),
        "expected assembled agent text in last_agent_message"
    );
}

// =========================================================================
// 3. CancelSubmit produces Cancelling and SendCancel side effect
// =========================================================================

/// When CancelSubmit arrives during an active prompt, the reducer loop
/// should emit TurnLifecycle::Cancelling.
#[tokio::test]
async fn cancel_submit_produces_cancelling_event() {
    let (reducer_tx, reducer_rx) = mpsc::channel::<InboundEvent>(64);
    let (backend_event_tx, mut backend_event_rx) = mpsc::channel::<BackendEvent>(64);
    let session_runtime = Arc::new(Mutex::new(SessionRuntime::new()));
    let client_event_normalizer = Arc::new(Mutex::new(ClientEventNormalizer::default()));

    tokio::spawn(AcpBackend::run_reducer_loop(
        reducer_rx,
        Arc::clone(&client_event_normalizer),
        backend_event_tx,
        None,
        Arc::clone(&session_runtime),
        None,
        reducer_tx.clone(),
    ));

    // Submit prompt to enter Prompt phase
    reducer_tx
        .send(InboundEvent::PromptSubmit(simple_prompt(), None))
        .await
        .unwrap();

    // Give reducer time to process
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Send cancel
    reducer_tx.send(InboundEvent::CancelSubmit).await.unwrap();

    // Collect events
    let events = collect_client_events(&mut backend_event_rx, Duration::from_millis(200)).await;

    assert!(
        events
            .iter()
            .any(|e| matches!(e, ClientEvent::TurnLifecycle(TurnLifecycle::Cancelling))),
        "expected Cancelling event, got: {events:?}"
    );
}

// =========================================================================
// 4. Runtime state is updated by the reducer loop
// =========================================================================

/// The SessionRuntime behind the Arc<Mutex> should reflect phase changes
/// made by the reducer loop.
#[tokio::test]
async fn reducer_loop_updates_shared_runtime_state() {
    let (reducer_tx, reducer_rx) = mpsc::channel::<InboundEvent>(64);
    let (backend_event_tx, mut _backend_event_rx) = mpsc::channel::<BackendEvent>(64);
    let session_runtime = Arc::new(Mutex::new(SessionRuntime::new()));
    let client_event_normalizer = Arc::new(Mutex::new(ClientEventNormalizer::default()));

    tokio::spawn(AcpBackend::run_reducer_loop(
        reducer_rx,
        Arc::clone(&client_event_normalizer),
        backend_event_tx,
        None,
        Arc::clone(&session_runtime),
        None,
        reducer_tx.clone(),
    ));

    // Initially idle
    {
        let rt = session_runtime.lock().await;
        assert_eq!(
            rt.phase_view(),
            nori_protocol::session_runtime::SessionPhaseView::Idle
        );
    }

    // Submit prompt
    reducer_tx
        .send(InboundEvent::PromptSubmit(simple_prompt(), None))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Should be in Prompt phase
    {
        let rt = session_runtime.lock().await;
        assert_eq!(
            rt.phase_view(),
            nori_protocol::session_runtime::SessionPhaseView::Prompt,
            "expected Prompt phase after PromptSubmit"
        );
    }

    // Complete the turn
    reducer_tx
        .send(InboundEvent::PromptResponse {
            stop_reason: acp::StopReason::EndTurn,
        })
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Should be back to Idle
    {
        let rt = session_runtime.lock().await;
        assert_eq!(
            rt.phase_view(),
            nori_protocol::session_runtime::SessionPhaseView::Idle,
            "expected Idle phase after PromptResponse"
        );
    }
}
