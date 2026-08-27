//! Behavior of [`HarnessRemoteHost`] over a real harness session backed by
//! the mock ACP agent: stable outward session ids, remote prompt turns in
//! stream order, transcript-backed `session/load` replay, and delegated
//! permission routing for remote-owned turns.

use std::future::Future;
use std::sync::Arc;
use std::task::Poll;
use std::time::Duration;

use nori_config::NoriConfig;
use nori_harness::remote_agent::HarnessRemoteHost;
use nori_harness::remote_agent::HostedAgent;
use nori_harness::runtime::AgentPrepareSpec;
use nori_harness::runtime::LaunchedSession;
use nori_harness::runtime::SessionLaunchSpec;
use nori_harness::runtime::SessionStart;
use nori_harness::runtime::launch_session;
use nori_harness::runtime::prepare_agent;
use nori_protocol::AcpEvent;
use nori_protocol::NoriEvent;
use nori_protocol::SessionEvent;
use nori_protocol::acp::v1 as acp;
use pretty_assertions::assert_eq;
use serial_test::serial;

const SESSION_BUSY_ERROR_CODE: i32 = -32015;

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
    let agent = prepare_agent(AgentPrepareSpec {
        config: Arc::new(config),
        cli_version: "remote-host-test".to_string(),
        session_context: None,
        initial_context: None,
    })
    .await
    .expect("prepare mock agent");
    let session = launch_session(SessionLaunchSpec {
        agent,
        start: SessionStart::New,
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

fn notification_nori_status(event: &SessionEvent) -> Option<(String, String)> {
    let SessionEvent::Acp(AcpEvent::Notification(acp::AgentNotification::SessionNotification(
        notification,
    ))) = event
    else {
        return None;
    };
    let acp::SessionUpdate::SessionInfoUpdate(info) = &notification.update else {
        return None;
    };
    let status = info.meta.as_ref()?.get("nori")?.get("status")?.as_str()?;
    Some((notification.session_id.to_string(), status.to_string()))
}

#[tokio::test]
#[serial]
async fn local_prompt_broadcasts_a_complete_observer_turn_without_a_foreign_response() {
    let fixture = launch_attached().await;
    let info = wait_for_session(&fixture.host).await;
    let session_id = info.session_id.clone();
    let mut subscription = fixture.host.subscribe().await;

    fixture
        .session
        .handle
        .prompt(vec![acp::ContentBlock::Text(acp::TextContent::new(
            "local turn",
        ))])
        .await
        .expect("submit local prompt");

    let observed = tokio::time::timeout(Duration::from_secs(10), async {
        let mut sequence = Vec::new();
        loop {
            let event = subscription
                .events
                .recv()
                .await
                .ok_or_else(|| "subscription closed before observer turn ended".to_string())?;
            if let Some((update_session_id, status)) = notification_nori_status(&event) {
                assert_eq!(update_session_id, session_id.to_string());
                sequence.push(format!("status:{status}"));
                if status == "idle" {
                    return Ok::<_, String>(sequence);
                }
                continue;
            }
            if let Some((update_session_id, text)) = notification_text(&event) {
                assert_eq!(update_session_id, session_id.to_string());
                sequence.push(format!("message:{text}"));
                continue;
            }
            if matches!(event, SessionEvent::Acp(AcpEvent::Response { .. })) {
                return Err(
                    "observer received a response for a prompt it did not issue".to_string()
                );
            }
        }
    })
    .await;

    fixture.session.handle.shutdown().await.expect("shutdown");
    let sequence = observed
        .expect("observer turn should end")
        .expect("observer stream should remain valid");
    assert_eq!(
        sequence,
        vec![
            "status:working".to_string(),
            "message:local turn".to_string(),
            "message:Test message 1".to_string(),
            "message:Test message 2".to_string(),
            "status:idle".to_string(),
        ]
    );
}

#[tokio::test]
#[serial]
async fn local_prompt_cancellation_still_ends_the_observer_turn() {
    // SAFETY: tests that mutate the mock-agent environment run serially.
    unsafe { std::env::set_var("MOCK_AGENT_STREAM_UNTIL_CANCEL", "1") };
    let _guard = EnvGuard("MOCK_AGENT_STREAM_UNTIL_CANCEL");

    let fixture = launch_attached().await;
    let info = wait_for_session(&fixture.host).await;
    let mut subscription = fixture.host.subscribe().await;

    fixture
        .session
        .handle
        .prompt(vec![acp::ContentBlock::Text(acp::TextContent::new(
            "cancel locally",
        ))])
        .await
        .expect("submit local prompt");
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let event = subscription
                .events
                .recv()
                .await
                .expect("subscription closed before local output");
            if notification_text(&event).is_some_and(|(_, text)| text == "Streaming...") {
                break;
            }
        }
    })
    .await
    .expect("local output should start");
    fixture
        .session
        .handle
        .cancel()
        .await
        .expect("cancel locally");

    let statuses = tokio::time::timeout(Duration::from_secs(5), async {
        let mut statuses = Vec::new();
        loop {
            let event = subscription
                .events
                .recv()
                .await
                .expect("subscription closed before cancellation completed");
            if let Some((update_session_id, status)) = notification_nori_status(&event) {
                assert_eq!(update_session_id, info.session_id.to_string());
                statuses.push(status.clone());
                if status == "idle" {
                    return statuses;
                }
            }
        }
    })
    .await;

    fixture.session.handle.shutdown().await.expect("shutdown");
    assert_eq!(
        statuses.expect("cancelled observer turn should end"),
        vec!["idle".to_string()]
    );
}

