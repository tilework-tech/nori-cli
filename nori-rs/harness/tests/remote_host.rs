//! Behavior of [`HarnessRemoteHost`] over a real harness session backed by
//! the mock ACP agent: stable outward session ids, remote prompt turns in
//! stream order, transcript-backed `session/load` replay, and delegated
//! permission routing for remote-owned turns.

use std::sync::Arc;
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
async fn queued_remote_prompt_does_not_freeze_the_host() {
    // SAFETY: tests that mutate the mock-agent environment run serially.
    unsafe { std::env::set_var("MOCK_AGENT_DELAY_MS", "3000") };
    let _guard = EnvGuard("MOCK_AGENT_DELAY_MS");

    let fixture = launch_attached().await;
    let info = wait_for_session(&fixture.host).await;
    let session_id = info.session_id.clone();
    let mut subscription = fixture.host.subscribe().await;

    // The first remote turn takes several seconds to finish.
    let first_request_id = fixture
        .host
        .prompt(
            &session_id,
            vec![acp::ContentBlock::Text(acp::TextContent::new("first"))],
            None,
        )
        .await
        .expect("submit the first prompt");

    // A second prompt queues behind the active turn; its submission resolves
    // only when the queue drains, so it must not hold the host hostage.
    let queued = {
        let host = fixture.host.clone();
        let session_id = session_id.clone();
        tokio::spawn(async move {
            host.prompt(
                &session_id,
                vec![acp::ContentBlock::Text(acp::TextContent::new("second"))],
                None,
            )
            .await
        })
    };
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Regression (H1): while the second prompt is queued, other host methods
    // must stay responsive instead of blocking until the turn ends.
    let sessions = tokio::time::timeout(Duration::from_secs(2), fixture.host.list_sessions())
        .await
        .expect("list_sessions must not block behind a queued prompt")
        .expect("list sessions");
    assert_eq!(sessions.len(), 1);

    // Both turns then complete in order once the queue drains.
    let second_request_id = tokio::time::timeout(Duration::from_secs(15), queued)
        .await
        .expect("the queued prompt should be issued when the first turn ends")
        .expect("prompt task")
        .expect("submit the second prompt");
    let mut outcomes = Vec::new();
    let mut sequence = Vec::new();
    tokio::time::timeout(Duration::from_secs(15), async {
        while outcomes.len() < 2 {
            let event = subscription
                .events
                .recv()
                .await
                .expect("subscription closed before both turns ended");
            match event {
                SessionEvent::Acp(AcpEvent::Notification(
                    acp::AgentNotification::SessionNotification(notification),
                )) => {
                    if let acp::SessionUpdate::UserMessageChunk(chunk) = notification.update
                        && let acp::ContentBlock::Text(text) = chunk.content
                    {
                        sequence.push(format!("user:{}", text.text));
                    }
                }
                SessionEvent::Acp(AcpEvent::Response { request_id, .. }) => {
                    sequence.push(if request_id == first_request_id {
                        "response:first".to_string()
                    } else {
                        "response:second".to_string()
                    });
                    outcomes.push(request_id);
                }
                SessionEvent::Acp(_) | SessionEvent::Nori(_) => {}
            }
        }
    })
    .await
    .expect("both turns should complete");
    assert_eq!(outcomes, vec![first_request_id, second_request_id]);
    assert_eq!(
        sequence,
        vec![
            "user:first".to_string(),
            "response:first".to_string(),
            "user:second".to_string(),
            "response:second".to_string(),
        ],
        "a queued prompt must not enter the fan-out until it becomes active"
    );

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
