use std::sync::Arc;
use std::time::Duration;

use nori_config::NoriConfig;
use nori_harness::runtime::SessionLaunchSpec;
use nori_harness::runtime::SessionResume;
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

#[tokio::test]
#[serial]
async fn shutdown_interrupts_a_hung_connection_attempt() {
    // SAFETY: this test is serialized with every other environment-mutating test.
    unsafe { std::env::set_var("MOCK_AGENT_HANG", "1") };
    let _guard = EnvGuard("MOCK_AGENT_HANG");
    let temp = tempfile::tempdir().expect("create session directory");
    let config = NoriConfig {
        active_agent: "mock-model".to_string(),
        cwd: temp.path().to_path_buf(),
        nori_home: temp.path().to_path_buf(),
        ..Default::default()
    };
    let mut session = launch_session(SessionLaunchSpec {
        config: Arc::new(config),
        cli_version: "boundary-test".to_string(),
        session_context: None,
        initial_context: None,
        resume: None,
    });

    tokio::time::timeout(Duration::from_secs(2), session.handle.shutdown())
        .await
        .expect("shutdown must not wait for the connect timeout")
        .expect("shutdown request should succeed");

    let ended = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if let Some(SessionEvent::Nori(NoriEvent::SessionEnded(ended))) =
                session.events.recv().await
            {
                return ended;
            }
        }
    })
    .await
    .expect("shutdown should close the public lifecycle");
    assert_eq!(ended.reason, nori_protocol::SessionEndReason::Shutdown);
    let next_event = tokio::time::timeout(Duration::from_secs(2), session.events.recv())
        .await
        .expect("shutdown should close the public event stream");
    assert!(next_event.is_none());
}