#[tokio::test]
#[serial]
async fn local_prompt_failure_still_ends_the_observer_turn() {
    // SAFETY: tests that mutate the mock-agent environment run serially.
    unsafe { std::env::set_var("MOCK_AGENT_PROMPT_FAIL", "1") };
    let _guard = EnvGuard("MOCK_AGENT_PROMPT_FAIL");

    let fixture = launch_attached().await;
    let info = wait_for_session(&fixture.host).await;
    let mut subscription = fixture.host.subscribe().await;

    fixture
        .session
        .handle
        .prompt(vec![acp::ContentBlock::Text(acp::TextContent::new(
            "fail locally",
        ))])
        .await
        .expect("submit local prompt");

    let statuses = tokio::time::timeout(Duration::from_secs(5), async {
        let mut statuses = Vec::new();
        loop {
            let event = subscription
                .events
                .recv()
                .await
                .expect("subscription closed before failure completed");
            if let Some((update_session_id, status)) = notification_nori_status(&event) {
                assert_eq!(update_session_id, info.session_id.to_string());
                statuses.push(status.clone());
                if status == "idle" {
                    return statuses;
                }
            }
        }
    })
    .await;

    fixture.session.handle.shutdown().await.expect("shutdown");
    assert_eq!(
        statuses.expect("failed observer turn should end"),
        vec!["working".to_string(), "idle".to_string()]
    );
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
            None,
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
        vec![
            "hello".to_string(),
            "Test message 1".to_string(),
            "Test message 2".to_string(),
        ]
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
async fn remote_host_rewrites_and_forwards_every_original_user_content_block() {
    let mut fixture = launch_attached().await;
    let info = wait_for_session(&fixture.host).await;
    let session_id = info.session_id.clone();
    let prompt = vec![
        acp::ContentBlock::Text(acp::TextContent::new("inspect this")),
        acp::ContentBlock::Image(acp::ImageContent::new("aW1hZ2U=", "image/png")),
        acp::ContentBlock::ResourceLink(acp::ResourceLink::new("notes", "file:///tmp/notes.md")),
    ];
    let prompt_meta = acp::Meta::from_iter([(
        nori_protocol::PROMPT_ECHO_ID_META_KEY.to_string(),
        serde_json::Value::String("outer-prompt".to_string()),
    )]);

    let mut subscription = fixture.host.subscribe().await;
    let request_id = fixture
        .host
        .prompt(&session_id, prompt.clone(), Some(prompt_meta))
        .await
        .expect("submit remote prompt");
    let remote_notifications = tokio::time::timeout(Duration::from_secs(10), async {
        let mut notifications = Vec::new();
        loop {
            let event = subscription
                .events
                .recv()
                .await
                .expect("subscription closed before the prompt completed");
            match event {
                SessionEvent::Acp(AcpEvent::Notification(
                    acp::AgentNotification::SessionNotification(notification),
                )) => {
                    assert_eq!(notification.session_id, session_id);
                    if matches!(notification.update, acp::SessionUpdate::UserMessageChunk(_)) {
                        notifications.push(notification);
                    }
                }
                SessionEvent::Acp(AcpEvent::Response {
                    request_id: response_id,
                    ..
                }) if response_id == request_id => break notifications,
                SessionEvent::Acp(_) | SessionEvent::Nori(_) => {}
            }
        }
    })
    .await
    .expect("prompt should complete");

    let local_notifications = tokio::time::timeout(Duration::from_secs(10), async {
        let mut notifications = Vec::new();
        loop {
            let event = fixture
                .session
                .events
                .recv()
                .await
                .expect("primary event stream closed before the prompt completed");
            match event {
                SessionEvent::Acp(AcpEvent::Notification(
                    acp::AgentNotification::SessionNotification(notification),
                )) if matches!(notification.update, acp::SessionUpdate::UserMessageChunk(_)) => {
                    notifications.push(notification)
                }
                SessionEvent::Acp(AcpEvent::Response {
                    request_id: response_id,
                    ..
                }) if response_id == request_id => break notifications,
                SessionEvent::Acp(_) | SessionEvent::Nori(_) => {}
            }
        }
    })
    .await
    .expect("primary prompt stream should complete");

    assert_eq!(
        remote_notifications
            .iter()
            .map(|notification| match &notification.update {
                acp::SessionUpdate::UserMessageChunk(chunk) => chunk.content.clone(),
                _ => unreachable!("collected only user message notifications"),
            })
            .collect::<Vec<_>>(),
        prompt
    );
    let message_id = remote_notifications
        .first()
        .and_then(|notification| match &notification.update {
            acp::SessionUpdate::UserMessageChunk(chunk) => chunk.message_id.clone(),
            _ => None,
        })
        .expect("broadcast chunks must have a message id");
    assert!(remote_notifications.iter().all(|notification| matches!(
        &notification.update,
        acp::SessionUpdate::UserMessageChunk(chunk)
            if chunk.message_id.as_ref() == Some(&message_id)
                && chunk.meta.as_ref().and_then(|meta| {
                    meta.get(nori_protocol::PROMPT_ECHO_ID_META_KEY)
                }) == Some(&serde_json::Value::String("outer-prompt".to_string()))
    )));
    assert_eq!(local_notifications.len(), remote_notifications.len());
    for (local, remote) in local_notifications.iter().zip(&remote_notifications) {
        assert_ne!(local.session_id, remote.session_id);
        assert_eq!(remote.session_id, session_id);
        let mut expected_remote = local.clone();
        expected_remote.session_id = session_id.clone();
        assert_eq!(&expected_remote, remote);
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
            None,
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
        .filter_map(|notification| {
            assert_eq!(notification.session_id, session_id);
            match &notification.update {
                acp::SessionUpdate::UserMessageChunk(chunk)
                | acp::SessionUpdate::AgentMessageChunk(chunk) => match &chunk.content {
                    acp::ContentBlock::Text(text) => Some(text.text.clone()),
                    other => panic!("unexpected replay content: {other:?}"),
                },
                acp::SessionUpdate::SessionInfoUpdate(_) => None,
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
        .prompt(&bogus, vec![], None)
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
            None,
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

#[tokio::test]
#[serial]
async fn remote_prompt_is_rejected_while_a_local_turn_is_active() {
    // SAFETY: tests that mutate the mock-agent environment run serially.
    unsafe { std::env::set_var("MOCK_AGENT_DELAY_MS", "3000") };
    let _guard = EnvGuard("MOCK_AGENT_DELAY_MS");

    let mut fixture = launch_attached().await;
    let info = wait_for_session(&fixture.host).await;
    let session_id = info.session_id.clone();
    let mut subscription = fixture.host.subscribe().await;
    let local_request_id = fixture
        .session
        .handle
        .prompt(vec![acp::ContentBlock::Text(acp::TextContent::new(
            "first",
        ))])
        .await
        .expect("submit local prompt");

    let rejected = tokio::time::timeout(
        Duration::from_secs(2),
        fixture.host.prompt(
            &session_id,
            vec![acp::ContentBlock::Text(acp::TextContent::new("remote"))],
            None,
        ),
    )
    .await;

    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let event = fixture
                .session
                .events
                .recv()
                .await
                .expect("primary event stream closed before local turn ended");
            if matches!(
                event,
                SessionEvent::Acp(AcpEvent::Response { request_id, .. })
                    if request_id == local_request_id
            ) {
                break;
            }
        }
    })
    .await
    .expect("local turn should end");
    tokio::time::sleep(Duration::from_millis(200)).await;
    let rejected_prompt_was_later_broadcast =
        std::iter::from_fn(|| subscription.events.try_recv().ok())
            .filter_map(|event| notification_text(&event))
            .any(|(_, text)| text == "remote");

    fixture.session.handle.shutdown().await.expect("shutdown");
    let error = rejected
        .expect("remote prompt rejection should be immediate")
        .expect_err("remote prompt must not queue behind local activity");
    assert_eq!(i32::from(error.code), SESSION_BUSY_ERROR_CODE);
    assert!(
        !rejected_prompt_was_later_broadcast,
        "a rejected remote prompt must never execute later"
    );
}

#[tokio::test]
#[serial]
async fn remote_cancel_does_not_cancel_a_local_turn() {
    // SAFETY: tests that mutate the mock-agent environment run serially.
    unsafe { std::env::set_var("MOCK_AGENT_STREAM_UNTIL_CANCEL", "1") };
    let _guard = EnvGuard("MOCK_AGENT_STREAM_UNTIL_CANCEL");

    let fixture = launch_attached().await;
    let info = wait_for_session(&fixture.host).await;
    let session_id = info.session_id.clone();
    let mut subscription = fixture.host.subscribe().await;

    fixture
        .session
        .handle
        .prompt(vec![acp::ContentBlock::Text(acp::TextContent::new(
            "local streaming turn",
        ))])
        .await
        .expect("submit local prompt");
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let event = subscription
                .events
                .recv()
                .await
                .expect("subscription closed before local output");
            if notification_text(&event).is_some_and(|(_, text)| text == "Streaming...") {
                break;
            }
        }
    })
    .await
    .expect("local output should start");
    fixture
        .host
        .cancel(&session_id)
        .await
        .expect("ignore remote cancel for local turn");

    tokio::time::sleep(Duration::from_millis(100)).await;
    while subscription.events.try_recv().is_ok() {}
    let output_continued = tokio::time::timeout(Duration::from_millis(500), async {
        loop {
            let event = subscription
                .events
                .recv()
                .await
                .expect("subscription closed while local turn was active");
            if notification_text(&event).is_some_and(|(_, text)| text == "Streaming...") {
                return true;
            }
        }
    })
    .await
    .unwrap_or(false);

    fixture
        .session
        .handle
        .cancel()
        .await
        .expect("cancel locally");
    fixture.session.handle.shutdown().await.expect("shutdown");
    assert!(
        output_continued,
        "local-owned agent output must continue after a remote cancel"
    );
}

