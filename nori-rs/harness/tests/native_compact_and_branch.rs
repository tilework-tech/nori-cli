//! Behavior tests for native `/compact` forwarding and branch-at-head `/fork`.
//!
//! These drive a real harness session against the mock ACP agent and observe
//! only external behavior: emitted `SessionEvent`s and the recorded ACP wire
//! log (session ids and prompt content that actually reached the agent).

use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use nori_config::NoriConfig;
use nori_harness::runtime::LaunchedSession;
use nori_harness::runtime::SessionLaunchSpec;
use nori_harness::runtime::launch_session;
use nori_protocol::AcpEvent;
use nori_protocol::NoriEvent;
use nori_protocol::SessionEvent;
use nori_protocol::acp;
use pretty_assertions::assert_eq;
use serial_test::serial;

struct EnvGuard(&'static str);

impl Drop for EnvGuard {
    fn drop(&mut self) {
        // SAFETY: tests that mutate the mock-agent environment run serially.
        unsafe { std::env::remove_var(self.0) };
    }
}

/// Launch a mock-agent session with ACP wire recording enabled so tests can
/// observe the exact prompts and session ids that reached the agent.
fn launch_with_wire_log(cwd: &Path, wire_log_dir: &Path) -> LaunchedSession {
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
    launch_session(SessionLaunchSpec {
        config: Arc::new(config),
        cli_version: "native-compact-branch-test".to_string(),
        session_context: None,
        initial_context: None,
        resume: None,
    })
}

#[expect(
    clippy::expect_used,
    reason = "focused failures for a missing bootstrap"
)]
async fn wait_for_started(session: &mut LaunchedSession) -> String {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            match session.events.recv().await.expect("event stream closed") {
                SessionEvent::Nori(NoriEvent::SessionStarted(started)) => {
                    return started.acp_session_id.to_string();
                }
                SessionEvent::Nori(NoriEvent::RequestFailed(failure)) => {
                    panic!("session bootstrap failed: {failure:?}");
                }
                SessionEvent::Acp(_) | SessionEvent::Nori(_) => {}
            }
        }
    })
    .await
    .expect("session should start")
}

/// Drive the event stream until one prompt turn completes (a `PromptResponse`
/// arrives for the given request id), returning whether a `ContextCompacted`
/// event was observed along the way.
#[expect(clippy::expect_used, reason = "focused failures for a stalled prompt")]
async fn drive_prompt_to_completion(
    session: &mut LaunchedSession,
    request_id: &acp::v1::RequestId,
) -> bool {
    tokio::time::timeout(Duration::from_secs(10), async {
        let mut saw_context_compacted = false;
        loop {
            match session.events.recv().await.expect("event stream closed") {
                SessionEvent::Nori(NoriEvent::ContextCompacted(_)) => {
                    saw_context_compacted = true;
                }
                SessionEvent::Acp(AcpEvent::Response {
                    request_id: response_id,
                    response: Ok(acp::v1::AgentResponse::PromptResponse(_)),
                }) if &response_id == request_id => return saw_context_compacted,
                SessionEvent::Nori(NoriEvent::RequestFailed(failure)) => {
                    panic!("prompt failed: {failure:?}");
                }
                SessionEvent::Acp(_) | SessionEvent::Nori(_) => {}
            }
        }
    })
    .await
    .expect("prompt should complete")
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

fn first_prompt_text(params: &serde_json::Value) -> &str {
    params["prompt"][0]["text"].as_str().unwrap_or_default()
}

fn prompt_session_id(params: &serde_json::Value) -> &str {
    params["sessionId"].as_str().unwrap_or_default()
}

async fn submit_text(session: &LaunchedSession, text: &str) -> acp::v1::RequestId {
    session
        .handle
        .prompt(vec![acp::v1::ContentBlock::Text(
            acp::v1::TextContent::new(text),
        )])
        .await
        .unwrap_or_else(|error| panic!("submit prompt {text:?}: {error}"))
}

#[expect(clippy::expect_used, reason = "focused failure for a missing tempdir")]
fn temp_dirs() -> (tempfile::TempDir, PathBuf) {
    let temp = tempfile::tempdir().expect("create session directory");
    let wire = temp.path().join("acp-wire");
    (temp, wire)
}

#[tokio::test]
#[serial]
async fn native_compact_forwards_without_swapping_session() {
    // SAFETY: mock-agent env-mutating tests run serially.
    unsafe { std::env::set_var("MOCK_AGENT_ADVERTISE_COMPACT", "1") };
    let _guard = EnvGuard("MOCK_AGENT_ADVERTISE_COMPACT");
    let (temp, wire) = temp_dirs();
    let mut session = launch_with_wire_log(temp.path(), &wire);

    let bootstrap_session_id = wait_for_started(&mut session).await;

    // Warm-up turn flushes the agent's AvailableCommandsUpdate into the runtime
    // so the subsequent /compact is recognized as a native command.
    let warmup = submit_text(&session, "warmup").await;
    drive_prompt_to_completion(&mut session, &warmup).await;

    session.handle.compact().await.expect("compact");
    // The native compact is forwarded as an ordinary turn ("/compact").
    let compact_saw_context_compacted = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let SessionEvent::Nori(NoriEvent::ContextCompacted(_)) =
                session.events.recv().await.expect("event stream closed")
            {
                return true;
            }
        }
    })
    .await
    .expect("native compact should emit ContextCompacted");
    assert!(compact_saw_context_compacted);

    let follow_up = submit_text(&session, "hi").await;
    drive_prompt_to_completion(&mut session, &follow_up).await;

    session.handle.shutdown().await.expect("shutdown");

    let prompts = recorded_prompts(&wire);
    assert_eq!(
        prompts.len(),
        3,
        "warm-up, native /compact, and follow-up prompts should all reach the agent"
    );
    // All three prompts stayed on the original session: native compact did not
    // swap the ACP session id.
    for params in &prompts {
        assert_eq!(prompt_session_id(params), bootstrap_session_id);
    }
    assert_eq!(first_prompt_text(&prompts[1]), "/compact");
    // The follow-up prompt is verbatim: no summary prefix, no summarize-and-swap.
    assert_eq!(first_prompt_text(&prompts[2]), "hi");
}

