//! End-to-end behavior of the `_session/goal` extension bridge.
//!
//! Drives a real harness session against the mock ACP agent with the goal
//! extension advertised and observes only external behavior: emitted
//! `SessionEvent`s and the recorded ACP wire log. Verifies that the harness
//! drives the goal through the extension, mirrors agent-published snapshots,
//! and never starts its own goal-continuation loop while the agent's native
//! loop owns the goal.

use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use nori_config::NoriConfig;
use nori_harness::runtime::AgentPrepareSpec;
use nori_harness::runtime::LaunchedSession;
use nori_harness::runtime::SessionLaunchSpec;
use nori_harness::runtime::SessionStart;
use nori_harness::runtime::launch_session;
use nori_harness::runtime::prepare_agent;
use nori_protocol::NoriEvent;
use nori_protocol::SessionEvent;
use nori_protocol::ThreadGoal;
use nori_protocol::ThreadGoalStatus;
use pretty_assertions::assert_eq;
use serial_test::serial;

struct EnvGuard(&'static str);

impl Drop for EnvGuard {
    fn drop(&mut self) {
        // SAFETY: tests that mutate the mock-agent environment run serially.
        unsafe { std::env::remove_var(self.0) };
    }
}

fn set_mock_env(name: &'static str) -> EnvGuard {
    // SAFETY: tests that mutate the mock-agent environment run serially.
    unsafe { std::env::set_var(name, "1") };
    EnvGuard(name)
}

#[expect(
    clippy::expect_used,
    reason = "focused failure for an agent preparation failure"
)]
async fn launch_with_wire_log(cwd: &Path, wire_log_dir: &Path) -> LaunchedSession {
    let config = NoriConfig {
        active_agent: "mock-model".to_string(),
        cwd: cwd.to_path_buf(),
        nori_home: cwd.to_path_buf(),
        acp_proxy: nori_config::AcpProxyConfig {
            enabled: true,
            log_dir: wire_log_dir.to_path_buf(),
        },
        ..Default::default()
    };
    let agent = prepare_agent(AgentPrepareSpec {
        config: Arc::new(config),
        cli_version: "goal-ext-bridge-test".to_string(),
        session_context: None,
        initial_context: None,
    })
    .await
    .expect("prepare mock agent");
    launch_session(SessionLaunchSpec {
        agent,
        start: SessionStart::New,
    })
}

#[expect(
    clippy::expect_used,
    reason = "focused failures for a missing bootstrap"
)]
async fn wait_for_started(session: &mut LaunchedSession) {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            match session.events.recv().await.expect("event stream closed") {
                SessionEvent::Nori(NoriEvent::SessionStarted(_)) => return,
                SessionEvent::Nori(NoriEvent::RequestFailed(failure)) => {
                    panic!("session bootstrap failed: {failure:?}");
                }
                SessionEvent::Acp(_) | SessionEvent::Nori(_) => {}
            }
        }
    })
    .await
    .expect("session should start");
}

#[expect(
    clippy::expect_used,
    reason = "focused failures for a missing goal event"
)]
async fn wait_for_goal_status(
    session: &mut LaunchedSession,
    status: ThreadGoalStatus,
) -> ThreadGoal {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            match session.events.recv().await.expect("event stream closed") {
                SessionEvent::Nori(NoriEvent::GoalChanged(Some(goal))) if goal.status == status => {
                    return goal;
                }
                SessionEvent::Acp(_) | SessionEvent::Nori(_) => {}
            }
        }
    })
    .await
    .unwrap_or_else(|_| panic!("goal should reach status {status:?}"))
}

#[expect(
    clippy::expect_used,
    reason = "focused failures for a missing goal event"
)]
async fn wait_for_goal_cleared(session: &mut LaunchedSession) {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            match session.events.recv().await.expect("event stream closed") {
                SessionEvent::Nori(NoriEvent::GoalChanged(None)) => return,
                SessionEvent::Acp(_) | SessionEvent::Nori(_) => {}
            }
        }
    })
    .await
    .expect("goal should clear");
}

