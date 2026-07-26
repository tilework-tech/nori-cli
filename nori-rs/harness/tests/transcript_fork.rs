//! Behavior test for transcript forking on branch-at-head `/fork`.
//!
//! Drives a real harness session against the mock ACP agent and asserts on
//! observable transcript files and the public `SessionForked` event: branching
//! must fork the transcript into a fresh conversation seeded from the parent,
//! record the `forked_from` lineage, and leave the parent conversation frozen.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use nori_config::NoriConfig;
use nori_harness::runtime::LaunchedSession;
use nori_harness::runtime::SessionLaunchSpec;
use nori_harness::runtime::launch_session;
use nori_harness::transcript::Transcript;
use nori_harness::transcript::TranscriptLoader;
use nori_harness::transcript::TranscriptRecord;
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

fn launch(cwd: &Path) -> LaunchedSession {
    let config = NoriConfig {
        active_agent: "mock-model".to_string(),
        cwd: cwd.to_path_buf(),
        nori_home: cwd.to_path_buf(),
        ..Default::default()
    };
    launch_session(SessionLaunchSpec {
        config: Arc::new(config),
        cli_version: "transcript-fork-test".to_string(),
        session_context: None,
        initial_context: None,
        resume: None,
    })
}

#[expect(
    clippy::expect_used,
    reason = "focused failure for a missing bootstrap"
)]
async fn wait_for_started(session: &mut LaunchedSession) -> String {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            match session.events.recv().await.expect("event stream closed") {
                SessionEvent::Nori(NoriEvent::SessionStarted(started)) => {
                    return started.transcript_id.expect("bootstrap transcript id");
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

#[expect(clippy::expect_used, reason = "focused failure for a stalled prompt")]
async fn drive_prompt_to_completion(
    session: &mut LaunchedSession,
    request_id: &acp::v1::RequestId,
) {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            match session.events.recv().await.expect("event stream closed") {
                SessionEvent::Acp(AcpEvent::Response {
                    request_id: response_id,
                    response: Ok(acp::v1::AgentResponse::PromptResponse(_)),
                }) if &response_id == request_id => return,
                SessionEvent::Nori(NoriEvent::RequestFailed(failure)) => {
                    panic!("prompt failed: {failure:?}");
                }
                SessionEvent::Acp(_) | SessionEvent::Nori(_) => {}
            }
        }
    })
    .await
    .expect("prompt should complete");
}

/// Drive the event stream until the public `SessionForked` event arrives,
/// returning the (previous, new) conversation ids it carries.
#[expect(
    clippy::expect_used,
    reason = "focused failure for a missing fork event"
)]
async fn wait_for_fork(session: &mut LaunchedSession) -> (String, String) {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            match session.events.recv().await.expect("event stream closed") {
                SessionEvent::Nori(NoriEvent::SessionForked(forked)) => {
                    return (forked.previous_conversation_id, forked.new_conversation_id);
                }
                SessionEvent::Nori(NoriEvent::RequestFailed(failure)) => {
                    panic!("branch failed: {failure:?}");
                }
                SessionEvent::Acp(_) | SessionEvent::Nori(_) => {}
            }
        }
    })
    .await
    .expect("branch should emit SessionForked")
}

#[expect(clippy::expect_used, reason = "focused failure for a missing submit")]
async fn submit_text(session: &LaunchedSession, text: &str) -> acp::v1::RequestId {
    session
        .handle
        .prompt(vec![acp::v1::ContentBlock::Text(
            acp::v1::TextContent::new(text),
        )])
        .await
        .expect("submit prompt")
}

fn user_contents(transcript: &Transcript) -> Vec<String> {
    transcript
        .records()
        .filter_map(|record| match record {
            TranscriptRecord::User { content } => Some(content.to_string()),
            TranscriptRecord::Assistant { .. }
            | TranscriptRecord::Thinking { .. }
            | TranscriptRecord::SessionEvent(_) => None,
        })
        .collect()
}

#[tokio::test]
#[serial]
async fn branch_forks_transcript_and_freezes_parent() {
    // SAFETY: mock-agent env-mutating tests run serially.
    unsafe { std::env::set_var("MOCK_AGENT_SUPPORT_SESSION_FORK", "1") };
    let _guard = EnvGuard("MOCK_AGENT_SUPPORT_SESSION_FORK");

    let temp = tempfile::tempdir().expect("create session directory");
    let nori_home = temp.path().to_path_buf();
    let mut session = launch(temp.path());

    let parent_conversation_id = wait_for_started(&mut session).await;

    let before = submit_text(&session, "before fork").await;
    drive_prompt_to_completion(&mut session, &before).await;

    session
        .handle
        .branch()
        .await
        .expect("branch should succeed");
    let (previous_conversation_id, new_conversation_id) = wait_for_fork(&mut session).await;
    assert_eq!(previous_conversation_id, parent_conversation_id);
    assert_ne!(new_conversation_id, parent_conversation_id);

    let after = submit_text(&session, "after fork").await;
    drive_prompt_to_completion(&mut session, &after).await;

    session.handle.shutdown().await.expect("shutdown");

    let loader = TranscriptLoader::new(nori_home);
    let parent_meta = loader
        .find_session_metadata_by_id(&parent_conversation_id)
        .await
        .expect("lookup parent")
        .expect("parent metadata present");
    let project_id = parent_meta.project_id;

    let parent = loader
        .load_transcript(&project_id, &parent_conversation_id)
        .await
        .expect("load parent transcript");
    let child = loader
        .load_transcript(&project_id, &new_conversation_id)
        .await
        .expect("load child transcript");

    // Child records its lineage and carries both the seeded and post-fork turns.
    assert_eq!(
        child.meta.forked_from.as_deref(),
        Some(parent_conversation_id.as_str())
    );
    let child_users = user_contents(&child);
    assert!(
        child_users
            .iter()
            .any(|content| content.contains("before fork")),
        "child transcript should be seeded with the pre-fork turn: {child_users:?}"
    );
    assert!(
        child_users
            .iter()
            .any(|content| content.contains("after fork")),
        "child transcript should record the post-fork turn: {child_users:?}"
    );

    // Parent is frozen: no lineage, keeps the pre-fork turn, never sees the
    // post-fork turn (the recorder-swap hazard check).
    assert_eq!(parent.meta.forked_from, None);
    let parent_users = user_contents(&parent);
    assert!(
        parent_users
            .iter()
            .any(|content| content.contains("before fork")),
        "parent transcript should keep the pre-fork turn: {parent_users:?}"
    );
    assert!(
        !parent_users
            .iter()
            .any(|content| content.contains("after fork")),
        "parent transcript must stay frozen after the fork: {parent_users:?}"
    );
}
