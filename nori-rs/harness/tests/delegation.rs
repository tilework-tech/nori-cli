//! Delegation boundaries exercised with real ACP subprocesses and their MCP client.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "integration assertions identify failed behavior"
)]
use nori_config::NoriConfig;
use nori_harness::ConversationId;
use nori_harness::remote_agent::ManagedAgent;
use nori_harness::remote_agent::ManagedRemoteHost;
use nori_harness::runtime::AgentPrepareSpec;
use nori_harness::runtime::SessionStart;
use nori_harness::session_manager::ManagedSession;
use nori_harness::session_manager::SessionAgent;
use nori_harness::session_manager::SessionManager;
use nori_harness::session_manager::SessionManagerConfig;
use nori_harness::session_manager::SessionSource;
use nori_harness::session_manager::StartSession;
use nori_protocol::AcpEvent;
use nori_protocol::Delegation;
use nori_protocol::DelegationStatus;
use nori_protocol::NoriEvent;
use nori_protocol::SessionEvent;
use nori_protocol::SessionStatus;
use nori_protocol::acp;
use serial_test::serial;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

const WAIT: Duration = Duration::from_secs(20);
struct Env(&'static str, Option<String>);
impl Env {
    fn set(key: &'static str, value: &str) -> Self {
        let old = std::env::var(key).ok();
        unsafe {
            std::env::set_var(key, value);
        }
        Self(key, old)
    }
}
impl Drop for Env {
    fn drop(&mut self) {
        unsafe {
            match &self.1 {
                Some(v) => std::env::set_var(self.0, v),
                None => std::env::remove_var(self.0),
            }
        }
    }
}
fn spec(home: &Path, enabled: bool) -> AgentPrepareSpec {
    AgentPrepareSpec {
        config: Arc::new(NoriConfig {
            active_agent: "mock-model".into(),
            cwd: home.into(),
            nori_home: home.into(),
            multi_session: enabled,
            ..Default::default()
        }),
        cli_version: "delegation-test".into(),
        session_context: None,
        initial_context: None,
    }
}
fn start(manager: &SessionManager, home: &Path, enabled: bool) -> ManagedSession {
    manager
        .start(StartSession {
            agent: SessionAgent::Prepare(spec(home, enabled)),
            start: SessionStart::New,
            parent: None,
            source: SessionSource::User,
        })
        .expect("parent starts")
}
fn text(s: &str) -> Vec<acp::v1::ContentBlock> {
    vec![acp::v1::ContentBlock::Text(acp::v1::TextContent::new(s))]
}
fn id(d: &Delegation) -> ConversationId {
    ConversationId::from_string(&d.child_id).expect("child identity")
}
async fn status(m: &SessionManager, id: &ConversationId, wanted: SessionStatus) {
    tokio::time::timeout(WAIT, async {
        loop {
            if m.get(id).is_some_and(|r| r.status == wanted) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("session reaches status");
}
async fn terminal(m: &SessionManager, parent: &ConversationId, request: &str) -> Delegation {
    tokio::time::timeout(WAIT, async {
        loop {
            if let Some(d) = m.delegations(parent).into_iter().find(|d| {
                d.request_id == request
                    && matches!(
                        d.status,
                        DelegationStatus::Completed
                            | DelegationStatus::Failed
                            | DelegationStatus::Cancelled
                            | DelegationStatus::Declined
                    )
            }) {
                return d;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("delegation completes")
}
fn manager(cap: i32) -> SessionManager {
    SessionManager::new(SessionManagerConfig {
        max_sessions: cap,
        ..Default::default()
    })
}

async fn wait_parent_reply(parent: &mut ManagedSession, child: &str) -> String {
    tokio::time::timeout(WAIT, async {
        loop {
            if let SessionEvent::Acp(AcpEvent::Notification(
                acp::v1::AgentNotification::SessionNotification(n),
            )) = parent.events.recv().await.unwrap().event
                && let acp::v1::SessionUpdate::AgentMessageChunk(chunk) = n.update
                && let acp::v1::ContentBlock::Text(text) = chunk.content
                && text.text.contains(child)
            {
                return text.text;
            }
        }
    })
    .await
    .expect("parent agent consumes attributed child result")
}

#[tokio::test]
#[serial]
async fn same_kind_starts_immediately_and_retains_child_history_after_releasing_slot() {
    let home = tempfile::tempdir().unwrap();
    let m = manager(2);
    let p = start(&m, home.path(), true);
    status(&m, &p.conversation_id, SessionStatus::Ready).await;
    let d = m
        .delegate(
            &p.conversation_id,
            "mock-model".into(),
            "summarize E1".into(),
        )
        .await
        .unwrap();
    assert_eq!(d.status, DelegationStatus::Running);
    let done = terminal(&m, &p.conversation_id, &d.request_id).await;
    assert_eq!(done.status, DelegationStatus::Completed);
    assert!(done.result.is_some());
    status(&m, &id(&d), SessionStatus::Closed).await;
    assert_eq!(m.free_slots(), 1);
    let child = m.get(&id(&d)).unwrap();
    assert_eq!(child.parent, Some(p.conversation_id));
    assert_eq!(child.source, SessionSource::Session);
    assert!(child.transcript_path.is_some());
    m.shutdown(Duration::ZERO).await;
}
#[tokio::test]
#[serial]
async fn different_kind_waits_for_approval_and_decline_never_launches() {
    let home = tempfile::tempdir().unwrap();
    let m = manager(3);
    let p = start(&m, home.path(), true);
    status(&m, &p.conversation_id, SessionStatus::Ready).await;
    let d = m
        .delegate(
            &p.conversation_id,
            "mock-model-alt".into(),
            "child task".into(),
        )
        .await
        .unwrap();
    assert_eq!(d.status, DelegationStatus::AwaitingApproval);
    assert_eq!(m.list().len(), 1);
    let no = m
        .resolve_delegation(&p.conversation_id, &d.request_id, false)
        .await
        .unwrap();
    assert_eq!(no.status, DelegationStatus::Declined);
    assert_eq!(m.list().len(), 1);
    assert!(
        m.resolve_delegation(&p.conversation_id, &d.request_id, true)
            .await
            .is_err()
    );
    let d = m
        .delegate(
            &p.conversation_id,
            "mock-model-alt".into(),
            "approved task".into(),
        )
        .await
        .unwrap();
    let yes = m
        .resolve_delegation(&p.conversation_id, &d.request_id, true)
        .await
        .unwrap();
    assert_eq!(yes.status, DelegationStatus::Running);
    assert_eq!(
        terminal(&m, &p.conversation_id, &d.request_id).await.status,
        DelegationStatus::Completed
    );
    m.shutdown(Duration::ZERO).await;
}
#[tokio::test]
#[serial]
async fn depth_two_is_allowed_and_depth_three_is_rejected() {
    let home = tempfile::tempdir().unwrap();
    let m = manager(5);
    let p = start(&m, home.path(), true);
    status(&m, &p.conversation_id, SessionStatus::Ready).await;
    let a = m
        .delegate(
            &p.conversation_id,
            "mock-model".into(),
            "mock:stream-until-cancel".into(),
        )
        .await
        .unwrap();
    status(&m, &id(&a), SessionStatus::Working).await;
    let b = m
        .delegate(
            &id(&a),
            "mock-model".into(),
            "mock:stream-until-cancel".into(),
        )
        .await
        .unwrap();
    status(&m, &id(&b), SessionStatus::Working).await;
    assert_eq!(b.depth, 2);
    let err = m
        .delegate(&id(&b), "mock-model".into(), "too deep".into())
        .await
        .unwrap_err();
    assert!(err.to_string().contains("depth"));
    m.shutdown(Duration::ZERO).await;
}
#[tokio::test]
#[serial]
async fn child_permission_belongs_to_child_and_family_reports_waiting() {
    let home = tempfile::tempdir().unwrap();
    let m = manager(3);
    let p = start(&m, home.path(), true);
    status(&m, &p.conversation_id, SessionStatus::Ready).await;
    let mut events = m.subscribe(4096);
    let d = m
        .delegate(
            &p.conversation_id,
            "mock-model".into(),
            "mock:request-permission:echo child".into(),
        )
        .await
        .unwrap();
    let request = tokio::time::timeout(WAIT, async {
        loop {
            let e = events.recv().await.unwrap();
            if let SessionEvent::Acp(AcpEvent::Request {
                request_id,
                request: acp::v1::AgentRequest::RequestPermissionRequest(_),
            }) = e.event
            {
                assert_eq!(e.conversation_id, d.child_id);
                return request_id;
            }
        }
    })
    .await
    .unwrap();
    let notice = tokio::time::timeout(WAIT, async {
        loop {
            let e = events.recv().await.unwrap();
            if e.conversation_id == p.conversation_id.to_string()
                && let SessionEvent::Nori(NoriEvent::Notice(notice)) = e.event
                && notice.message.contains("waiting on you")
            {
                return notice.message;
            }
        }
    })
    .await
    .expect("parent hears that its child is waiting");
    assert!(
        notice.contains("mock:request-permission:echo child") && notice.contains("mock-model"),
        "the notice names the asking child by its task: {notice}"
    );
    assert!(
        !notice.contains(&d.child_id) && !notice.contains(&d.parent_id),
        "the notice uses readable names: {notice}"
    );
    assert!(notice.contains("Alt+Down"), "{notice}");
    assert_eq!(m.get(&id(&d)).unwrap().parent, Some(p.conversation_id));
    assert!(
        m.get(&p.conversation_id)
            .unwrap()
            .pending_requests
            .is_empty()
    );
    assert_eq!(
        m.family_status(&p.conversation_id),
        Some(SessionStatus::WaitingOnYou)
    );
    m.handle(&id(&d))
        .unwrap()
        .respond_to_agent(
            request,
            Ok(acp::v1::ClientResponse::RequestPermissionResponse(
                acp::v1::RequestPermissionResponse::new(
                    acp::v1::RequestPermissionOutcome::Cancelled,
                ),
            )),
        )
        .await
        .unwrap();
    let done = terminal(&m, &p.conversation_id, &d.request_id).await;
    let result = done.result.unwrap_or_default();
    assert!(
        result
            .lines()
            .any(|line| line.starts_with("Permission request was cancelled")),
        "text after the permission request starts its own paragraph: {result:?}"
    );
    m.shutdown(Duration::ZERO).await;
}
#[tokio::test]
#[serial]
async fn result_is_automatically_submitted_as_attributed_parent_input() {
    let _echo = Env::set("MOCK_AGENT_ECHO_PROMPT", "1");
    let home = tempfile::tempdir().unwrap();
    let m = manager(3);
    let mut p = start(&m, home.path(), true);
    status(&m, &p.conversation_id, SessionStatus::Ready).await;
    let d = m
        .delegate(
            &p.conversation_id,
            "mock-model".into(),
            "unique child payload".into(),
        )
        .await
        .unwrap();
    tokio::time::timeout(WAIT, async {
        loop {
            let e = p.events.recv().await.unwrap();
            if let SessionEvent::Acp(AcpEvent::Notification(
                acp::v1::AgentNotification::SessionNotification(n),
            )) = e.event
                && let acp::v1::SessionUpdate::AgentMessageChunk(c) = n.update
                && let acp::v1::ContentBlock::Text(t) = c.content
                && t.text.contains("unique child payload")
            {
                assert!(
                    t.text.contains(&d.child_id),
                    "the agent can still address the child: {}",
                    t.text
                );
                assert!(
                    t.text.contains("from mock-model · unique child payload"),
                    "the result names the child readably: {}",
                    t.text
                );
                break;
            }
        }
    })
    .await
    .expect("automatic result reaches parent agent");
    assert_eq!(
        d.origin,
        nori_protocol::DelegationOrigin::User,
        "a manager call made for the user is attributed to the user"
    );
    m.shutdown(Duration::ZERO).await;
}
#[tokio::test]
#[serial]
async fn closing_parent_cancels_running_descendants_and_pending_approvals() {
    let home = tempfile::tempdir().unwrap();
    let m = manager(5);
    let p = start(&m, home.path(), true);
    status(&m, &p.conversation_id, SessionStatus::Ready).await;
    let a = m
        .delegate(
            &p.conversation_id,
            "mock-model".into(),
            "mock:stream-until-cancel".into(),
        )
        .await
        .unwrap();
    status(&m, &id(&a), SessionStatus::Working).await;
    let grandchild = m
        .delegate(
            &id(&a),
            "mock-model".into(),
            "mock:stream-until-cancel".into(),
        )
        .await
        .unwrap();
    status(&m, &id(&grandchild), SessionStatus::Working).await;
    let pending = m
        .delegate(
            &p.conversation_id,
            "mock-model-alt".into(),
            "not started".into(),
        )
        .await
        .unwrap();
    m.close(&p.conversation_id, Duration::ZERO).await.unwrap();
    assert_eq!(
        terminal(&m, &p.conversation_id, &a.request_id).await.status,
        DelegationStatus::Cancelled
    );
    assert_eq!(
        terminal(&m, &p.conversation_id, &pending.request_id)
            .await
            .status,
        DelegationStatus::Cancelled
    );
    assert!(
        m.resolve_delegation(&p.conversation_id, &pending.request_id, true)
            .await
            .is_err()
    );
    assert_eq!(
        terminal(&m, &id(&a), &grandchild.request_id).await.status,
        DelegationStatus::Cancelled
    );
    assert_eq!(m.free_slots(), 5);
}
#[tokio::test]
#[serial]
async fn child_crash_returns_failure_without_failing_parent() {
    let home = tempfile::tempdir().unwrap();
    let m = manager(3);
    let mut p = start(&m, home.path(), true);
    status(&m, &p.conversation_id, SessionStatus::Ready).await;
    let d = m
        .delegate(
            &p.conversation_id,
            "mock-model".into(),
            "mock:exit-during-prompt".into(),
        )
        .await
        .unwrap();
    let done = terminal(&m, &p.conversation_id, &d.request_id).await;
    assert_eq!(done.status, DelegationStatus::Failed);
    assert_eq!(
        done.result.as_deref(),
        Some("Agent process exited unexpectedly (exit code 17).")
    );
    assert!(m.handle(&p.conversation_id).is_some());
    let reply = wait_parent_reply(&mut p, &d.child_id).await;
    assert!(
        reply.contains("from mock-model · mock:exit-during-prompt · failed"),
        "{reply}"
    );
    assert!(reply.contains("Agent process exited unexpectedly (exit code 17)."));
    assert!(!reply.contains("RequestFailure"), "{reply}");
    m.shutdown(Duration::ZERO).await;
}
#[tokio::test]
#[serial]
async fn competing_children_share_the_same_session_cap() {
    let home = tempfile::tempdir().unwrap();
    let m = manager(2);
    let p = start(&m, home.path(), true);
    status(&m, &p.conversation_id, SessionStatus::Ready).await;
    let (a, b) = tokio::join!(
        m.delegate(
            &p.conversation_id,
            "mock-model".into(),
            "mock:stream-until-cancel".into()
        ),
        m.delegate(
            &p.conversation_id,
            "mock-model".into(),
            "mock:stream-until-cancel".into()
        )
    );
    assert_ne!(a.is_ok(), b.is_ok());
    assert_eq!(m.free_slots(), 0);
    m.shutdown(Duration::ZERO).await;
}
#[tokio::test]
#[serial]
async fn flag_off_rejects_delegation_without_starting_child() {
    let home = tempfile::tempdir().unwrap();
    let m = manager(3);
    let p = start(&m, home.path(), false);
    status(&m, &p.conversation_id, SessionStatus::Ready).await;
    assert!(
        m.delegate(&p.conversation_id, "mock-model".into(), "task".into())
            .await
            .unwrap_err()
            .to_string()
            .contains("multi_session")
    );
    assert_eq!(m.list().len(), 1);
    m.shutdown(Duration::ZERO).await;
}
#[tokio::test]
#[serial]
async fn actual_agent_mcp_request_starts_child_and_nested_results_finish_once() {
    let _http = Env::set("MOCK_AGENT_MCP_HTTP", "1");
    let _echo = Env::set("MOCK_AGENT_ECHO_PROMPT", "1");
    let home = tempfile::tempdir().unwrap();
    let m = manager(4);
    let p = start(&m, home.path(), true);
    status(&m, &p.conversation_id, SessionStatus::Ready).await;
    let mut events = m.subscribe(4096);
    p.handle
        .prompt(text(
            "mock:delegate mock-model mock:delegate mock-model nested leaf",
        ))
        .await
        .unwrap();
    tokio::time::timeout(WAIT, async {
        loop {
            let ds = m.delegations(&p.conversation_id);
            if ds.iter().any(|d| d.status == DelegationStatus::Completed) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("nested delegation completes");
    let top = m.delegations(&p.conversation_id).pop().unwrap();
    assert_eq!(
        top.origin,
        nori_protocol::DelegationOrigin::Agent,
        "an MCP request is attributed to the agent"
    );
    let nested = m.delegations(&id(&top));
    assert_eq!(nested.len(), 1);
    assert_eq!(nested[0].status, DelegationStatus::Completed);
    assert!(top.result.as_ref().unwrap().contains("nested leaf"));
    assert!(top.result.as_ref().unwrap().contains(&nested[0].child_id));
    let mut completions = 0;
    let mut grandchild_finished = false;
    let mut continuation_finished = false;
    while let Ok(e) = events.try_recv() {
        match e.event {
            SessionEvent::Nori(NoriEvent::DelegationChanged(d))
                if d.status == DelegationStatus::Completed =>
            {
                if d.request_id == nested[0].request_id {
                    grandchild_finished = true;
                }
                if d.request_id == top.request_id
                    && e.conversation_id == p.conversation_id.to_string()
                {
                    assert!(
                        continuation_finished,
                        "child continuation finished before reporting final result"
                    );
                    completions += 1;
                }
            }
            SessionEvent::Acp(AcpEvent::Response {
                response: Ok(acp::v1::AgentResponse::PromptResponse(_)),
                ..
            }) if e.conversation_id == top.child_id && grandchild_finished => {
                continuation_finished = true
            }
            _ => {}
        }
    }
    assert_eq!(completions, 1);
    m.shutdown(Duration::ZERO).await;
}
#[tokio::test]
#[serial]
async fn crashed_parent_preserves_child_and_result_reaches_stable_resume() {
    let _delay = Env::set("MOCK_AGENT_DELAY_MS", "500");
    let _response = Env::set("MOCK_AGENT_RESPONSE", "held unique result");
    let home = tempfile::tempdir().unwrap();
    let m = manager(3);
    let p = start(&m, home.path(), true);
    status(&m, &p.conversation_id, SessionStatus::Ready).await;
    let d = m
        .delegate(
            &p.conversation_id,
            "mock-model".into(),
            "surviving child".into(),
        )
        .await
        .unwrap();
    p.handle
        .prompt(text("mock:exit-during-prompt"))
        .await
        .unwrap();
    status(&m, &p.conversation_id, SessionStatus::Failed).await;
    assert_eq!(
        terminal(&m, &p.conversation_id, &d.request_id).await.status,
        DelegationStatus::Completed
    );
    let _echo = Env::set("MOCK_AGENT_ECHO_PROMPT", "1");
    let mut resumed = m
        .resume(&p.conversation_id, spec(home.path(), true))
        .await
        .unwrap();
    assert_eq!(resumed.conversation_id, p.conversation_id);
    tokio::time::timeout(WAIT, async {
        loop {
            if let SessionEvent::Acp(AcpEvent::Notification(
                acp::v1::AgentNotification::SessionNotification(n),
            )) = resumed.events.recv().await.unwrap().event
                && let acp::v1::SessionUpdate::AgentMessageChunk(c) = n.update
                && let acp::v1::ContentBlock::Text(t) = c.content
                && t.text.contains(&d.child_id)
                && t.text.contains("held unique result")
            {
                break;
            }
        }
    })
    .await
    .expect("held child result starts resumed parent turn");
    m.shutdown(Duration::ZERO).await;
}
#[tokio::test]
#[serial]
async fn closing_crashed_parent_still_cancels_child() {
    let home = tempfile::tempdir().unwrap();
    let m = manager(3);
    let p = start(&m, home.path(), true);
    status(&m, &p.conversation_id, SessionStatus::Ready).await;
    let d = m
        .delegate(
            &p.conversation_id,
            "mock-model".into(),
            "mock:stream-until-cancel".into(),
        )
        .await
        .unwrap();
    status(&m, &id(&d), SessionStatus::Working).await;
    p.handle
        .prompt(text("mock:exit-during-prompt"))
        .await
        .unwrap();
    status(&m, &p.conversation_id, SessionStatus::Failed).await;
    m.close(&p.conversation_id, Duration::ZERO).await.unwrap();
    assert_eq!(
        terminal(&m, &p.conversation_id, &d.request_id).await.status,
        DelegationStatus::Cancelled
    );
    assert!(m.handle(&id(&d)).is_none());
}
#[tokio::test]
#[serial]
async fn approval_racing_parent_close_cannot_leave_a_running_child() {
    let home = tempfile::tempdir().unwrap();
    let m = manager(3);
    let p = start(&m, home.path(), true);
    status(&m, &p.conversation_id, SessionStatus::Ready).await;
    let d = m
        .delegate(
            &p.conversation_id,
            "mock-model-alt".into(),
            "mock:stream-until-cancel".into(),
        )
        .await
        .unwrap();
    let (_approved, closed) = tokio::join!(
        m.resolve_delegation(&p.conversation_id, &d.request_id, true),
        m.close(&p.conversation_id, Duration::ZERO)
    );
    closed.unwrap();
    assert_eq!(
        terminal(&m, &p.conversation_id, &d.request_id).await.status,
        DelegationStatus::Cancelled
    );
    assert_eq!(m.free_slots(), 3);
}
#[tokio::test]
#[serial]
async fn child_result_waits_in_busy_parent_queue() {
    let delay = Env::set("MOCK_AGENT_DELAY_MS", "1000");
    let home = tempfile::tempdir().unwrap();
    let m = manager(3);
    let mut p = start(&m, home.path(), true);
    status(&m, &p.conversation_id, SessionStatus::Ready).await;
    drop(delay);
    p.handle.prompt(text("finish parent work")).await.unwrap();
    status(&m, &p.conversation_id, SessionStatus::Working).await;
    let mut events = m.subscribe(4096);
    let d = m
        .delegate(
            &p.conversation_id,
            "mock-model".into(),
            "queued child result".into(),
        )
        .await
        .unwrap();
    tokio::time::timeout(WAIT, async {
        loop {
            let e = events.recv().await.unwrap();
            if e.conversation_id == p.conversation_id.to_string()
                && let SessionEvent::Nori(NoriEvent::QueueChanged(q)) = e.event
                && q.prompts.iter().any(|prompt| prompt.contains(&d.child_id))
            {
                break;
            }
        }
    })
    .await
    .expect("result queued in busy parent");
    assert_eq!(
        terminal(&m, &p.conversation_id, &d.request_id).await.status,
        DelegationStatus::Completed
    );
    let reply = wait_parent_reply(&mut p, &d.child_id).await;
    assert!(reply.contains("· completed"), "{reply}");
    m.shutdown(Duration::ZERO).await;
}

#[tokio::test]
#[serial]
async fn only_requesting_parent_can_resolve_and_concurrent_approval_starts_one_child() {
    let home = tempfile::tempdir().unwrap();
    let m = manager(5);
    let p = start(&m, home.path(), true);
    let other = start(&m, home.path(), true);
    status(&m, &p.conversation_id, SessionStatus::Ready).await;
    status(&m, &other.conversation_id, SessionStatus::Ready).await;
    let d = m
        .delegate(
            &p.conversation_id,
            "mock-model-alt".into(),
            "mock:stream-until-cancel".into(),
        )
        .await
        .unwrap();
    assert!(
        m.resolve_delegation(&other.conversation_id, &d.request_id, true)
            .await
            .is_err()
    );
    let (a, b) = tokio::join!(
        m.resolve_delegation(&p.conversation_id, &d.request_id, true),
        m.resolve_delegation(&p.conversation_id, &d.request_id, true)
    );
    assert_ne!(a.is_ok(), b.is_ok());
    assert_eq!(m.list().len(), 3);
    m.shutdown(Duration::ZERO).await;
}
#[tokio::test]
#[serial]
async fn flag_off_agent_cannot_invoke_delegate_over_mcp() {
    let _http = Env::set("MOCK_AGENT_MCP_HTTP", "1");
    let home = tempfile::tempdir().unwrap();
    let m = manager(3);
    let mut p = start(&m, home.path(), false);
    status(&m, &p.conversation_id, SessionStatus::Ready).await;
    let request = p
        .handle
        .prompt(text("mock:delegate mock-model forbidden child"))
        .await
        .unwrap();
    tokio::time::timeout(WAIT, async {
        loop {
            if let SessionEvent::Acp(AcpEvent::Response { request_id, .. }) =
                p.events.recv().await.unwrap().event
                && request_id == request
            {
                break;
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(m.list().len(), 1);
    assert!(m.delegations(&p.conversation_id).is_empty());
    m.shutdown(Duration::ZERO).await;
}

#[tokio::test]
#[serial]
async fn cancelling_child_reports_cancelled_to_parent() {
    let home = tempfile::tempdir().unwrap();
    let m = manager(3);
    let p = start(&m, home.path(), true);
    status(&m, &p.conversation_id, SessionStatus::Ready).await;
    let d = m
        .delegate(
            &p.conversation_id,
            "mock-model".into(),
            "mock:stream-until-cancel".into(),
        )
        .await
        .unwrap();
    status(&m, &id(&d), SessionStatus::Working).await;
    m.handle(&id(&d)).unwrap().cancel().await.unwrap();
    assert_eq!(
        terminal(&m, &p.conversation_id, &d.request_id).await.status,
        DelegationStatus::Cancelled
    );
    m.shutdown(Duration::ZERO).await;
}

#[tokio::test]
#[serial]
async fn forking_a_child_preserves_delegation_depth() {
    let _http = Env::set("MOCK_AGENT_MCP_HTTP", "1");
    let _fork = Env::set("MOCK_AGENT_SUPPORT_SESSION_FORK", "1");
    let home = tempfile::tempdir().unwrap();
    let m = manager(6);
    let p = start(&m, home.path(), true);
    status(&m, &p.conversation_id, SessionStatus::Ready).await;
    let mut events = m.subscribe(4096);
    let child = m
        .delegate(
            &p.conversation_id,
            "mock-model".into(),
            "mock:delegate mock-model-alt pending leaf".into(),
        )
        .await
        .unwrap();
    tokio::time::timeout(WAIT, async {
        loop {
            if !m.delegations(&id(&child)).is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    tokio::time::timeout(WAIT, async {
        loop {
            let e = events.recv().await.unwrap();
            if e.conversation_id == child.child_id
                && matches!(
                    e.event,
                    SessionEvent::Acp(AcpEvent::Response {
                        response: Ok(acp::v1::AgentResponse::PromptResponse(_)),
                        ..
                    })
                )
            {
                break;
            }
        }
    })
    .await
    .unwrap();
    m.handle(&id(&child)).unwrap().branch().await.unwrap();
    let fork = tokio::time::timeout(WAIT, async {
        loop {
            if let Some(fork) = m
                .list()
                .into_iter()
                .find(|r| r.parent == Some(id(&child)) && r.source == SessionSource::Fork)
            {
                return fork.conversation_id;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let leaf = m
        .delegate(
            &fork,
            "mock-model".into(),
            "mock:stream-until-cancel".into(),
        )
        .await
        .unwrap();
    assert_eq!(leaf.depth, 2);
    status(&m, &id(&leaf), SessionStatus::Working).await;
    assert!(
        m.delegate(&id(&leaf), "mock-model".into(), "too deep".into())
            .await
            .unwrap_err()
            .to_string()
            .contains("depth")
    );
    let listed = ManagedRemoteHost::new(m.clone())
        .request(serde_json::json!({"jsonrpc":"2.0","id":1,"method":"_nori/session/list"}))
        .await;
    let rows = listed.response["result"]["sessions"].as_array().unwrap();
    assert_eq!(
        rows.iter()
            .find(|row| row["session_id"] == fork.to_string())
            .unwrap()["depth"],
        1
    );
    assert_eq!(
        rows.iter()
            .find(|row| row["session_id"] == leaf.child_id)
            .unwrap()["depth"],
        2
    );
    m.shutdown(Duration::ZERO).await;
}

#[tokio::test]
#[serial]
async fn fork_migrates_running_children_and_pending_approval_to_new_parent() {
    let _fork = Env::set("MOCK_AGENT_SUPPORT_SESSION_FORK", "1");
    let home = tempfile::tempdir().unwrap();
    let m = manager(5);
    let mut p = start(&m, home.path(), true);
    status(&m, &p.conversation_id, SessionStatus::Ready).await;
    let running = m
        .delegate(
            &p.conversation_id,
            "mock-model".into(),
            "mock:stream-until-cancel".into(),
        )
        .await
        .unwrap();
    let pending = m
        .delegate(
            &p.conversation_id,
            "mock-model-alt".into(),
            "mock:stream-until-cancel".into(),
        )
        .await
        .unwrap();
    p.handle.branch().await.unwrap();
    let fork = tokio::time::timeout(WAIT, async {
        loop {
            let e = p.events.recv().await.unwrap();
            if let SessionEvent::Nori(NoriEvent::SessionForked(f)) = e.event {
                return ConversationId::from_string(&f.new_conversation_id).unwrap();
            }
        }
    })
    .await
    .unwrap();
    status(&m, &fork, SessionStatus::WaitingOnYou).await;
    assert_eq!(m.get(&id(&running)).unwrap().parent, Some(fork));
    let approved = m
        .resolve_delegation(&fork, &pending.request_id, true)
        .await
        .unwrap();
    assert_eq!(approved.parent_id, fork.to_string());
    m.close(&fork, Duration::ZERO).await.unwrap();
    assert_eq!(m.free_slots(), 5);
    assert_eq!(
        terminal(&m, &fork, &running.request_id).await.status,
        DelegationStatus::Cancelled
    );
    assert_eq!(
        terminal(&m, &fork, &pending.request_id).await.status,
        DelegationStatus::Cancelled
    );
}

#[cfg(unix)]
#[tokio::test]
#[serial]
async fn result_queued_before_parent_crash_is_redelivered_after_resume() {
    let _reply = Env::set("MOCK_AGENT_RESPONSE", "durable child payload");
    let delay = Env::set("MOCK_AGENT_DELAY_MS", "5000");
    let home = tempfile::tempdir().unwrap();
    let m = manager(3);
    let mut parent_spec = spec(home.path(), true);
    Arc::make_mut(&mut parent_spec.config).acp_proxy = nori_config::AcpProxyConfig {
        enabled: true,
        log_dir: home.path().join("parent-wire"),
    };
    let p = m
        .start(StartSession {
            agent: SessionAgent::Prepare(parent_spec),
            start: SessionStart::New,
            parent: None,
            source: SessionSource::User,
        })
        .unwrap();
    status(&m, &p.conversation_id, SessionStatus::Ready).await;
    drop(delay);
    p.handle
        .prompt(text("parent interrupted work"))
        .await
        .unwrap();
    status(&m, &p.conversation_id, SessionStatus::Working).await;
    let mut events = m.subscribe(4096);
    let child = m
        .delegate(
            &p.conversation_id,
            "mock-model".into(),
            "durable task".into(),
        )
        .await
        .unwrap();
    tokio::time::timeout(WAIT, async {
        loop {
            let e = events.recv().await.unwrap();
            if e.conversation_id == p.conversation_id.to_string()
                && let SessionEvent::Nori(NoriEvent::QueueChanged(q)) = e.event
                && q.prompts.iter().any(|text| text.contains(&child.child_id))
            {
                break;
            }
        }
    })
    .await
    .unwrap();
    let pid = std::fs::read_dir(home.path().join("parent-wire"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter_map(|path| {
            let name = path.file_name()?.to_str()?;
            let pid = name.split('-').nth(1)?.parse::<i32>().ok()?;
            Some((path, pid))
        })
        .find(|(path, _)| {
            std::fs::read_to_string(path)
                .unwrap_or_default()
                .contains("parent interrupted work")
        })
        .unwrap()
        .1;
    assert_eq!(unsafe { libc::kill(pid, libc::SIGKILL) }, 0);
    status(&m, &p.conversation_id, SessionStatus::Failed).await;
    let _echo = Env::set("MOCK_AGENT_ECHO_PROMPT", "1");
    let mut resumed = m
        .resume(&p.conversation_id, spec(home.path(), true))
        .await
        .unwrap();
    tokio::time::timeout(WAIT, async {
        loop {
            if let SessionEvent::Acp(AcpEvent::Notification(
                acp::v1::AgentNotification::SessionNotification(n),
            )) = resumed.events.recv().await.unwrap().event
                && let acp::v1::SessionUpdate::AgentMessageChunk(c) = n.update
                && let acp::v1::ContentBlock::Text(t) = c.content
                && t.text.contains(&child.child_id)
                && t.text.contains("durable child payload")
            {
                break;
            }
        }
    })
    .await
    .expect("queued result survives parent crash");
    m.shutdown(Duration::ZERO).await;
}