#[tokio::test]
#[serial]
async fn public_boundary_preserves_bootstrap_and_prompt_acp_envelopes() {
    let temp = tempfile::tempdir().expect("create session directory");
    let config = NoriConfig {
        active_agent: "mock-model".to_string(),
        cwd: temp.path().to_path_buf(),
        nori_home: temp.path().to_path_buf(),
        ..Default::default()
    };

    let mut session = launch_session(SessionLaunchSpec {
        config: Arc::new(config),
        cli_version: "boundary-test".to_string(),
        session_context: None,
        initial_context: None,
        resume: None,
    });

    tokio::time::timeout(Duration::from_secs(10), session.handle.get_session_config())
        .await
        .expect("session bootstrap should complete before the late consumer starts")
        .expect("session config should be available after bootstrap");

    let bootstrap = tokio::time::timeout(Duration::from_secs(10), async {
        let mut events = Vec::new();
        loop {
            let event = session
                .events
                .recv()
                .await
                .expect("session event stream closed during bootstrap");
            let started = matches!(event, SessionEvent::Nori(NoriEvent::SessionStarted(_)));
            events.push(event);
            if started {
                return events;
            }
        }
    })
    .await
    .expect("session bootstrap should complete");

    let initialize_responses = bootstrap
        .iter()
        .enumerate()
        .filter_map(|(index, event)| match event {
            SessionEvent::Acp(AcpEvent::Response {
                request_id,
                response: Ok(acp::v1::AgentResponse::InitializeResponse(_)),
            }) => Some((index, request_id)),
            _ => None,
        })
        .collect::<Vec<_>>();
    let new_session_responses = bootstrap
        .iter()
        .enumerate()
        .filter_map(|(index, event)| match event {
            SessionEvent::Acp(AcpEvent::Response {
                request_id,
                response: Ok(acp::v1::AgentResponse::NewSessionResponse(response)),
            }) => Some((index, request_id, response)),
            _ => None,
        })
        .collect::<Vec<_>>();
    let session_started_index = bootstrap
        .iter()
        .position(|event| matches!(event, SessionEvent::Nori(NoriEvent::SessionStarted(_))))
        .expect("session start should follow ACP bootstrap responses");
    let transcript_path = bootstrap
        .iter()
        .find_map(|event| match event {
            SessionEvent::Nori(NoriEvent::SessionStarted(started)) => {
                started.transcript_path.clone()
            }
            SessionEvent::Acp(_) | SessionEvent::Nori(_) => None,
        })
        .expect("session should expose its transcript path");

    assert_eq!(initialize_responses.len(), 1);
    assert_eq!(new_session_responses.len(), 1);
    let (initialize_index, initialize_request_id) = initialize_responses[0];
    let (new_session_index, new_session_request_id, new_session_response) =
        new_session_responses[0];
    assert!(initialize_index < new_session_index);
    assert!(new_session_index < session_started_index);
    assert_ne!(initialize_request_id, new_session_request_id);
    let acp_session_id = new_session_response.session_id.clone();

    let prompt_request_id = session
        .handle
        .prompt(vec![acp::v1::ContentBlock::Text(
            acp::v1::TextContent::new("hello from the boundary test"),
        )])
        .await
        .expect("submit prompt");

    let (chunks, stop_reason, prompting_request_id) =
        tokio::time::timeout(Duration::from_secs(10), async {
        let mut chunks = Vec::new();
        let mut prompting_request_id = None;
        loop {
            match session
                .events
                .recv()
                .await
                .expect("session event stream closed during prompt")
            {
                SessionEvent::Acp(AcpEvent::Notification(
                    acp::v1::AgentNotification::SessionNotification(notification),
                )) => {
                    assert_eq!(notification.session_id, acp_session_id);
                    if let acp::v1::SessionUpdate::AgentMessageChunk(chunk) = notification.update
                        && let acp::v1::ContentBlock::Text(text) = chunk.content
                    {
                        chunks.push(text.text);
                    }
                }
                SessionEvent::Acp(AcpEvent::Response {
                    request_id,
                    response: Ok(acp::v1::AgentResponse::PromptResponse(response)),
                }) if request_id == prompt_request_id => {
                    return (chunks, response.stop_reason, prompting_request_id);
                }
                SessionEvent::Nori(NoriEvent::SessionPhaseChanged(
                    nori_protocol::SessionPhase::Prompting { request_id },
                )) => prompting_request_id = Some(request_id),
                SessionEvent::Acp(AcpEvent::Response {
                    request_id,
                    response: Err(error),
                }) if request_id == prompt_request_id => {
                    panic!("submitted prompt failed with ACP error: {error:?}");
                }
                SessionEvent::Acp(AcpEvent::Response {
                    request_id,
                    response: Ok(acp::v1::AgentResponse::PromptResponse(_)),
                }) => {
                    panic!(
                        "received prompt response for {request_id:?}, expected {prompt_request_id:?}"
                    );
                }
                SessionEvent::Nori(NoriEvent::RequestFailed(failure)) => {
                    panic!("submitted prompt failed without an ACP response: {failure:?}");
                }
                SessionEvent::Nori(NoriEvent::SessionEnded(ended)) => {
                    panic!("session ended before prompt response: {ended:?}");
                }
                SessionEvent::Acp(_)
                | SessionEvent::Nori(_) => {}
            }
        }
    })
    .await
    .expect("prompt should complete");

    assert_eq!(chunks, vec!["Test message 1", "Test message 2"]);
    assert_eq!(stop_reason, acp::v1::StopReason::EndTurn);
    assert_eq!(prompting_request_id, Some(prompt_request_id));

    tokio::time::timeout(Duration::from_secs(10), session.handle.shutdown())
        .await
        .expect("shutdown should complete")
        .expect("shutdown session");
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if matches!(
                session.events.recv().await,
                Some(SessionEvent::Nori(NoriEvent::SessionEnded(_)))
            ) {
                break;
            }
        }
    })
    .await
    .expect("connected shutdown should emit SessionEnded");
    let next_event = tokio::time::timeout(Duration::from_secs(2), session.events.recv())
        .await
        .expect("connected shutdown should close the public event stream");
    assert!(next_event.is_none());

    let transcript = tokio::fs::read_to_string(transcript_path)
        .await
        .expect("read recorded transcript");
    let records = transcript
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).expect("valid transcript line"))
        .collect::<Vec<_>>();
    assert!(
        records.iter().all(|record| record["type"] != "assistant"),
        "v3 must not duplicate ACP assistant output in a second representation"
    );
    assert!(
        records
            .iter()
            .all(|record| record["type"] != "client_event"),
        "v3 must not persist the retired normalized protocol"
    );
    assert!(records.iter().any(|record| {
        record["type"] == "session_event"
            && record["event"]["source"] == "acp"
            && record["event"]["event"]["message_type"] == "notification"
    }));
    assert!(records.iter().any(|record| {
        record["type"] == "session_event"
            && record["event"]["source"] == "nori"
            && record["event"]["event"]["event_type"] == "session_ended"
    }));
}