#[tokio::test]
#[serial]
async fn compact_without_native_support_swaps_and_injects_summary() {
    let (temp, wire) = temp_dirs();
    let mut session = launch_with_wire_log(temp.path(), &wire);

    let bootstrap_session_id = wait_for_started(&mut session).await;

    session.handle.compact().await.expect("compact");
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let SessionEvent::Nori(NoriEvent::ContextCompacted(_)) =
                session.events.recv().await.expect("event stream closed")
            {
                return;
            }
        }
    })
    .await
    .expect("fallback compact should emit ContextCompacted");

    let follow_up = submit_text(&session, "hi").await;
    drive_prompt_to_completion(&mut session, &follow_up).await;

    session.handle.shutdown().await.expect("shutdown");

    let prompts = recorded_prompts(&wire);
    assert_eq!(
        prompts.len(),
        2,
        "summarization prompt and follow-up prompt should both reach the agent"
    );
    // The summarization prompt ran on the original session; the follow-up ran
    // on a freshly created session (summarize-and-swap).
    assert_eq!(prompt_session_id(&prompts[0]), bootstrap_session_id);
    assert_ne!(
        prompt_session_id(&prompts[1]),
        bootstrap_session_id,
        "fallback compact should create and swap to a new session"
    );
    assert!(
        first_prompt_text(&prompts[1]).contains(nori_harness::compact::SUMMARY_PREFIX),
        "fallback compact must inject the summary prefix into the next prompt"
    );
}

#[tokio::test]
#[serial]
async fn branch_forks_active_session() {
    // SAFETY: mock-agent env-mutating tests run serially.
    unsafe { std::env::set_var("MOCK_AGENT_SUPPORT_SESSION_FORK", "1") };
    let _guard = EnvGuard("MOCK_AGENT_SUPPORT_SESSION_FORK");
    let (temp, wire) = temp_dirs();
    let mut session = launch_with_wire_log(temp.path(), &wire);

    let bootstrap_session_id = wait_for_started(&mut session).await;

    session
        .handle
        .branch()
        .await
        .expect("branch should succeed");

    // The fork response carries the new (forked) session id.
    let forked_session_id = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let SessionEvent::Acp(AcpEvent::Response {
                response: Ok(acp::v1::AgentResponse::ForkSessionResponse(response)),
                ..
            }) = session.events.recv().await.expect("event stream closed")
            {
                return response.session_id.to_string();
            }
        }
    })
    .await
    .expect("branch should fork via session/fork");
    assert_ne!(forked_session_id, bootstrap_session_id);

    // The active session is now the forked one: the next prompt targets it.
    let after_branch = submit_text(&session, "after branch").await;
    drive_prompt_to_completion(&mut session, &after_branch).await;

    session.handle.shutdown().await.expect("shutdown");

    let prompts = recorded_prompts(&wire);
    assert_eq!(prompts.len(), 1, "one prompt should have reached the agent");
    assert_eq!(
        prompt_session_id(&prompts[0]),
        forked_session_id,
        "post-branch prompt should target the forked session"
    );
}

#[tokio::test]
#[serial]
async fn branch_unsupported_agent_errors() {
    let (temp, wire) = temp_dirs();
    let mut session = launch_with_wire_log(temp.path(), &wire);

    let bootstrap_session_id = wait_for_started(&mut session).await;

    let error = session
        .handle
        .branch()
        .await
        .expect_err("branch should fail without fork support");
    assert!(
        error.to_string().contains("does not support branching"),
        "unexpected branch error: {error}"
    );

    // The session id is unchanged: a subsequent prompt still targets it.
    let after = submit_text(&session, "still here").await;
    drive_prompt_to_completion(&mut session, &after).await;

    session.handle.shutdown().await.expect("shutdown");

    let prompts = recorded_prompts(&wire);
    assert_eq!(prompts.len(), 1);
    assert_eq!(prompt_session_id(&prompts[0]), bootstrap_session_id);
}

#[tokio::test]
#[serial]
async fn branch_leaves_session_intact_when_fork_rpc_fails() {
    // SAFETY: mock-agent env-mutating tests run serially.
    unsafe {
        std::env::set_var("MOCK_AGENT_SUPPORT_SESSION_FORK", "1");
        std::env::set_var("MOCK_AGENT_FORK_SESSION_FAIL", "1");
    }
    let _support_guard = EnvGuard("MOCK_AGENT_SUPPORT_SESSION_FORK");
    let _fail_guard = EnvGuard("MOCK_AGENT_FORK_SESSION_FAIL");
    let (temp, wire) = temp_dirs();
    let mut session = launch_with_wire_log(temp.path(), &wire);

    let bootstrap_session_id = wait_for_started(&mut session).await;

    // The agent advertises fork, so the capability gate passes, but the fork
    // RPC itself fails. Branch should surface the error and leave the active
    // session untouched.
    session
        .handle
        .branch()
        .await
        .expect_err("branch should fail when the fork RPC errors");

    let after = submit_text(&session, "still here").await;
    drive_prompt_to_completion(&mut session, &after).await;

    session.handle.shutdown().await.expect("shutdown");

    let prompts = recorded_prompts(&wire);
    assert_eq!(prompts.len(), 1);
    assert_eq!(prompt_session_id(&prompts[0]), bootstrap_session_id);
}
