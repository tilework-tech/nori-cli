//! Session manager behaviour against real mock ACP agent processes.
//!
//! Requires `cargo build -p mock-acp-agent` (or `MOCK_ACP_AGENT_BIN`).

use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use nori_config::NoriConfig;
use nori_harness::ConversationId;
use nori_harness::runtime::AgentPrepareSpec;
use nori_harness::runtime::SessionStart;
use nori_harness::session_manager::ManagedSession;
use nori_harness::session_manager::ResumeError;
use nori_harness::session_manager::SessionAgent;
use nori_harness::session_manager::SessionManager;
use nori_harness::session_manager::SessionManagerConfig;
use nori_harness::session_manager::SessionSource;
use nori_harness::session_manager::StartError;
use nori_harness::session_manager::StartSession;
use nori_protocol::AcpEvent;
use nori_protocol::NoriEvent;
use nori_protocol::SessionEnvelope;
use nori_protocol::SessionEvent;
use nori_protocol::SessionStatus;
use nori_protocol::acp;
use pretty_assertions::assert_eq;
use serial_test::serial;
use tokio::sync::mpsc;

const WAIT: Duration = Duration::from_secs(15);

struct EnvGuard(&'static str, Option<String>);

impl EnvGuard {
    fn set(name: &'static str, value: &str) -> Self {
        let previous = std::env::var(name).ok();
        // SAFETY: tests that mutate the mock-agent environment run serially.
        unsafe { std::env::set_var(name, value) };
        Self(name, previous)
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        // SAFETY: tests that mutate the mock-agent environment run serially.
        unsafe {
            match &self.1 {
                Some(previous) => std::env::set_var(self.0, previous),
                None => std::env::remove_var(self.0),
            }
        }
    }
}

fn prepare_spec(agent: &str, home: &Path, wire_log_dir: Option<PathBuf>) -> AgentPrepareSpec {
    let mut config = NoriConfig {
        active_agent: agent.to_string(),
        cwd: home.to_path_buf(),
        nori_home: home.to_path_buf(),
        ..Default::default()
    };
    if let Some(log_dir) = wire_log_dir {
        config.acp_proxy = nori_config::AcpProxyConfig {
            enabled: true,
            log_dir,
        };
    }
    AgentPrepareSpec {
        config: Arc::new(config),
        cli_version: "session-manager-test".to_string(),
        session_context: None,
        initial_context: None,
    }
}

fn start(manager: &SessionManager, spec: AgentPrepareSpec) -> ManagedSession {
    manager
        .start(StartSession {
            agent: SessionAgent::Prepare(spec),
            start: SessionStart::New,
            parent: None,
            source: SessionSource::User,
        })
        .expect("session starts")
}

async fn wait_for_status(manager: &SessionManager, id: &ConversationId, wanted: SessionStatus) {
    let waited = tokio::time::timeout(WAIT, async {
        loop {
            if manager.get(id).map(|record| record.status) == Some(wanted) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await;
    assert!(
        waited.is_ok(),
        "session {id} never reached {wanted:?}; last record: {:?}",
        manager.get(id)
    );
}

fn prompt_text(text: &str) -> Vec<acp::v1::ContentBlock> {
    vec![acp::v1::ContentBlock::Text(acp::v1::TextContent::new(text))]
}

fn agent_text(event: &SessionEvent) -> Option<String> {
    let SessionEvent::Acp(AcpEvent::Notification(acp::v1::AgentNotification::SessionNotification(
        notification,
    ))) = event
    else {
        return None;
    };
    let acp::v1::SessionUpdate::AgentMessageChunk(chunk) = &notification.update else {
        return None;
    };
    let acp::v1::ContentBlock::Text(text) = &chunk.content else {
        return None;
    };
    Some(text.text.clone())
}

fn prompt_completed(event: &SessionEvent, request_id: &acp::v1::RequestId) -> bool {
    matches!(
        event,
        SessionEvent::Acp(AcpEvent::Response {
            request_id: response_id,
            response: Ok(acp::v1::AgentResponse::PromptResponse(_)),
        }) if response_id == request_id
    )
}

/// Collect a session's own envelopes until its prompt `request_id` completes.
async fn collect_turn(
    events: &mut mpsc::UnboundedReceiver<SessionEnvelope>,
    request_id: &acp::v1::RequestId,
) -> Vec<SessionEnvelope> {
    tokio::time::timeout(WAIT, async {
        let mut collected = Vec::new();
        loop {
            let envelope = events.recv().await.expect("session stream open");
            let done = prompt_completed(&envelope.event, request_id);
            collected.push(envelope);
            if done {
                return collected;
            }
        }
    })
    .await
    .expect("turn completes")
}

#[cfg(unix)]
#[expect(
    clippy::expect_used,
    reason = "wire-log filenames are the process-boundary test fixture"
)]
fn recorded_agent_pid(wire_log_dir: &Path) -> i32 {
    let wire_log = std::fs::read_dir(wire_log_dir)
        .expect("wire log directory")
        .next()
        .expect("wire log")
        .expect("wire log entry")
        .path();
    wire_log
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| name.split('-').nth(1))
        .and_then(|pid| pid.parse::<i32>().ok())
        .expect("wire log filename contains child pid")
}

#[tokio::test]
#[serial]
async fn two_sessions_on_two_agent_processes_interleave_turns() {
    let _alpha = EnvGuard::set("MOCK_AGENT_RESPONSE_MOCK_MODEL", "alpha reply");
    let _beta = EnvGuard::set("MOCK_AGENT_RESPONSE_MOCK_MODEL_ALT", "beta reply");
    let _delay = EnvGuard::set("MOCK_AGENT_DELAY_MS", "1500");
    let home_a = tempfile::tempdir().expect("temp dir");
    let home_b = tempfile::tempdir().expect("temp dir");
    let manager = SessionManager::new(SessionManagerConfig::default());
    let mut all_sessions = manager.subscribe(4096);

    let mut a = start(&manager, prepare_spec("mock-model", home_a.path(), None));
    let mut b = start(
        &manager,
        prepare_spec("mock-model-alt", home_b.path(), None),
    );
    wait_for_status(&manager, &a.conversation_id, SessionStatus::Ready).await;
    wait_for_status(&manager, &b.conversation_id, SessionStatus::Ready).await;

    let request_a = a
        .handle
        .prompt(prompt_text("first"))
        .await
        .expect("prompt a");
    let request_b = b
        .handle
        .prompt(prompt_text("second"))
        .await
        .expect("prompt b");
    let both_working = tokio::time::timeout(WAIT, async {
        loop {
            let statuses = [&a.conversation_id, &b.conversation_id]
                .map(|id| manager.get(id).map(|record| record.status));
            if statuses == [Some(SessionStatus::Working); 2] {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await;
    assert!(both_working.is_ok(), "both turns run at the same time");
    let turn_a = collect_turn(&mut a.events, &request_a).await;
    let turn_b = collect_turn(&mut b.events, &request_b).await;

    let texts = |turn: &[SessionEnvelope]| -> Vec<String> {
        turn.iter()
            .filter_map(|envelope| agent_text(&envelope.event))
            .collect()
    };
    let owners = |turn: &[SessionEnvelope]| -> Vec<(String, String)> {
        let mut owners: Vec<(String, String)> = turn
            .iter()
            .map(|envelope| (envelope.conversation_id.clone(), envelope.agent.clone()))
            .collect();
        owners.dedup();
        owners
    };
    assert_eq!(
        (
            texts(&turn_a),
            owners(&turn_a),
            texts(&turn_b),
            owners(&turn_b)
        ),
        (
            vec!["alpha reply".to_string()],
            vec![(a.conversation_id.to_string(), "mock-model".to_string())],
            vec!["beta reply".to_string()],
            vec![(b.conversation_id.to_string(), "mock-model-alt".to_string())],
        )
    );

    // The shared stream carries both sessions, each numbered 1, 2, 3, ...
    // up to the same envelope that ended the turn on the session's own stream.
    let last_seq = |turn: &[SessionEnvelope]| turn.last().map(|envelope| envelope.seq);
    let wanted = HashMap::from([
        (a.conversation_id.to_string(), last_seq(&turn_a)),
        (b.conversation_id.to_string(), last_seq(&turn_b)),
    ]);
    let mut sequences: HashMap<String, Vec<i64>> = HashMap::new();
    tokio::time::timeout(WAIT, async {
        while wanted
            .iter()
            .any(|(id, last)| sequences.get(id).and_then(|seqs| seqs.last().copied()) < *last)
        {
            let envelope = all_sessions.recv().await.expect("shared stream open");
            sequences
                .entry(envelope.conversation_id)
                .or_default()
                .push(envelope.seq);
        }
    })
    .await
    .expect("shared stream catches up with both sessions");
    for (id, seqs) in &sequences {
        let expected: Vec<i64> = (1..=seqs.len() as i64).collect();
        assert_eq!(seqs, &expected, "session {id} is numbered without gaps");
    }
    assert_eq!(sequences.len(), 2, "only the two sessions appear");

    manager.shutdown().await;
}

#[cfg(unix)]
#[tokio::test]
#[serial]
async fn an_agent_killed_mid_turn_fails_only_its_session_which_can_be_resumed() {
    let _alpha = EnvGuard::set("MOCK_AGENT_RESPONSE_MOCK_MODEL", "doomed reply");
    let _beta = EnvGuard::set("MOCK_AGENT_RESPONSE_MOCK_MODEL_ALT", "survivor reply");
    let _delay = EnvGuard::set("MOCK_AGENT_DELAY_MS", "1500");
    let home_a = tempfile::tempdir().expect("temp dir");
    let home_b = tempfile::tempdir().expect("temp dir");
    let wire_a = home_a.path().join("wire");
    let manager = SessionManager::new(SessionManagerConfig::default());

    let mut a = start(
        &manager,
        prepare_spec("mock-model", home_a.path(), Some(wire_a.clone())),
    );
    let mut b = start(
        &manager,
        prepare_spec("mock-model-alt", home_b.path(), None),
    );
    wait_for_status(&manager, &a.conversation_id, SessionStatus::Ready).await;
    wait_for_status(&manager, &b.conversation_id, SessionStatus::Ready).await;

    let _request_a = a
        .handle
        .prompt(prompt_text("first"))
        .await
        .expect("prompt a");
    let request_b = b
        .handle
        .prompt(prompt_text("second"))
        .await
        .expect("prompt b");
    tokio::time::timeout(WAIT, async {
        loop {
            let envelope = a.events.recv().await.expect("a stream open");
            if agent_text(&envelope.event).is_some() {
                return;
            }
        }
    })
    .await
    .expect("a streams before it is killed");
    // SAFETY: sends SIGKILL to the mock agent process this test started.
    let killed = unsafe { libc::kill(recorded_agent_pid(&wire_a), libc::SIGKILL) };
    assert_eq!(killed, 0, "kill the agent process of session a");

    let turn_b = collect_turn(&mut b.events, &request_b).await;
    wait_for_status(&manager, &a.conversation_id, SessionStatus::Failed).await;
    wait_for_status(&manager, &b.conversation_id, SessionStatus::Ready).await;
    assert_eq!(
        turn_b
            .iter()
            .filter_map(|envelope| agent_text(&envelope.event))
            .collect::<Vec<_>>(),
        vec!["survivor reply".to_string()]
    );
    assert!(manager.handle(&a.conversation_id).is_none());

    let mut resumed = manager
        .resume(
            &a.conversation_id,
            prepare_spec("mock-model", home_a.path(), None),
        )
        .await
        .expect("failed session resumes from its transcript");
    let restored = tokio::time::timeout(WAIT, async {
        let mut restored = Vec::new();
        loop {
            let envelope = resumed.events.recv().await.expect("resumed stream open");
            if matches!(
                envelope.event,
                SessionEvent::Nori(NoriEvent::ReplayFinished)
            ) {
                return restored;
            }
            restored.extend(agent_text(&envelope.event));
        }
    })
    .await
    .expect("resumed session replays its transcript");
    wait_for_status(&manager, &resumed.conversation_id, SessionStatus::Ready).await;
    assert_eq!(
        restored,
        vec!["doomed reply".to_string()],
        "the resumed session replays the failed session's history"
    );
    let request = resumed
        .handle
        .prompt(prompt_text("again"))
        .await
        .expect("prompt resumed session");
    collect_turn(&mut resumed.events, &request).await;

    let record = manager
        .get(&resumed.conversation_id)
        .expect("resumed session is registered");
    assert_eq!(
        (
            record.resumed_from,
            record.agent,
            manager.get(&a.conversation_id).is_none(),
            manager.list().len(),
        ),
        (Some(a.conversation_id), "mock-model".to_string(), true, 2,)
    );

    manager.shutdown().await;
}

#[tokio::test]
#[serial]
async fn shutdown_closes_every_session_and_refuses_new_ones() {
    let home_a = tempfile::tempdir().expect("temp dir");
    let home_b = tempfile::tempdir().expect("temp dir");
    let manager = SessionManager::new(SessionManagerConfig::default());
    let a = start(&manager, prepare_spec("mock-model", home_a.path(), None));
    let b = start(
        &manager,
        prepare_spec("mock-model-alt", home_b.path(), None),
    );
    wait_for_status(&manager, &a.conversation_id, SessionStatus::Ready).await;
    wait_for_status(&manager, &b.conversation_id, SessionStatus::Ready).await;

    let mut report = manager.shutdown().await;
    report.closed.sort_by_key(ToString::to_string);
    let mut expected = vec![a.conversation_id, b.conversation_id];
    expected.sort_by_key(ToString::to_string);

    wait_for_status(&manager, &a.conversation_id, SessionStatus::Closed).await;
    wait_for_status(&manager, &b.conversation_id, SessionStatus::Closed).await;
    let late = manager
        .start(StartSession {
            agent: SessionAgent::Prepare(prepare_spec("mock-model", home_a.path(), None)),
            start: SessionStart::New,
            parent: None,
            source: SessionSource::User,
        })
        .err();
    assert_eq!(
        (report.closed, report.timed_out, report.failed, late),
        (
            expected,
            Vec::new(),
            Vec::new(),
            Some(StartError::ShuttingDown)
        )
    );
}

#[tokio::test]
#[serial]
async fn the_cap_refuses_a_session_beyond_the_limit() {
    let home = tempfile::tempdir().expect("temp dir");
    let manager = SessionManager::new(SessionManagerConfig {
        max_sessions: 1,
        ..SessionManagerConfig::default()
    });
    let first = start(&manager, prepare_spec("mock-model", home.path(), None));
    wait_for_status(&manager, &first.conversation_id, SessionStatus::Ready).await;

    let refused = manager
        .start(StartSession {
            agent: SessionAgent::Prepare(prepare_spec("mock-model-alt", home.path(), None)),
            start: SessionStart::New,
            parent: None,
            source: SessionSource::User,
        })
        .err();
    manager
        .close(&first.conversation_id, Duration::ZERO)
        .await
        .expect("close first session");
    wait_for_status(&manager, &first.conversation_id, SessionStatus::Closed).await;
    let second = start(&manager, prepare_spec("mock-model-alt", home.path(), None));
    wait_for_status(&manager, &second.conversation_id, SessionStatus::Ready).await;

    assert_eq!(refused, Some(StartError::CapacityReached { limit: 1 }));
    manager.shutdown().await;
}

#[tokio::test]
#[serial]
async fn a_session_keeps_its_conversation_id_as_its_transcript_id() {
    let home = tempfile::tempdir().expect("temp dir");
    let manager = SessionManager::new(SessionManagerConfig::default());
    let mut session = start(&manager, prepare_spec("mock-model", home.path(), None));

    let transcript_id = tokio::time::timeout(WAIT, async {
        loop {
            let envelope = session.events.recv().await.expect("session stream open");
            if let SessionEvent::Nori(NoriEvent::SessionStarted(started)) = envelope.event {
                return started.transcript_id;
            }
        }
    })
    .await
    .expect("session starts");

    assert_eq!(transcript_id, Some(session.conversation_id.to_string()));
    manager.shutdown().await;
}

#[tokio::test]
#[serial]
async fn only_failed_sessions_can_be_resumed() {
    let home = tempfile::tempdir().expect("temp dir");
    let manager = SessionManager::new(SessionManagerConfig::default());
    let session = start(&manager, prepare_spec("mock-model", home.path(), None));
    wait_for_status(&manager, &session.conversation_id, SessionStatus::Ready).await;

    let result = manager
        .resume(
            &session.conversation_id,
            prepare_spec("mock-model", home.path(), None),
        )
        .await;

    assert!(
        matches!(result, Err(ResumeError::NotFailed(id)) if id == session.conversation_id),
        "resuming a ready session is refused"
    );
    manager.shutdown().await;
}

#[tokio::test]
#[serial]
async fn a_subscriber_that_falls_behind_is_dropped_without_slowing_sessions() {
    let home = tempfile::tempdir().expect("temp dir");
    let manager = SessionManager::new(SessionManagerConfig::default());
    let mut slow = manager.subscribe(1);
    let mut session = start(&manager, prepare_spec("mock-model", home.path(), None));
    wait_for_status(&manager, &session.conversation_id, SessionStatus::Ready).await;

    let request = session
        .handle
        .prompt(prompt_text("hello"))
        .await
        .expect("prompt");
    collect_turn(&mut session.events, &request).await;
    let first = slow.recv().await.map(|envelope| envelope.seq);
    let after_overflow = slow.recv().await.map(|envelope| envelope.seq);

    assert_eq!((first, after_overflow), (Some(1), None));
    manager.shutdown().await;
}
