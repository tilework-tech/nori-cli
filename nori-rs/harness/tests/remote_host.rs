//! Behavior of [`HarnessRemoteHost`] over a real harness session backed by
//! the mock ACP agent: stable outward session ids, remote prompt turns in
//! stream order, transcript-backed `session/load` replay, and delegated
//! permission routing for remote-owned turns.

use std::sync::Arc;
use std::time::Duration;

use nori_config::NoriConfig;
use nori_harness::remote_agent::HarnessRemoteHost;
use nori_harness::remote_agent::HostedAgent;
use nori_harness::runtime::LaunchedSession;
use nori_harness::runtime::SessionLaunchSpec;
use nori_harness::runtime::launch_session;
use nori_protocol::AcpEvent;
use nori_protocol::NoriEvent;
use nori_protocol::SessionEvent;
use nori_protocol::acp::v1 as acp;
use pretty_assertions::assert_eq;
use serial_test::serial;

struct EnvGuard(&'static str);

impl Drop for EnvGuard {
    fn drop(&mut self) {
        // SAFETY: tests that mutate the mock-agent environment run serially.
        unsafe { std::env::remove_var(self.0) };
    }
}

struct RemoteFixture {
    host: Arc<HarnessRemoteHost>,
    session: LaunchedSession,
    _temp: tempfile::TempDir,
}

#[expect(
    clippy::expect_used,
    reason = "fixture failures should fail the test loudly"
)]
async fn launch_attached() -> RemoteFixture {
    let temp = tempfile::tempdir().expect("create session directory");
    let config = NoriConfig {
        active_agent: "mock-model".to_string(),
        cwd: temp.path().to_path_buf(),
        nori_home: temp.path().to_path_buf(),
        ..Default::default()
    };
    let session = launch_session(SessionLaunchSpec {
        config: Arc::new(config),
        cli_version: "remote-host-test".to_string(),
        session_context: None,
        initial_context: None,
        resume: None,
    });
    let host = Arc::new(HarnessRemoteHost::new());
    host.attach(session.handle.clone(), temp.path().to_path_buf())
        .await
        .expect("attach remote host");
    RemoteFixture {
        host,
        session,
        _temp: temp,
    }
}

/// Poll until the host exposes the started session, returning its outward id.
#[expect(
    clippy::expect_used,
    reason = "fixture failures should fail the test loudly"
)]
async fn wait_for_session(host: &HarnessRemoteHost) -> acp::SessionInfo {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let sessions = host.list_sessions().await.expect("list sessions");
        if let Some(info) = sessions.into_iter().next() {
            return info;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "session never became visible to the remote host"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn notification_text(event: &SessionEvent) -> Option<(String, String)> {
    let SessionEvent::Acp(AcpEvent::Notification(acp::AgentNotification::SessionNotification(
        notification,
    ))) = event
    else {
        return None;
    };
    let (acp::SessionUpdate::AgentMessageChunk(chunk)
    | acp::SessionUpdate::UserMessageChunk(chunk)) = &notification.update
    else {
        return None;
    };
    let acp::ContentBlock::Text(text) = &chunk.content else {
        return None;
    };
    Some((notification.session_id.to_string(), text.text.clone()))
}

#[tokio::test]
#[serial]
async fn remote_prompt_streams_rewritten_updates_then_the_response() {
    let fixture = launch_attached().await;
    let info = wait_for_session(&fixture.host).await;
    let session_id = info.session_id.clone();

    let mut subscription = fixture.host.subscribe().await;
    let request_id = fixture
        .host
        .prompt(
            &session_id,
            vec![acp::ContentBlock::Text(acp::TextContent::new("hello"))],
        )
        .await
        .expect("submit remote prompt");

    // The downstream mock streams two chunks, then the response; everything
    // must arrive on the subscription in that order, with outward ids.
    let mut texts = Vec::new();
    let response = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let event = subscription
                .events
                .recv()
                .await
                .expect("subscription closed before the prompt completed");
            if let Some((update_session_id, text)) = notification_text(&event) {
                assert_eq!(update_session_id, session_id.to_string());
                texts.push(text);
                continue;
            }
            if let SessionEvent::Acp(AcpEvent::Response {
                request_id: response_id,
                response,
            }) = event
            {
                assert_eq!(response_id, request_id);
                return response;
            }
        }
    })
    .await
    .expect("prompt should complete");

    assert_eq!(
        texts,
        vec!["Test message 1".to_string(), "Test message 2".to_string()]
    );
    match response.expect("prompt should succeed") {
        acp::AgentResponse::PromptResponse(response) => {
            assert_eq!(response.stop_reason, acp::StopReason::EndTurn);
        }
        other => panic!("expected a prompt response, got {other:?}"),
    }

    fixture.session.handle.shutdown().await.expect("shutdown");
}