#[tokio::test]
#[serial]
async fn remote_cancel_still_cancels_a_remote_owned_turn() {
    // SAFETY: tests that mutate the mock-agent environment run serially.
    unsafe { std::env::set_var("MOCK_AGENT_STREAM_UNTIL_CANCEL", "1") };
    let _guard = EnvGuard("MOCK_AGENT_STREAM_UNTIL_CANCEL");

    let fixture = launch_attached().await;
    let info = wait_for_session(&fixture.host).await;
    let session_id = info.session_id.clone();
    let mut subscription = fixture.host.subscribe().await;
    let request_id = fixture
        .host
        .prompt(
            &session_id,
            vec![acp::ContentBlock::Text(acp::TextContent::new(
                "remote streaming turn",
            ))],
            None,
        )
        .await
        .expect("submit remote prompt");
    fixture
        .host
        .cancel(&session_id)
        .await
        .expect("cancel immediately after remote prompt admission");
    let response = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let event = subscription
                .events
                .recv()
                .await
                .expect("subscription closed before remote cancellation completed");
            if let SessionEvent::Acp(AcpEvent::Response {
                request_id: response_id,
                response,
            }) = event
                && response_id == request_id
            {
                return response;
            }
        }
    })
    .await
    .expect("remote cancellation should complete");

    fixture.session.handle.shutdown().await.expect("shutdown");
    match response.expect("remote cancellation should be a successful ACP response") {
        acp::AgentResponse::PromptResponse(response) => {
            assert_eq!(response.stop_reason, acp::StopReason::Cancelled);
        }
        other => panic!("expected prompt response, got {other:?}"),
    }
}