#[tokio::test]
#[serial]
async fn delegated_permission_round_trips_as_raw_acp_with_the_same_request_id() {
    let temp = tempfile::tempdir().expect("create session directory");
    let config = NoriConfig {
        active_agent: "mock-model".to_string(),
        cwd: temp.path().to_path_buf(),
        nori_home: temp.path().to_path_buf(),
        ..Default::default()
    };
    let mut session = launch_session(SessionLaunchSpec {
        config: Arc::new(config),
        cli_version: "boundary-test".to_string(),
        session_context: None,
        initial_context: None,
        resume: None,
    });

    let acp_session_id = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            match session
                .events
                .recv()
                .await
                .expect("session event stream closed during bootstrap")
            {
                SessionEvent::Acp(AcpEvent::Response {
                    response: Ok(acp::v1::AgentResponse::NewSessionResponse(response)),
                    ..
                }) => return response.session_id,
                SessionEvent::Nori(NoriEvent::RequestFailed(failure)) => {
                    panic!("session bootstrap failed: {failure:?}");
                }
                SessionEvent::Acp(_) | SessionEvent::Nori(_) => {}
            }
        }
    })
    .await
    .expect("session bootstrap should complete");

    let prompt_request_id = session
        .handle
        .prompt(vec![acp::v1::ContentBlock::Text(
            acp::v1::TextContent::new("mock:request-permission"),
        )])
        .await
        .expect("submit permission prompt");

    let permission_request_id = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            match session
                .events
                .recv()
                .await
                .expect("session event stream closed before permission request")
            {
                SessionEvent::Acp(AcpEvent::Request {
                    request_id,
                    request: acp::v1::AgentRequest::RequestPermissionRequest(permission_request),
                }) => {
                    assert_eq!(permission_request.session_id, acp_session_id);
                    assert_eq!(permission_request.options.len(), 2);
                    return request_id;
                }
                SessionEvent::Acp(AcpEvent::Response {
                    request_id,
                    response: Ok(acp::v1::AgentResponse::PromptResponse(_)),
                }) if request_id == prompt_request_id => {
                    panic!("prompt completed before delegating its permission request");
                }
                SessionEvent::Nori(NoriEvent::RequestFailed(failure)) => {
                    panic!("permission prompt failed: {failure:?}");
                }
                SessionEvent::Acp(_) | SessionEvent::Nori(_) => {}
            }
        }
    })
    .await
    .expect("permission request should reach the embedder");

    session
        .handle
        .respond_to_agent(
            permission_request_id.clone(),
            Ok(acp::v1::ClientResponse::RequestPermissionResponse(
                acp::v1::RequestPermissionResponse::new(
                    acp::v1::RequestPermissionOutcome::Selected(
                        acp::v1::SelectedPermissionOutcome::new("allow"),
                    ),
                ),
            )),
        )
        .await
        .expect("respond to delegated permission request");

    let confirmation = tokio::time::timeout(Duration::from_secs(10), async {
        let mut confirmation = None;
        loop {
            match session
                .events
                .recv()
                .await
                .expect("session event stream closed before prompt completion")
            {
                SessionEvent::Acp(AcpEvent::Notification(
                    acp::v1::AgentNotification::SessionNotification(notification),
                )) => {
                    if let acp::v1::SessionUpdate::AgentMessageChunk(chunk) = notification.update
                        && let acp::v1::ContentBlock::Text(text) = chunk.content
                        && text.text.contains("Permission granted with option: allow")
                    {
                        confirmation = Some(text.text);
                    }
                }
                SessionEvent::Acp(AcpEvent::Response {
                    request_id,
                    response: Ok(acp::v1::AgentResponse::PromptResponse(response)),
                }) if request_id == prompt_request_id => {
                    assert_eq!(response.stop_reason, acp::v1::StopReason::EndTurn);
                    return confirmation;
                }
                SessionEvent::Nori(NoriEvent::RequestFailed(failure)) => {
                    panic!("permission response failed: {failure:?}");
                }
                SessionEvent::Acp(_) | SessionEvent::Nori(_) => {}
            }
        }
    })
    .await
    .expect("permission response should let the agent finish");

    assert_eq!(
        confirmation.as_deref(),
        Some("Permission granted with option: allow")
    );

    let rejected_prompt_request_id = session
        .handle
        .prompt(vec![acp::v1::ContentBlock::Text(
            acp::v1::TextContent::new("mock:request-permission"),
        )])
        .await
        .expect("submit second permission prompt");
    let rejected_permission_request_id = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            match session
                .events
                .recv()
                .await
                .expect("session event stream closed before second permission request")
            {
                SessionEvent::Acp(AcpEvent::Request {
                    request_id,
                    request: acp::v1::AgentRequest::RequestPermissionRequest(_),
                }) => return request_id,
                SessionEvent::Acp(AcpEvent::Response {
                    request_id,
                    response: Ok(acp::v1::AgentResponse::PromptResponse(_)),
                }) if request_id == rejected_prompt_request_id => {
                    panic!("second prompt completed before delegating its permission request");
                }
                SessionEvent::Acp(_) | SessionEvent::Nori(_) => {}
            }
        }
    })
    .await
    .expect("second permission request should reach the embedder");
    session
        .handle
        .respond_to_agent(
            rejected_permission_request_id,
            Err(acp::v1::Error::invalid_params().data("denied by boundary test")),
        )
        .await
        .expect("send schema-native error response");

    let rejected_confirmation = tokio::time::timeout(Duration::from_secs(10), async {
        let mut rejected_confirmation = None;
        loop {
            match session
                .events
                .recv()
                .await
                .expect("session event stream closed before rejected prompt completion")
            {
                SessionEvent::Acp(AcpEvent::Notification(
                    acp::v1::AgentNotification::SessionNotification(notification),
                )) => {
                    if let acp::v1::SessionUpdate::AgentMessageChunk(chunk) = notification.update
                        && let acp::v1::ContentBlock::Text(text) = chunk.content
                        && text.text == "Permission request failed"
                    {
                        rejected_confirmation = Some(text.text);
                    }
                }
                SessionEvent::Acp(AcpEvent::Response {
                    request_id,
                    response: Ok(acp::v1::AgentResponse::PromptResponse(response)),
                }) if request_id == rejected_prompt_request_id => {
                    assert_eq!(response.stop_reason, acp::v1::StopReason::EndTurn);
                    return rejected_confirmation;
                }
                SessionEvent::Acp(_) | SessionEvent::Nori(_) => {}
            }
        }
    })
    .await
    .expect("schema error should reach the agent without ending the session");
    assert_eq!(
        rejected_confirmation.as_deref(),
        Some("Permission request failed")
    );

    tokio::time::timeout(Duration::from_secs(10), session.handle.shutdown())
        .await
        .expect("shutdown should complete")
        .expect("shutdown session");
}

