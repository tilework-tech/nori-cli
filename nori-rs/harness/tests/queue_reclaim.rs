//! Taking a queued prompt back before it is delivered, against a real mock
//! ACP agent process that logs every prompt it receives.
//!
//! Requires `cargo build -p mock-acp-agent` (or `MOCK_ACP_AGENT_BIN`).

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use nori_config::NoriConfig;
use nori_harness::backend::PromptWithdrawn;
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
use nori_protocol::QueueEntry;
use nori_protocol::SessionEnvelope;
use nori_protocol::SessionEvent;
use nori_protocol::SessionStatus;
use nori_protocol::acp;
use pretty_assertions::assert_eq;
use serial_test::serial;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

#[path = "support/env.rs"]
mod env;

use env::EnvGuard;

const WAIT: Duration = Duration::from_secs(20);

#[expect(clippy::expect_used, reason = "test fixture boundary")]
fn start_session(manager: &SessionManager, home: &Path) -> ManagedSession {
    let config = NoriConfig {
        active_agent: "mock-model".to_string(),
        cwd: home.to_path_buf(),
        nori_home: home.to_path_buf(),
        ..Default::default()
    };
    manager
        .start(StartSession {
            agent: SessionAgent::Prepare(AgentPrepareSpec {
                config: Arc::new(config),
                cli_version: "queue-reclaim-test".to_string(),
                session_context: None,
                initial_context: None,
            }),
            start: SessionStart::New,
            parent: None,
            source: SessionSource::User,
        })
        .expect("session starts")
}

async fn wait_ready(manager: &SessionManager, session: &ManagedSession) {
    let ready = tokio::time::timeout(WAIT, async {
        while manager.get(&session.conversation_id).map(|r| r.status) != Some(SessionStatus::Ready)
        {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await;
    assert!(ready.is_ok(), "session never became ready");
}

fn submit(handle: &HarnessHandle, text: &str) -> JoinHandle<anyhow::Result<acp::v1::RequestId>> {
    let handle = handle.clone();
    let content = vec![acp::v1::ContentBlock::Text(acp::v1::TextContent::new(text))];
    tokio::spawn(async move { handle.prompt(content).await })
}

/// Wait for the next queue snapshot whose rows read `texts`.
#[expect(
    clippy::expect_used,
    reason = "test helper fails at the stream boundary"
)]
async fn queue_reads(
    events: &mut mpsc::UnboundedReceiver<SessionEnvelope>,
    texts: &[&str],
) -> Vec<QueueEntry> {
    tokio::time::timeout(WAIT, async {
        loop {
            let envelope = events.recv().await.expect("session stream open");
            if let SessionEvent::Nori(NoriEvent::QueueChanged(queue)) = envelope.event {
                let rows: Vec<&str> = queue.entries.iter().map(|e| e.text.as_str()).collect();
                if rows == texts {
                    assert_eq!(
                        queue.prompts, texts,
                        "older clients keep reading the plain text list"
                    );
                    return queue.entries;
                }
            }
        }
    })
    .await
    .expect("queue reaches the expected rows")
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
async fn reclaiming_one_of_two_identical_queued_prompts_delivers_the_rest_in_order() {
    let home = tempfile::tempdir().expect("temp dir");
    let log = home.path().join("prompts.jsonl");
    let _delay = EnvGuard::set("MOCK_AGENT_DELAY_MS", "800");
    let _log = EnvGuard::set("MOCK_AGENT_PROMPT_LOG", &log.display().to_string());
    let manager = SessionManager::new(SessionManagerConfig::default());
    let mut session = start_session(&manager, home.path());
    wait_ready(&manager, &session).await;

    let _first = submit(&session.handle, "turn one");
    wait_delivered(&log, 1).await;
    let alpha = submit(&session.handle, "alpha");
    queue_reads(&mut session.events, &["alpha"]).await;
    let dup_a = submit(&session.handle, "dup");
    queue_reads(&mut session.events, &["alpha", "dup"]).await;
    let dup_b = submit(&session.handle, "dup");
    let entries = queue_reads(&mut session.events, &["alpha", "dup", "dup"]).await;
    assert!(entries.iter().all(|entry| entry.editable));

    let reclaimed = session
        .handle
        .reclaim_queued_prompt(entries[2].id.clone())
        .await
        .expect("reclaim request");
    assert_eq!(reclaimed.as_deref(), Some("dup"));
    let remaining = queue_reads(&mut session.events, &["alpha", "dup"]).await;
    assert_eq!(
        remaining,
        entries[..2].to_vec(),
        "the other rows keep their ids"
    );

    let withdrawn = dup_b
        .await
        .expect("join")
        .expect_err("withdrawn submission");
    assert!(
        withdrawn.downcast_ref::<PromptWithdrawn>().is_some(),
        "{withdrawn:?}"
    );

    let again = session
        .handle
        .reclaim_queued_prompt(entries[2].id.clone())
        .await
        .expect("second reclaim request");
    assert_eq!(again, None, "a prompt is reclaimed at most once");

    let _edited = submit(&session.handle, "dup edited");
    alpha.await.expect("join").expect("alpha starts");
    dup_a.await.expect("join").expect("first dup starts");
    let observed = wait_delivered(&log, 4).await;
    tokio::time::sleep(Duration::from_millis(1200)).await;
    assert_eq!(observed, ["turn one", "alpha", "dup", "dup edited"]);
    assert_eq!(delivered(&log), observed, "nothing else is delivered");

    manager.shutdown(Duration::ZERO).await;
}

#[tokio::test]
#[serial]
async fn a_prompt_that_already_started_cannot_be_reclaimed() {
    let home = tempfile::tempdir().expect("temp dir");
    let log = home.path().join("prompts.jsonl");
    let _delay = EnvGuard::set("MOCK_AGENT_DELAY_MS", "600");
    let _log = EnvGuard::set("MOCK_AGENT_PROMPT_LOG", &log.display().to_string());
    let manager = SessionManager::new(SessionManagerConfig::default());
    let mut session = start_session(&manager, home.path());
    wait_ready(&manager, &session).await;

    let _first = submit(&session.handle, "turn one");
    wait_delivered(&log, 1).await;
    let next = submit(&session.handle, "next");
    let entries = queue_reads(&mut session.events, &["next"]).await;
    queue_reads(&mut session.events, &[]).await;
    next.await.expect("join").expect("next starts");

    let reclaimed = session
        .handle
        .reclaim_queued_prompt(entries[0].id.clone())
        .await
        .expect("reclaim request");
    assert_eq!(reclaimed, None);
    assert_eq!(wait_delivered(&log, 2).await, ["turn one", "next"]);

    manager.shutdown(Duration::ZERO).await;
}