/// The recorded `session/prompt` params that reached the agent, in order.
#[expect(
    clippy::expect_used,
    reason = "focused failures for a missing wire log"
)]
fn recorded_prompts(wire_log_dir: &Path) -> Vec<serde_json::Value> {
    let mut wire_logs = std::fs::read_dir(wire_log_dir)
        .expect("wire log directory")
        .map(|entry| entry.expect("wire log entry").path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "jsonl")
        })
        .collect::<Vec<_>>();
    assert_eq!(
        wire_logs.len(),
        1,
        "one ACP child should produce one wire log"
    );
    let wire_log = wire_logs.pop().expect("ACP wire log");
    std::fs::read_to_string(wire_log)
        .expect("read ACP wire log")
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).expect("valid wire log line"))
        .filter(|record| {
            record["direction"] == "client_to_agent"
                && record["message"]["method"] == "session/prompt"
        })
        .map(|record| record["message"]["params"].clone())
        .collect()
}

#[expect(clippy::expect_used, reason = "focused failure for a missing tempdir")]
fn temp_dirs() -> (tempfile::TempDir, PathBuf) {
    let temp = tempfile::tempdir().expect("create session directory");
    let wire_log_dir = temp.path().join("wire-logs");
    (temp, wire_log_dir)
}

#[tokio::test(flavor = "multi_thread")]
#[serial]
async fn goal_extension_drives_goal_and_suppresses_harness_continuation() {
    let _goal_ext = set_mock_env("MOCK_AGENT_GOAL_EXT");
    let _autocomplete = set_mock_env("MOCK_AGENT_GOAL_EXT_AUTOCOMPLETE");
    let (temp, wire_log_dir) = temp_dirs();

    let mut session = launch_with_wire_log(temp.path(), &wire_log_dir).await;
    wait_for_started(&mut session).await;

    // Setting the goal drives the extension. The mock completes the goal the
    // moment it accepts it, so the returned mirror may already be past
    // `Active`; the objective is what set_goal guarantees.
    let goal = session
        .handle
        .set_goal("finish the demo".to_string(), None)
        .await
        .expect("set_goal should succeed via the goal extension");
    assert_eq!(goal.objective, "finish the demo");

    // The mock's native loop completes the goal on its own; the harness
    // mirrors the published snapshot into its store and event stream.
    let completed = wait_for_goal_status(&mut session, ThreadGoalStatus::Complete).await;
    assert_eq!(completed.objective, "finish the demo");
    let current = session.handle.goal().await.expect("goal query");
    assert_eq!(
        current.map(|goal| goal.status),
        Some(ThreadGoalStatus::Complete)
    );

    // The harness continuation loop stayed off while the extension drove the
    // goal: no prompt of any kind (goal continuations included) reached the
    // agent.
    assert_eq!(
        recorded_prompts(&wire_log_dir),
        Vec::<serde_json::Value>::new()
    );
}

#[tokio::test(flavor = "multi_thread")]
#[serial]
async fn goal_extension_clear_round_trips() {
    let _goal_ext = set_mock_env("MOCK_AGENT_GOAL_EXT");
    let (temp, wire_log_dir) = temp_dirs();

    let mut session = launch_with_wire_log(temp.path(), &wire_log_dir).await;
    wait_for_started(&mut session).await;

    let goal = session
        .handle
        .set_goal("keep going".to_string(), None)
        .await
        .expect("set_goal should succeed via the goal extension");
    assert_eq!(goal.status, ThreadGoalStatus::Active);
    // Drain the agent's "active" snapshot through the relay before clearing,
    // so the mirrored snapshot cannot resurrect the goal after the clear.
    wait_for_goal_status(&mut session, ThreadGoalStatus::Active).await;

    session.handle.clear_goal().await.expect("clear_goal");
    wait_for_goal_cleared(&mut session).await;
    let current = session.handle.goal().await.expect("goal query");
    assert_eq!(current, None);
    assert_eq!(
        recorded_prompts(&wire_log_dir),
        Vec::<serde_json::Value>::new()
    );
}