#[tokio::test]
#[serial]
async fn typed_session_list_query_also_preserves_the_raw_acp_response() {
    // SAFETY: tests that mutate the mock-agent environment run serially.
    unsafe { std::env::set_var("MOCK_AGENT_SUPPORT_SESSION_LIST", "1") };
    let _guard = EnvGuard("MOCK_AGENT_SUPPORT_SESSION_LIST");
    let temp = tempfile::tempdir().expect("create session directory");
    let config = NoriConfig {
        active_agent: "mock-model".to_string(),
        cwd: temp.path().to_path_buf(),
        nori_home: temp.path().to_path_buf(),
        ..Default::default()
    };
    let mut session = launch_session(SessionLaunchSpec {
        config: Arc::new(config),
        cli_version: "boundary-test".to_string(),
        session_context: None,
        initial_context: None,
        resume: None,
    });

    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if matches!(
                session.events.recv().await,
                Some(SessionEvent::Nori(NoriEvent::SessionStarted(_)))
            ) {
                break;
            }
        }
    })
    .await
    .expect("session bootstrap should complete");

    let sessions = session
        .handle
        .list_sessions(temp.path().to_path_buf())
        .await
        .expect("typed session list query should succeed");
    assert_eq!(sessions.len(), 2);
    assert_eq!(sessions[0].session_id.to_string(), "mock-session-1");

    let raw_response = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            match session
                .events
                .recv()
                .await
                .expect("session event stream closed before list response")
            {
                SessionEvent::Acp(AcpEvent::Response {
                    request_id,
                    response: Ok(acp::v1::AgentResponse::ListSessionsResponse(response)),
                }) => return (request_id, response),
                SessionEvent::Acp(_) | SessionEvent::Nori(_) => {}
            }
        }
    })
    .await
    .expect("ACP list response should remain observable");
    assert!(!raw_response.0.to_string().is_empty());
    assert_eq!(raw_response.1.sessions, sessions);

    session.handle.shutdown().await.expect("shutdown session");
}

