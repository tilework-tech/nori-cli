//! A queue held after an interrupted or failed turn, against a real mock ACP
//! agent process that logs every prompt it receives.
//!
//! Requires `cargo build -p mock-acp-agent` (or `MOCK_ACP_AGENT_BIN`).

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use nori_config::NoriConfig;
use nori_harness::runtime::AgentPrepareSpec;
use nori_harness::runtime::HarnessHandle;
use nori_harness::runtime::SessionStart;
use nori_harness::session_manager::ManagedSession;
use nori_harness::session_manager::SessionAgent;
use nori_harness::session_manager::SessionManager;
use nori_harness::session_manager::SessionManagerConfig;
use nori_harness::session_manager::SessionSource;
use nori_harness::session_manager::StartSession;
use nori_protocol::NoriEvent;
use nori_protocol::QueuePause;
use nori_protocol::SessionEnvelope;
use nori_protocol::SessionEvent;
use nori_protocol::SessionStatus;
use nori_protocol::acp;
use pretty_assertions::assert_eq;
use serial_test::serial;
use tokio::sync::mpsc;

#[path = "support/env.rs"]
mod env;

use env::EnvGuard;

const WAIT: Duration = Duration::from_secs(20);

#[expect(clippy::expect_used, reason = "test fixture boundary")]
async fn ready_session(manager: &SessionManager, home: &Path) -> ManagedSession {
    let config = NoriConfig {
        active_agent: "mock-model".to_string(),
        cwd: home.to_path_buf(),
        nori_home: home.to_path_buf(),
        ..Default::default()
    };
    let session = manager
        .start(StartSession {
            agent: SessionAgent::Prepare(AgentPrepareSpec {
                config: Arc::new(config),
                cli_version: "queue-pause-test".to_string(),
                session_context: None,
                initial_context: None,
            }),
            start: SessionStart::New,
            parent: None,
            source: SessionSource::User,
        })
        .expect("session starts");
    let ready = tokio::time::timeout(WAIT, async {
        while manager.get(&session.conversation_id).map(|r| r.status) != Some(SessionStatus::Ready)
        {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await;
    assert!(ready.is_ok(), "session never became ready");
    session
}

fn submit(handle: &HarnessHandle, text: &str) {
    let handle = handle.clone();
    let content = vec![acp::v1::ContentBlock::Text(acp::v1::TextContent::new(text))];
    tokio::spawn(async move { handle.prompt(content).await });
}

/// Wait for the next queue snapshot that matches `rows` and `paused`.
#[expect(
    clippy::expect_used,
    reason = "test helper fails at the stream boundary"
)]
async fn queue_reads(
    events: &mut mpsc::UnboundedReceiver<SessionEnvelope>,
    rows: &[&str],
    paused: Option<QueuePause>,
) {
    tokio::time::timeout(WAIT, async {
        loop {
            let envelope = events.recv().await.expect("session stream open");
            if let SessionEvent::Nori(NoriEvent::QueueChanged(queue)) = envelope.event
                && queue.prompts == rows
                && queue.paused == paused
            {
                return;
            }
        }
    })
    .await
    .expect("queue reaches the expected state")
}

fn delivered(log: &Path) -> Vec<String> {
    std::fs::read_to_string(log)
        .unwrap_or_default()
        .lines()
        .filter_map(|line| serde_json::from_str::<String>(line).ok())
        .collect()
}

async fn wait_delivered(log: &Path, count: usize) -> Vec<String> {
    let deadline = Instant::now() + WAIT;
    while delivered(log).len() < count && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    delivered(log)
}

#[tokio::test]
#[serial]
async fn an_interrupted_turn_holds_the_queue_until_resume_then_sends_each_once() {
    let home = tempfile::tempdir().expect("temp dir");
    let log = home.path().join("prompts.jsonl");
    let _log = EnvGuard::set("MOCK_AGENT_PROMPT_LOG", &log.display().to_string());
    let _delay = EnvGuard::set("MOCK_AGENT_DELAY_MS", "300");
    let manager = SessionManager::new(SessionManagerConfig::default());
    let mut session = ready_session(&manager, home.path()).await;

    submit(&session.handle, "mock:stream-until-cancel");
    wait_delivered(&log, 1).await;
    for text in ["alpha", "dup", "dup"] {
        submit(&session.handle, text);
    }
    queue_reads(&mut session.events, &["alpha", "dup", "dup"], None).await;

    session.handle.cancel().await.expect("cancel");
    queue_reads(
        &mut session.events,
        &["alpha", "dup", "dup"],
        Some(QueuePause::Interrupted),
    )
    .await;
    tokio::time::sleep(Duration::from_millis(1000)).await;
    assert_eq!(
        delivered(&log),
        ["mock:stream-until-cancel"],
        "nothing queued is sent on its own after an interrupt"
    );

    session.handle.resume_queue().await.expect("resume");
    session
        .handle
        .resume_queue()
        .await
        .expect("a second resume is harmless");
    let observed = wait_delivered(&log, 4).await;
    tokio::time::sleep(Duration::from_millis(1000)).await;
    assert_eq!(
        observed,
        ["mock:stream-until-cancel", "alpha", "dup", "dup"]
    );
    assert_eq!(
        delivered(&log),
        observed,
        "each prompt is sent exactly once"
    );

    manager.shutdown(Duration::ZERO).await;
}

#[tokio::test]
#[serial]
async fn a_failed_turn_holds_the_queue_until_resume() {
    let home = tempfile::tempdir().expect("temp dir");
    let log = home.path().join("prompts.jsonl");
    let _log = EnvGuard::set("MOCK_AGENT_PROMPT_LOG", &log.display().to_string());
    let _delay = EnvGuard::set("MOCK_AGENT_DELAY_MS", "800");
    let manager = SessionManager::new(SessionManagerConfig::default());
    let mut session = ready_session(&manager, home.path()).await;

    submit(&session.handle, "mock:prompt-fail-after-delay");
    wait_delivered(&log, 1).await;
    submit(&session.handle, "first after failure");
    submit(&session.handle, "second after failure");
    queue_reads(
        &mut session.events,
        &["first after failure", "second after failure"],
        Some(QueuePause::TurnFailed),
    )
    .await;
    tokio::time::sleep(Duration::from_millis(1000)).await;
    assert_eq!(delivered(&log), ["mock:prompt-fail-after-delay"]);

    session.handle.resume_queue().await.expect("resume");
    let observed = wait_delivered(&log, 3).await;
    tokio::time::sleep(Duration::from_millis(1200)).await;
    assert_eq!(
        observed,
        [
            "mock:prompt-fail-after-delay",
            "first after failure",
            "second after failure"
        ]
    );
    assert_eq!(delivered(&log), observed);

    manager.shutdown(Duration::ZERO).await;
}