#[tokio::test]
#[serial]
async fn immediate_remote_cancel_waits_for_remote_turn_registration() {
    // SAFETY: tests that mutate the mock-agent environment run serially.
    unsafe { std::env::set_var("MOCK_AGENT_STREAM_UNTIL_CANCEL", "1") };
    let _guard = EnvGuard("MOCK_AGENT_STREAM_UNTIL_CANCEL");

    let fixture = launch_attached().await;
    let info = wait_for_session(&fixture.host).await;
    let session_id = info.session_id.clone();
    let mut subscription = fixture.host.subscribe().await;
    let mut prompt = Box::pin(fixture.host.prompt(
        &session_id,
        vec![acp::ContentBlock::Text(acp::TextContent::new(
            "cancel before registration",
        ))],
        None,
    ));

    let completed_on_first_poll = std::future::poll_fn(|cx| {
        Poll::Ready(match prompt.as_mut().poll(cx) {
            Poll::Ready(result) => Some(result),
            Poll::Pending => None,
        })
    })
    .await;
    assert!(
        completed_on_first_poll.is_none(),
        "prompt should still be awaiting transport request registration"
    );
    fixture
        .host
        .cancel(&session_id)
        .await
        .expect("accept immediate remote cancel");
    let request_id = tokio::time::timeout(Duration::from_secs(5), &mut prompt)
        .await
        .expect("remote prompt should finish registration")
        .expect("submit remote prompt");

    let response = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let event = subscription
                .events
                .recv()
                .await
                .expect("subscription closed before immediate cancellation completed");
            if let SessionEvent::Acp(AcpEvent::Response {
                request_id: response_id,
                response,
            }) = event
                && response_id == request_id
            {
                return response;
            }
        }
    })
    .await
    .expect("immediate remote cancellation should complete");

    fixture.session.handle.shutdown().await.expect("shutdown");
    match response.expect("remote cancellation should be a successful ACP response") {
        acp::AgentResponse::PromptResponse(response) => {
            assert_eq!(response.stop_reason, acp::StopReason::Cancelled);
        }
        other => panic!("expected prompt response, got {other:?}"),
    }
}