#[tokio::test]
#[serial]
async fn close_returns_directly_then_ends_the_public_session_as_closed() {
    // SAFETY: tests that mutate the mock-agent environment run serially.
    unsafe { std::env::set_var("MOCK_AGENT_SUPPORT_SESSION_CLOSE", "1") };
    let _guard = EnvGuard("MOCK_AGENT_SUPPORT_SESSION_CLOSE");
    let temp = tempfile::tempdir().expect("create session directory");
    let config = NoriConfig {
        active_agent: "mock-model".to_string(),
        cwd: temp.path().to_path_buf(),
        nori_home: temp.path().to_path_buf(),
        ..Default::default()
    };
    let mut session = launch_session(SessionLaunchSpec {
        config: Arc::new(config),
        cli_version: "boundary-test".to_string(),
        session_context: None,
        initial_context: None,
        resume: None,
    });

    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if matches!(
                session.events.recv().await,
                Some(SessionEvent::Nori(NoriEvent::SessionStarted(_)))
            ) {
                break;
            }
        }
    })
    .await
    .expect("session bootstrap should complete");

    session
        .handle
        .close_session()
        .await
        .expect("typed close should succeed");

    let events = tokio::time::timeout(Duration::from_secs(10), async {
        let mut events = Vec::new();
        loop {
            let event = session
                .events
                .recv()
                .await
                .expect("session event stream closed before terminal lifecycle event");
            let ended = matches!(event, SessionEvent::Nori(NoriEvent::SessionEnded(_)));
            events.push(event);
            if ended {
                return events;
            }
        }
    })
    .await
    .expect("close should end the public session");

    let close_response_index = events
        .iter()
        .position(|event| {
            matches!(
                event,
                SessionEvent::Acp(AcpEvent::Response {
                    response: Ok(acp::v1::AgentResponse::CloseSessionResponse(_)),
                    ..
                })
            )
        })
        .expect("raw ACP close response should remain observable");
    let (ended_index, ended) = events
        .iter()
        .enumerate()
        .find_map(|(index, event)| match event {
            SessionEvent::Nori(NoriEvent::SessionEnded(ended)) => Some((index, ended)),
            SessionEvent::Acp(_) | SessionEvent::Nori(_) => None,
        })
        .expect("close should emit its Nori lifecycle result");
    assert!(close_response_index < ended_index);
    assert_eq!(ended.reason, nori_protocol::SessionEndReason::Closed);

    let next_event = tokio::time::timeout(Duration::from_secs(2), session.events.recv())
        .await
        .expect("close should close the public event stream");
    assert!(next_event.is_none());
}