#[tokio::test]
#[serial]
async fn load_session_replays_the_previous_turn_from_the_transcript() {
    let fixture = launch_attached().await;
    let info = wait_for_session(&fixture.host).await;
    let session_id = info.session_id.clone();

    let mut subscription = fixture.host.subscribe().await;
    fixture
        .host
        .prompt(
            &session_id,
            vec![acp::ContentBlock::Text(acp::TextContent::new(
                "remember this turn",
            ))],
        )
        .await
        .expect("submit remote prompt");
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let event = subscription
                .events
                .recv()
                .await
                .expect("subscription closed before the prompt completed");
            if matches!(event, SessionEvent::Acp(AcpEvent::Response { .. })) {
                break;
            }
        }
    })
    .await
    .expect("prompt should complete");

    let loaded = fixture
        .host
        .load_session(&session_id)
        .await
        .expect("load session history");
    let texts: Vec<String> = loaded
        .replay
        .iter()
        .map(|notification| {
            assert_eq!(notification.session_id, session_id);
            match &notification.update {
                acp::SessionUpdate::UserMessageChunk(chunk)
                | acp::SessionUpdate::AgentMessageChunk(chunk) => match &chunk.content {
                    acp::ContentBlock::Text(text) => text.text.clone(),
                    other => panic!("unexpected replay content: {other:?}"),
                },
                other => panic!("unexpected replay update: {other:?}"),
            }
        })
        .collect();
    assert_eq!(
        texts,
        vec![
            "remember this turn".to_string(),
            "Test message 1".to_string(),
            "Test message 2".to_string(),
        ]
    );

    fixture.session.handle.shutdown().await.expect("shutdown");
}

#[tokio::test]
#[serial]
async fn unknown_session_ids_are_rejected() {
    let fixture = launch_attached().await;
    wait_for_session(&fixture.host).await;

    let bogus = acp::SessionId::new("00000000-0000-0000-0000-000000000000");
    let error = fixture
        .host
        .load_session(&bogus)
        .await
        .expect_err("bogus load must fail");
    assert_eq!(i32::from(error.code), -32002);
    let error = fixture
        .host
        .prompt(&bogus, vec![])
        .await
        .expect_err("bogus prompt must fail");
    assert_eq!(i32::from(error.code), -32002);

    fixture.session.handle.shutdown().await.expect("shutdown");
}

#[tokio::test]
#[serial]
async fn outward_session_id_is_the_conversation_id_not_the_downstream_id() {
    let mut fixture = launch_attached().await;
    let info = wait_for_session(&fixture.host).await;

    let started = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let Some(SessionEvent::Nori(NoriEvent::SessionStarted(started))) =
                fixture.session.events.recv().await
            {
                return started;
            }
        }
    })
    .await
    .expect("primary stream should see SessionStarted");

    let transcript_id = started.transcript_id.expect("transcript id");
    assert_eq!(info.session_id.to_string(), transcript_id);
    assert_ne!(
        info.session_id.to_string(),
        started.acp_session_id.to_string()
    );

    fixture.session.handle.shutdown().await.expect("shutdown");
}

#[tokio::test]
#[serial]
async fn delegated_permission_requests_reach_the_remote_controller_for_remote_turns() {
    // SAFETY: tests that mutate the mock-agent environment run serially.
    unsafe { std::env::set_var("MOCK_AGENT_REQUEST_PERMISSION", "1") };
    let _guard = EnvGuard("MOCK_AGENT_REQUEST_PERMISSION");

    let fixture = launch_attached().await;
    let info = wait_for_session(&fixture.host).await;
    let session_id = info.session_id.clone();

    let mut subscription = fixture.host.subscribe().await;
    let request_id = fixture
        .host
        .prompt(
            &session_id,
            vec![acp::ContentBlock::Text(acp::TextContent::new(
                "do a tool call",
            ))],
        )
        .await
        .expect("submit remote prompt");

    let outcome = tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let event = subscription
                .events
                .recv()
                .await
                .expect("subscription closed before the permission request");
            match event {
                SessionEvent::Acp(AcpEvent::Request {
                    request_id: delegated_id,
                    request: acp::AgentRequest::RequestPermissionRequest(request),
                }) => {
                    let allow = request
                        .options
                        .iter()
                        .find(|option| matches!(option.kind, acp::PermissionOptionKind::AllowOnce))
                        .expect("an allow option");
                    let response = acp::RequestPermissionResponse::new(
                        acp::RequestPermissionOutcome::Selected(
                            acp::SelectedPermissionOutcome::new(allow.option_id.clone()),
                        ),
                    );
                    fixture
                        .host
                        .respond(
                            delegated_id,
                            Ok(acp::ClientResponse::RequestPermissionResponse(response)),
                        )
                        .await
                        .expect("answer the delegated request");
                }
                SessionEvent::Acp(AcpEvent::Response {
                    request_id: response_id,
                    response,
                }) => {
                    assert_eq!(response_id, request_id);
                    return response;
                }
                _ => {}
            }
        }
    })
    .await
    .expect("permission round-trip should complete the turn");

    assert!(outcome.is_ok(), "turn failed: {outcome:?}");
    fixture.session.handle.shutdown().await.expect("shutdown");
}