#[tokio::test]
#[serial]
async fn dropped_prompt_registration_does_not_wedge_a_reconnected_controller() {
    // SAFETY: tests that mutate the mock-agent environment run serially.
    unsafe { std::env::set_var("MOCK_AGENT_STREAM_UNTIL_CANCEL", "1") };
    let _guard = EnvGuard("MOCK_AGENT_STREAM_UNTIL_CANCEL");

    let fixture = launch_attached().await;
    let info = wait_for_session(&fixture.host).await;
    let session_id = info.session_id.clone();
    let _first_controller = fixture.host.subscribe().await;
    let mut abandoned_prompt = Box::pin(fixture.host.prompt(
        &session_id,
        vec![acp::ContentBlock::Text(acp::TextContent::new(
            "disconnect during registration",
        ))],
        None,
    ));
    let completed_on_first_poll = std::future::poll_fn(|cx| {
        Poll::Ready(match abandoned_prompt.as_mut().poll(cx) {
            Poll::Ready(result) => Some(result),
            Poll::Pending => None,
        })
    })
    .await;
    assert!(completed_on_first_poll.is_none());
    drop(abandoned_prompt);

    let mut replacement = fixture.host.subscribe().await;
    fixture
        .host
        .cancel(&session_id)
        .await
        .expect("remember cancel across disconnected prompt registration");
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let event = replacement
                .events
                .recv()
                .await
                .expect("replacement subscription closed before cancellation");
            if matches!(
                event,
                SessionEvent::Acp(AcpEvent::Response {
                    response: Ok(acp::AgentResponse::PromptResponse(acp::PromptResponse {
                        stop_reason: acp::StopReason::Cancelled,
                        ..
                    })),
                    ..
                })
            ) {
                break;
            }
        }
    })
    .await
    .expect("abandoned registration should still complete and consume its pending cancel");

    let request_id = tokio::time::timeout(
        Duration::from_secs(5),
        fixture.host.prompt(
            &session_id,
            vec![acp::ContentBlock::Text(acp::TextContent::new(
                "replacement controller turn",
            ))],
            None,
        ),
    )
    .await
    .expect("replacement prompt admission should not wedge")
    .expect("replacement controller should not remain busy");
    fixture
        .host
        .cancel(&session_id)
        .await
        .expect("cancel replacement turn");
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let event = replacement
                .events
                .recv()
                .await
                .expect("replacement subscription closed before its response");
            if matches!(
                event,
                SessionEvent::Acp(AcpEvent::Response {
                    request_id: response_id,
                    response: Ok(acp::AgentResponse::PromptResponse(_)),
                }) if response_id == request_id
            ) {
                break;
            }
        }
    })
    .await
    .expect("replacement controller turn should complete");

    fixture.session.handle.shutdown().await.expect("shutdown");
}