#[tokio::test]
#[serial]
async fn resumed_load_preserves_bootstrap_response_and_brackets_raw_replay() {
    // SAFETY: tests that mutate the mock-agent environment run serially.
    unsafe {
        std::env::set_var("MOCK_AGENT_SUPPORT_LOAD_SESSION", "1");
        std::env::set_var("MOCK_AGENT_LOAD_SESSION_NOTIFICATION_COUNT", "2");
    }
    let _load_guard = EnvGuard("MOCK_AGENT_SUPPORT_LOAD_SESSION");
    let _count_guard = EnvGuard("MOCK_AGENT_LOAD_SESSION_NOTIFICATION_COUNT");
    let temp = tempfile::tempdir().expect("create session directory");
    let config = NoriConfig {
        active_agent: "mock-model".to_string(),
        cwd: temp.path().to_path_buf(),
        nori_home: temp.path().to_path_buf(),
        ..Default::default()
    };
    let mut session = launch_session(SessionLaunchSpec {
        config: Arc::new(config),
        cli_version: "boundary-test".to_string(),
        session_context: None,
        initial_context: None,
        resume: Some(SessionResume {
            acp_session_id: Some("existing-session".to_string()),
            transcript: None,
        }),
    });

    let events = tokio::time::timeout(Duration::from_secs(10), async {
        let mut events = Vec::new();
        let mut saw_load_response = false;
        let mut saw_replay_finished = false;
        while !(saw_load_response && saw_replay_finished) {
            let event = session
                .events
                .recv()
                .await
                .expect("resume event stream closed");
            saw_load_response |= matches!(
                event,
                SessionEvent::Acp(AcpEvent::Response {
                    response: Ok(acp::v1::AgentResponse::LoadSessionResponse(_)),
                    ..
                })
            );
            saw_replay_finished |= matches!(event, SessionEvent::Nori(NoriEvent::ReplayFinished));
            events.push(event);
        }
        events
    })
    .await
    .expect("load replay should complete");

    let initialize_index = events
        .iter()
        .position(|event| {
            matches!(
                event,
                SessionEvent::Acp(AcpEvent::Response {
                    response: Ok(acp::v1::AgentResponse::InitializeResponse(_)),
                    ..
                })
            )
        })
        .expect("initialize response must survive resume bootstrap");
    let started_index = events
        .iter()
        .position(|event| matches!(event, SessionEvent::Nori(NoriEvent::SessionStarted(_))))
        .expect("resumed session should start");
    assert!(initialize_index < started_index);

    let replay_started_index = events
        .iter()
        .position(|event| matches!(event, SessionEvent::Nori(NoriEvent::ReplayStarted(_))))
        .expect("replay should be bracketed");
    let replay_finished_index = events
        .iter()
        .position(|event| matches!(event, SessionEvent::Nori(NoriEvent::ReplayFinished)))
        .expect("replay should finish");
    let (load_response_index, load_request_id) = events
        .iter()
        .enumerate()
        .find_map(|(index, event)| match event {
            SessionEvent::Acp(AcpEvent::Response {
                request_id,
                response: Ok(acp::v1::AgentResponse::LoadSessionResponse(_)),
            }) => Some((index, request_id.clone())),
            SessionEvent::Acp(_) | SessionEvent::Nori(_) => None,
        })
        .expect("load response should remain observable");
    let loading_request_id = events.iter().find_map(|event| match event {
        SessionEvent::Nori(NoriEvent::SessionPhaseChanged(
            nori_protocol::SessionPhase::Loading { request_id },
        )) => Some(request_id.clone()),
        SessionEvent::Acp(_) | SessionEvent::Nori(_) => None,
    });
    assert_eq!(loading_request_id, Some(load_request_id));
    assert!(load_response_index < started_index);
    assert!(
        load_response_index < replay_started_index || load_response_index > replay_finished_index,
        "the current load response must not be labeled as historical replay"
    );
    let replay_notifications = events
        .iter()
        .enumerate()
        .filter(|(_, event)| {
            matches!(
                event,
                SessionEvent::Acp(AcpEvent::Notification(
                    acp::v1::AgentNotification::SessionNotification(_)
                ))
            )
        })
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    assert_eq!(replay_notifications.len(), 2);
    assert!(
        replay_notifications
            .iter()
            .all(|index| replay_started_index < *index && *index < replay_finished_index)
    );

    session.handle.shutdown().await.expect("shutdown session");
}

#[tokio::test]
#[serial]
async fn failed_load_response_precedes_fallback_session_start() {
    // SAFETY: this test is serialized with every other environment-mutating test.
    unsafe {
        std::env::set_var("MOCK_AGENT_SUPPORT_LOAD_SESSION", "1");
        std::env::set_var("MOCK_AGENT_LOAD_SESSION_FAIL", "1");
    }
    let _load_guard = EnvGuard("MOCK_AGENT_SUPPORT_LOAD_SESSION");
    let _fail_guard = EnvGuard("MOCK_AGENT_LOAD_SESSION_FAIL");
    let temp = tempfile::tempdir().expect("create session directory");
    let config = NoriConfig {
        active_agent: "mock-model".to_string(),
        cwd: temp.path().to_path_buf(),
        nori_home: temp.path().to_path_buf(),
        ..Default::default()
    };
    let mut session = launch_session(SessionLaunchSpec {
        config: Arc::new(config),
        cli_version: "boundary-test".to_string(),
        session_context: None,
        initial_context: None,
        resume: Some(SessionResume {
            acp_session_id: Some("missing-session".to_string()),
            transcript: None,
        }),
    });

    let events = tokio::time::timeout(Duration::from_secs(10), async {
        let mut events = Vec::new();
        loop {
            let event = session
                .events
                .recv()
                .await
                .expect("fallback event stream closed");
            let started = matches!(event, SessionEvent::Nori(NoriEvent::SessionStarted(_)));
            events.push(event);
            if started {
                return events;
            }
        }
    })
    .await
    .expect("fallback session should start");

    let failed_load_index = events
        .iter()
        .position(|event| {
            matches!(
                event,
                SessionEvent::Acp(AcpEvent::Response {
                    response: Err(_),
                    ..
                })
            )
        })
        .expect("failed load must remain a raw ACP response");
    let fallback_new_index = events
        .iter()
        .position(|event| {
            matches!(
                event,
                SessionEvent::Acp(AcpEvent::Response {
                    response: Ok(acp::v1::AgentResponse::NewSessionResponse(_)),
                    ..
                })
            )
        })
        .expect("fallback session/new response must remain observable");
    let started_index = events
        .iter()
        .position(|event| matches!(event, SessionEvent::Nori(NoriEvent::SessionStarted(_))))
        .expect("fallback session should start");
    assert!(failed_load_index < fallback_new_index);
    assert!(fallback_new_index < started_index);

    session.handle.shutdown().await.expect("shutdown session");
}