#[tokio::test]
#[serial]
async fn a_stale_detach_does_not_break_the_replacement_subscription() {
    let fixture = launch_attached().await;
    let info = wait_for_session(&fixture.host).await;
    let session_id = info.session_id.clone();

    let mut first = fixture.host.subscribe().await;
    let mut second = fixture.host.subscribe().await;
    assert_ne!(first.id, second.id);

    // Last connect wins: the replaced subscription's receiver closes.
    let closed = tokio::time::timeout(Duration::from_secs(5), first.events.recv())
        .await
        .expect("replaced receiver should close");
    assert!(closed.is_none());

    // The replaced connection detaches with its stale id; the replacement
    // must keep receiving events.
    fixture.host.detach(first.id).await;
    fixture
        .host
        .prompt(
            &session_id,
            vec![acp::ContentBlock::Text(acp::TextContent::new("hello"))],
            None,
        )
        .await
        .expect("submit remote prompt");
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let event = second
                .events
                .recv()
                .await
                .expect("replacement subscription must stay attached");
            if matches!(event, SessionEvent::Acp(AcpEvent::Response { .. })) {
                break;
            }
        }
    })
    .await
    .expect("the replacement subscription should see the turn");

    fixture.session.handle.shutdown().await.expect("shutdown");
}

/// A switch candidate is already started when the TUI commits it. Attaching
/// at that boundary must seed the outward identity from the observed
/// `SessionStarted` instead of requiring the host to have replaced the current
/// session during hidden candidate startup.
#[tokio::test]
#[serial]
async fn attaching_started_replacement_preserves_current_until_commit() {
    let current = launch_attached().await;
    let current_info = wait_for_session(&current.host).await;

    let replacement_home = tempfile::tempdir().expect("replacement directory");
    let config = NoriConfig {
        active_agent: "mock-model".to_string(),
        cwd: replacement_home.path().to_path_buf(),
        nori_home: replacement_home.path().to_path_buf(),
        ..Default::default()
    };
    let agent = prepare_agent(AgentPrepareSpec {
        config: Arc::new(config),
        cli_version: "remote-host-replacement-test".to_string(),
        session_context: None,
        initial_context: None,
    })
    .await
    .expect("prepare replacement");
    let mut replacement = launch_session(SessionLaunchSpec {
        agent,
        start: SessionStart::New,
    });
    let started = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let Some(SessionEvent::Nori(NoriEvent::SessionStarted(started))) =
                replacement.events.recv().await
            {
                return started;
            }
        }
    })
    .await
    .expect("replacement should start");

    assert_eq!(
        current.host.list_sessions().await.expect("list current"),
        vec![current_info],
        "an uncommitted replacement must not disturb the current remote session"
    );

    current
        .host
        .attach_started(
            replacement.handle.clone(),
            replacement_home.path().to_path_buf(),
            started.clone(),
        )
        .await
        .expect("commit replacement attachment");

    let replacement_info = wait_for_session(&current.host).await;
    assert_eq!(
        replacement_info.session_id.to_string(),
        started.transcript_id.unwrap()
    );
    assert_eq!(replacement_info.cwd, started.cwd);

    current
        .session
        .handle
        .shutdown()
        .await
        .expect("shutdown current");
    replacement
        .handle
        .shutdown()
        .await
        .expect("shutdown replacement");
}
