use nori_harness::context_timeline::ContextSegment;
use nori_harness::transcript::TranscriptLoader;
use nori_protocol::AcpEvent;
use nori_protocol::NoriEvent;
use nori_protocol::SessionEvent;
use nori_protocol::SessionPhase;
use nori_protocol::acp::v1 as acp;
use pretty_assertions::assert_eq;
use serde_json::json;

#[path = "context_timeline/profile.rs"]
mod profile;

fn phase(id: i64) -> SessionEvent {
    SessionEvent::Nori(NoriEvent::SessionPhaseChanged(SessionPhase::Prompting {
        request_id: id.into(),
    }))
}

fn usage(used: u64, size: u64) -> SessionEvent {
    session_usage("agent-session", used, size)
}

fn session_usage(session: &str, used: u64, size: u64) -> SessionEvent {
    SessionEvent::Acp(AcpEvent::Notification(
        acp::AgentNotification::SessionNotification(acp::SessionNotification::new(
            session.to_string(),
            acp::SessionUpdate::UsageUpdate(acp::UsageUpdate::new(used, size)),
        )),
    ))
}

fn end(id: i64) -> SessionEvent {
    SessionEvent::Acp(AcpEvent::Response {
        request_id: id.into(),
        response: Ok(acp::AgentResponse::PromptResponse(
            acp::PromptResponse::new(acp::StopReason::EndTurn),
        )),
    })
}

async fn load(events: Vec<SessionEvent>) -> nori_harness::Transcript {
    load_evidence(events, false).await
}

#[expect(
    clippy::expect_used,
    reason = "test helpers fail with a focused boundary diagnostic"
)]
async fn load_evidence(events: Vec<SessionEvent>, historical: bool) -> nori_harness::Transcript {
    let dir = tempfile::tempdir().expect("temporary transcript directory");
    let session_dir = dir.path().join("transcripts/by-project/project/sessions");
    tokio::fs::create_dir_all(&session_dir)
        .await
        .expect("create transcript directory");
    let mut lines =
        vec![json!({
        "ts":"2026-01-01T00:00:00Z", "v":3, "type":"session_meta",
        "session_id":"timeline", "project_id":"project", "started_at":"2026-01-01T00:00:00Z",
        "cwd":"/repo", "cli_version":"test", "acp_session_id":"agent-session"
    }).to_string()];
    for (index, event) in events.into_iter().enumerate() {
        let response_kind = match &event {
            SessionEvent::Acp(AcpEvent::Response {
                response: Ok(acp::AgentResponse::NewSessionResponse(_)),
                ..
            }) => Some("new_session"),
            SessionEvent::Acp(AcpEvent::Response {
                response: Ok(acp::AgentResponse::ForkSessionResponse(_)),
                ..
            }) => Some("fork"),
            _ => None,
        };
        lines.push(json!({
            "ts":format!("2026-01-01T00:01:{index:02}Z"), "v":3, "type":"session_event", "event":event,
            "observation_id":format!("evidence-{index}"),
            "observed_at":format!("2026-01-01T00:00:{index:02}Z"),
            "historical_evidence":historical,
            "response_kind":response_kind,
        }).to_string());
    }
    tokio::fs::write(session_dir.join("timeline.jsonl"), lines.join("\n") + "\n")
        .await
        .expect("write transcript");
    TranscriptLoader::new(dir.path().to_path_buf())
        .load_transcript("project", "timeline")
        .await
        .expect("load transcript")
}

#[tokio::test]
async fn boundaries_capture_latest_readings_without_late_rewrites() {
    let timeline = load(vec![
        phase(10),
        usage(100, 1000),
        usage(120, 1000),
        end(10),
        usage(130, 1000),
        phase(11),
        end(11),
        phase(12),
        usage(0, 1000),
        end(12),
    ])
    .await
    .context_timeline();
    assert_eq!(timeline.turns.len(), 3);
    assert!(timeline.turns[0].start.observation.is_none());
    let first_end = timeline.turns[0].end.as_ref().unwrap();
    assert_eq!(first_end.observation.as_ref().unwrap().usage.used, 120);
    assert_eq!(first_end.recorded_at, "2026-01-01T00:00:03Z");
    assert_eq!(
        first_end.observation.as_ref().unwrap().observed_at,
        "2026-01-01T00:00:02Z"
    );
    assert_eq!(
        timeline.turns[1]
            .start
            .observation
            .as_ref()
            .unwrap()
            .usage
            .used,
        130
    );
    assert_eq!(
        timeline.turns[1].start.observation,
        timeline.turns[1].end.as_ref().unwrap().observation
    );
    assert_eq!(timeline.turns[1].delta(), None);
    assert_eq!(timeline.current.as_ref().unwrap().usage.used, 0);
    assert_eq!(
        timeline.turns[2].delta(),
        None,
        "a decrease invalidates turn attribution"
    );
}

#[tokio::test]
async fn segments_represent_baseline_and_observed_turn_growth() {
    let timeline = load(vec![
        usage(100, 1000),
        phase(10),
        usage(120, 1000),
        usage(150, 1000),
        end(10),
        phase(11),
        usage(190, 1000),
    ])
    .await
    .context_timeline();
    assert_eq!(
        timeline.segments,
        vec![
            ContextSegment {
                turn: None,
                tokens: 100
            },
            ContextSegment {
                turn: Some(1),
                tokens: 50
            },
            ContextSegment {
                turn: Some(2),
                tokens: 40
            },
        ]
    );
    assert_eq!(timeline.turns[0].delta(), Some(50));
    assert!(timeline.turns[1].end.is_none());
    assert_eq!(timeline.current_turn_delta(), Some(40));
    assert_eq!(
        timeline
            .segments
            .iter()
            .map(|segment| segment.tokens)
            .sum::<i64>(),
        190
    );
}

#[tokio::test]
async fn compaction_decrease_and_capacity_changes_reset_attribution() {
    for reset in [
        vec![usage(80, 1000)],
        vec![usage(160, 2000)],
        vec![
            SessionEvent::Nori(NoriEvent::ContextCompacted(
                nori_protocol::ContextCompactedEvent { summary: None },
            )),
            usage(160, 1000),
        ],
    ] {
        let mut events = vec![usage(100, 1000), phase(10), usage(150, 1000)];
        events.extend(reset);
        events.push(end(10));
        let timeline = load(events).await.context_timeline();
        let current = timeline.current.as_ref().unwrap().usage.used as i64;
        assert_eq!(
            timeline.segments,
            vec![ContextSegment {
                turn: None,
                tokens: current
            }]
        );
        assert_eq!(timeline.turns[0].delta(), None);
    }
}

#[tokio::test]
async fn replay_and_foreign_sessions_do_not_create_new_observations_or_turns() {
    let timeline = load(vec![
        usage(100, 1000),
        SessionEvent::Nori(NoriEvent::ReplayStarted(nori_protocol::ReplayStarted {
            source: nori_protocol::ReplaySource::Agent,
        })),
        phase(90),
        usage(999, 1000),
        end(90),
        SessionEvent::Nori(NoriEvent::ReplayFinished),
        phase(10),
        session_usage("different-session", 888, 1000),
        usage(125, 1000),
        end(10),
    ])
    .await
    .context_timeline();
    assert_eq!(timeline.turns.len(), 1);
    assert_eq!(
        timeline.turns[0]
            .start
            .observation
            .as_ref()
            .unwrap()
            .usage
            .used,
        100
    );
    assert_eq!(timeline.current.as_ref().unwrap().usage.used, 125);
    assert_eq!(timeline.turns[0].delta(), Some(25));
}

#[tokio::test]
async fn cancellation_and_terminal_failures_close_each_prompt_once() {
    let failure = SessionEvent::Nori(NoriEvent::RequestFailed(nori_protocol::RequestFailure {
        request_id: Some(11.into()),
        message: "connection failed".into(),
        kind: nori_protocol::RequestFailureKind::Fatal,
    }));
    let ended = SessionEvent::Nori(NoriEvent::SessionEnded(nori_protocol::SessionEnded {
        reason: nori_protocol::SessionEndReason::ConnectionLost,
        message: None,
    }));
    let transcript = load(vec![
        usage(100, 1000),
        phase(10),
        SessionEvent::Nori(NoriEvent::SessionPhaseChanged(SessionPhase::Cancelling {
            request_id: 10.into(),
        })),
        usage(125, 1000),
        end(10),
        phase(11),
        failure,
        ended,
    ])
    .await;
    let timeline = transcript.context_timeline();
    assert_eq!(timeline.turns.len(), 2);
    assert_eq!(timeline.turns[0].delta(), Some(25));
    assert!(timeline.turns.iter().all(|turn| turn.end.is_some()));
    assert_eq!(
        timeline.turns[1].end.as_ref().unwrap().recorded_at,
        "2026-01-01T00:00:06Z"
    );
}

#[tokio::test]
async fn historical_evidence_reconstructs_timeline_without_entering_conversation_records() {
    let transcript = load_evidence(
        vec![usage(100, 1000), phase(10), usage(125, 1000), end(10)],
        true,
    )
    .await;
    assert_eq!(transcript.context_timeline().turns.len(), 1);
    assert_eq!(
        transcript.records().count(),
        0,
        "archived timeline evidence is not conversation replay"
    );
    assert_eq!(
        transcript.context_timeline().current.unwrap().id,
        "evidence-2"
    );
}

#[tokio::test]
async fn missing_start_history_is_explicit_without_inventing_a_turn() {
    let timeline = load(vec![usage(100, 1000), end(10)])
        .await
        .context_timeline();
    assert!(timeline.turns.is_empty());
    assert!(timeline.incomplete_history);
    assert_eq!(timeline.current.as_ref().unwrap().usage.used, 100);
    assert_eq!(timeline.current_turn_delta(), None);
}

struct EnvGuard(Vec<&'static str>);
impl Drop for EnvGuard {
    fn drop(&mut self) {
        for name in &self.0 {
            // SAFETY: this integration test is serialized with mock-agent environment users.
            unsafe { std::env::remove_var(name) };
        }
    }
}

#[expect(
    clippy::expect_used,
    reason = "test helpers fail with a focused boundary diagnostic"
)]
async fn launch(
    cwd: &std::path::Path,
    start: nori_harness::runtime::SessionStart,
) -> nori_harness::runtime::LaunchedSession {
    let agent = nori_harness::runtime::prepare_agent(nori_harness::runtime::AgentPrepareSpec {
        config: std::sync::Arc::new(nori_config::NoriConfig {
            active_agent: "mock-model".into(),
            cwd: cwd.into(),
            nori_home: cwd.into(),
            ..Default::default()
        }),
        cli_version: "context-test".into(),
        session_context: None,
        initial_context: None,
    })
    .await
    .expect("prepare agent");
    nori_harness::runtime::launch_session(nori_harness::runtime::SessionLaunchSpec { agent, start })
}

#[expect(
    clippy::expect_used,
    reason = "test helpers fail with a focused boundary diagnostic"
)]
async fn receive_matching(
    session: &mut nori_harness::runtime::LaunchedSession,
    predicate: impl Fn(&SessionEvent) -> bool,
) -> SessionEvent {
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let event = session.events.recv().await.expect("event stream closed");
            if predicate(&event) {
                return event;
            }
        }
    })
    .await
    .expect("expected public boundary event")
}

async fn started(
    session: &mut nori_harness::runtime::LaunchedSession,
) -> nori_protocol::SessionStarted {
    match receive_matching(session, |event| {
        matches!(event, SessionEvent::Nori(NoriEvent::SessionStarted(_)))
    })
    .await
    {
        SessionEvent::Nori(NoriEvent::SessionStarted(started)) => started,
        _ => unreachable!(),
    }
}

#[expect(
    clippy::expect_used,
    reason = "test helpers fail with a focused boundary diagnostic"
)]
async fn prompt(session: &mut nori_harness::runtime::LaunchedSession) {
    let request_id = session
        .handle
        .prompt(vec![acp::ContentBlock::Text(acp::TextContent::new(
            "context checkpoint",
        ))])
        .await
        .expect("prompt");
    receive_matching(session,|event| matches!(event,SessionEvent::Acp(AcpEvent::Response {request_id:id,response:Ok(acp::AgentResponse::PromptResponse(_))}) if id==&request_id)).await;
}

#[expect(
    clippy::expect_used,
    reason = "test helpers fail with a focused boundary diagnostic"
)]
async fn saved(
    session: &nori_harness::runtime::LaunchedSession,
    cwd: &std::path::Path,
    id: &str,
) -> nori_harness::Transcript {
    session
        .handle
        .flush_transcript()
        .await
        .expect("flush transcript");
    let loader = TranscriptLoader::new(cwd.into());
    let meta = loader
        .find_session_metadata_by_id(id)
        .await
        .expect("lookup transcript")
        .expect("transcript exists");
    loader
        .load_transcript(&meta.project_id, id)
        .await
        .expect("load transcript")
}

#[tokio::test]
#[serial_test::serial]
async fn live_saved_forked_and_repeatedly_resumed_history_share_original_checkpoints() {
    // SAFETY: this test runs serially with all mock-agent environment users.
    unsafe {
        std::env::set_var(
            "MOCK_AGENT_CONTEXT_SEQUENCE",
            r#"[[{"used":100,"size":1000}],[{"used":150,"size":1000}],[{"used":190,"size":1000}]]"#,
        );
        std::env::set_var("MOCK_AGENT_SUPPORT_SESSION_FORK", "1");
        std::env::set_var("MOCK_AGENT_SUPPORT_LOAD_SESSION", "1");
        std::env::set_var("MOCK_AGENT_LOAD_SESSION_NOTIFICATION_COUNT", "2");
    }
    let _guard = EnvGuard(vec![
        "MOCK_AGENT_CONTEXT_SEQUENCE",
        "MOCK_AGENT_SUPPORT_SESSION_FORK",
        "MOCK_AGENT_SUPPORT_LOAD_SESSION",
        "MOCK_AGENT_LOAD_SESSION_NOTIFICATION_COUNT",
    ]);
    let dir = tempfile::tempdir().expect("session directory");
    let mut session = launch(dir.path(), nori_harness::runtime::SessionStart::New).await;
    let parent = started(&mut session).await;
    prompt(&mut session).await;
    prompt(&mut session).await;
    let live = session
        .handle
        .context_timeline()
        .await
        .expect("live timeline");
    assert_eq!(live.turns.len(), 2);
    assert_eq!(live.turns[1].delta(), Some(50));
    let parent_saved = saved(&session, dir.path(), parent.transcript_id.as_ref().unwrap()).await;
    assert_eq!(parent_saved.context_timeline(), live);
    session.handle.branch().await.expect("fork session");
    let forked = receive_matching(&mut session, |event| {
        matches!(event, SessionEvent::Nori(NoriEvent::SessionForked(_)))
    })
    .await;
    let SessionEvent::Nori(NoriEvent::SessionForked(forked)) = forked else {
        unreachable!()
    };
    let fork_saved = saved(&session, dir.path(), &forked.new_conversation_id).await;
    assert_eq!(fork_saved.context_timeline(), live);
    prompt(&mut session).await;
    let mut expected = session
        .handle
        .context_timeline()
        .await
        .expect("fork timeline");
    assert_eq!(expected.turns.len(), 3);
    assert_eq!(expected.current.as_ref().unwrap().usage.used, 190);
    let mut transcript = saved(&session, dir.path(), &forked.new_conversation_id).await;
    assert_eq!(transcript.context_timeline(), expected);
    session.handle.shutdown().await.expect("shutdown fork");
    for _ in 0..2 {
        let acp_session_id = transcript.meta.acp_session_id.clone();
        let mut resumed = launch(
            dir.path(),
            nori_harness::runtime::SessionStart::Resume(nori_harness::runtime::SessionResume {
                acp_session_id,
                transcript: Some(transcript),
            }),
        )
        .await;
        let current = started(&mut resumed).await;
        receive_matching(&mut resumed, |event| {
            matches!(event, SessionEvent::Nori(NoriEvent::ReplayFinished))
        })
        .await;
        assert_eq!(
            resumed
                .handle
                .context_timeline()
                .await
                .expect("resumed timeline"),
            expected
        );
        transcript = saved(
            &resumed,
            dir.path(),
            current.transcript_id.as_ref().unwrap(),
        )
        .await;
        assert_eq!(transcript.context_timeline(), expected);
        let replayed_text = transcript
            .records()
            .filter_map(|record| match record {
                nori_harness::TranscriptRecord::SessionEvent(SessionEvent::Acp(
                    AcpEvent::Notification(acp::AgentNotification::SessionNotification(
                        notification,
                    )),
                )) => match &notification.update {
                    acp::SessionUpdate::AgentMessageChunk(chunk) => match &chunk.content {
                        acp::ContentBlock::Text(text) => Some(text.text.clone()),
                        _ => None,
                    },
                    _ => None,
                },
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            replayed_text,
            vec!["replay chunk 0", "replay chunk 1"],
            "provider replay newer than local evidence must be retained once"
        );
        let inherited_turns = expected.turns.len();
        prompt(&mut resumed).await;
        expected = resumed
            .handle
            .context_timeline()
            .await
            .expect("new resumed turn");
        assert_eq!(expected.turns.len(), inherited_turns + 1);
        assert_eq!(expected.current.as_ref().unwrap().usage.used, 100);
        assert_ne!(
            expected.current.as_ref().unwrap().id,
            expected
                .turns
                .last()
                .unwrap()
                .start
                .observation
                .as_ref()
                .unwrap()
                .id
        );
        transcript = saved(
            &resumed,
            dir.path(),
            current.transcript_id.as_ref().unwrap(),
        )
        .await;
        assert_eq!(transcript.context_timeline(), expected);
        resumed
            .handle
            .shutdown()
            .await
            .expect("shutdown resumed session");
    }
}

#[tokio::test]
async fn compaction_without_a_fresh_reading_leaves_current_context_unknown() {
    let timeline = load(vec![
        usage(100, 1000),
        phase(10),
        SessionEvent::Nori(NoriEvent::ContextCompacted(
            nori_protocol::ContextCompactedEvent { summary: None },
        )),
        end(10),
        phase(11),
    ])
    .await
    .context_timeline();
    assert_eq!(timeline.turns.len(), 2);
    assert!(timeline.current.is_none());
    assert!(timeline.segments.is_empty());
    assert!(
        timeline.turns[0]
            .end
            .as_ref()
            .unwrap()
            .observation
            .is_none()
    );
    assert!(timeline.turns[1].start.observation.is_none());
    assert_eq!(timeline.current_turn_delta(), None);
}

#[tokio::test]
#[serial_test::serial]
async fn queued_prompt_starts_once_with_its_wire_request_id_after_previous_end() {
    // SAFETY: mock-agent environment users run serially.
    unsafe {
        std::env::set_var(
            "MOCK_AGENT_CONTEXT_SEQUENCE",
            r#"[[{"used":100,"size":1000}],[{"used":150,"size":1000}]]"#,
        );
        std::env::set_var("MOCK_AGENT_DELAY_MS", "100");
    }
    let _guard = EnvGuard(vec!["MOCK_AGENT_CONTEXT_SEQUENCE", "MOCK_AGENT_DELAY_MS"]);
    let dir = tempfile::tempdir().expect("session directory");
    let mut session = launch(dir.path(), nori_harness::runtime::SessionStart::New).await;
    started(&mut session).await;
    let first = session
        .handle
        .prompt(vec![acp::ContentBlock::Text(acp::TextContent::new(
            "first",
        ))])
        .await
        .expect("first prompt");
    let handle = session.handle.clone();
    let queued = tokio::spawn(async move {
        handle
            .prompt(vec![acp::ContentBlock::Text(acp::TextContent::new(
                "second",
            ))])
            .await
            .expect("queued prompt")
    });
    let second = queued.await.expect("queued task");
    let mut observed_starts = Vec::new();
    let mut saw_queued = false;
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let event = session.events.recv().await.expect("event stream");
            match event {
                SessionEvent::Nori(NoriEvent::SessionPhaseChanged(SessionPhase::Prompting {
                    request_id,
                })) => observed_starts.push(request_id),
                SessionEvent::Nori(NoriEvent::QueueChanged(queue)) if !queue.prompts.is_empty() => {
                    saw_queued = true
                }
                SessionEvent::Acp(AcpEvent::Response {
                    request_id,
                    response: Ok(acp::AgentResponse::PromptResponse(_)),
                }) if request_id == second => break,
                _ => {}
            }
        }
    })
    .await
    .expect("queued completion");
    assert!(
        saw_queued,
        "test must observe admission while the first prompt is busy"
    );
    assert_eq!(
        observed_starts,
        vec![first.clone(), second.clone()],
        "only actual ACP admissions produce starts"
    );
    let timeline = session.handle.context_timeline().await.expect("timeline");
    assert_eq!(
        timeline
            .turns
            .iter()
            .map(|turn| turn.request_id.clone())
            .collect::<Vec<_>>(),
        vec![first, second]
    );
    assert_eq!(
        timeline.turns[0].end.as_ref().unwrap().observation,
        timeline.turns[1].start.observation
    );
    assert_eq!(timeline.turns[1].delta(), Some(50));
    session.handle.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn raw_error_and_disconnect_each_close_an_active_prompt() {
    for terminal in [
        SessionEvent::Acp(AcpEvent::Response {
            request_id: 10.into(),
            response: Err(acp::Error::internal_error()),
        }),
        SessionEvent::Nori(NoriEvent::SessionEnded(nori_protocol::SessionEnded {
            reason: nori_protocol::SessionEndReason::ConnectionLost,
            message: None,
        })),
    ] {
        let timeline = load(vec![
            usage(100, 1000),
            phase(10),
            usage(130, 1000),
            terminal,
        ])
        .await
        .context_timeline();
        assert_eq!(timeline.turns.len(), 1);
        assert_eq!(timeline.turns[0].delta(), Some(30));
        assert!(timeline.turns[0].end.is_some());
    }
}

#[tokio::test]
#[serial_test::serial]
async fn resuming_legacy_messages_keeps_unknown_checkpoint_history_after_reload() {
    let dir = tempfile::tempdir().expect("legacy session directory");
    let session_dir = dir.path().join("transcripts/by-project/project/sessions");
    tokio::fs::create_dir_all(&session_dir)
        .await
        .expect("create transcript directory");
    tokio::fs::write(session_dir.join("legacy.jsonl"), concat!(
        r#"{"ts":"2025-01-01T00:00:00Z","v":2,"type":"session_meta","session_id":"legacy","project_id":"project","started_at":"2025-01-01T00:00:00Z","cwd":"/repo","cli_version":"test"}"#,
        "\n",
        r#"{"ts":"2025-01-01T00:00:01Z","v":2,"type":"user","id":"old-user","content":"legacy question"}"#,
        "\n"
    )).await.expect("write legacy transcript");
    let transcript = TranscriptLoader::new(dir.path().into())
        .load_transcript("project", "legacy")
        .await
        .expect("load legacy transcript");
    assert!(transcript.context_timeline().incomplete_history);
    let mut session = launch(
        dir.path(),
        nori_harness::runtime::SessionStart::Resume(nori_harness::runtime::SessionResume {
            acp_session_id: None,
            transcript: Some(transcript),
        }),
    )
    .await;
    let current = started(&mut session).await;
    receive_matching(&mut session, |event| {
        matches!(event, SessionEvent::Nori(NoriEvent::ReplayFinished))
    })
    .await;
    assert!(
        session
            .handle
            .context_timeline()
            .await
            .expect("live legacy timeline")
            .incomplete_history
    );
    let saved = saved(
        &session,
        dir.path(),
        current.transcript_id.as_ref().unwrap(),
    )
    .await;
    assert!(saved.context_timeline().incomplete_history);
    assert!(saved.records().any(|record|matches!(record,nori_harness::TranscriptRecord::SessionEvent(SessionEvent::Acp(AcpEvent::Notification(acp::AgentNotification::SessionNotification(notification)))) if matches!(&notification.update,acp::SessionUpdate::UserMessageChunk(_)))));
    session.handle.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn equal_usage_is_zero_growth_only_when_the_agent_sent_a_fresh_reading() {
    let timeline = load(vec![
        usage(100, 1000),
        phase(10),
        usage(100, 1000),
        end(10),
        phase(11),
        end(11),
    ])
    .await
    .context_timeline();
    assert_eq!(timeline.turns[0].delta(), Some(0));
    assert_eq!(timeline.turns[1].delta(), None);
    assert_eq!(timeline.current_turn_delta(), None);
}

#[tokio::test]
#[serial_test::serial]
async fn compact_session_swap_accepts_new_session_usage_live_and_after_reload() {
    // SAFETY: mock-agent environment users run serially.
    unsafe {
        std::env::set_var(
            "MOCK_AGENT_CONTEXT_SEQUENCE",
            r#"[[{"used":100,"size":1000}],[{"used":120,"size":1000}],[{"used":40,"size":1000}]]"#,
        );
    }
    let _guard = EnvGuard(vec!["MOCK_AGENT_CONTEXT_SEQUENCE"]);
    let dir = tempfile::tempdir().expect("session directory");
    let mut session = launch(dir.path(), nori_harness::runtime::SessionStart::New).await;
    let current = started(&mut session).await;
    prompt(&mut session).await;
    session.handle.compact().await.expect("compact");
    receive_matching(&mut session, |event| {
        matches!(event, SessionEvent::Nori(NoriEvent::ContextCompacted(_)))
    })
    .await;
    assert!(
        session
            .handle
            .context_timeline()
            .await
            .expect("compacted timeline")
            .current
            .is_none()
    );
    prompt(&mut session).await;
    let live = session
        .handle
        .context_timeline()
        .await
        .expect("new session timeline");
    assert_eq!(
        live.current.as_ref().expect("new session usage").usage.used,
        40
    );
    assert_eq!(
        live.segments,
        vec![ContextSegment {
            turn: None,
            tokens: 40
        }]
    );
    assert_eq!(
        saved(
            &session,
            dir.path(),
            current.transcript_id.as_ref().unwrap()
        )
        .await
        .context_timeline(),
        live
    );
    session.handle.shutdown().await.expect("shutdown");
}

fn session_identity(id: &str) -> SessionEvent {
    SessionEvent::Nori(NoriEvent::SessionStarted(nori_protocol::SessionStarted {
        transcript_id: None,
        acp_session_id: id.to_string().into(),
        cwd: "/repo".into(),
        transcript_path: None,
        history_log_id: 0,
        history_entry_count: 0,
        config_options: Vec::new(),
    }))
}

#[tokio::test]
async fn starting_a_replacement_session_keeps_history_but_resets_old_context() {
    let timeline = load(vec![
        usage(100, 1000),
        phase(10),
        usage(130, 1000),
        end(10),
        session_identity("replacement"),
    ])
    .await
    .context_timeline();
    assert_eq!(timeline.turns[0].delta(), Some(30));
    assert!(timeline.current.is_none());
    assert!(timeline.segments.is_empty());
}

#[tokio::test]
async fn raw_fork_response_changes_usage_identity_without_requiring_a_persistent_fork_event() {
    let timeline = load(vec![
        usage(100, 1000),
        phase(10),
        usage(130, 1000),
        end(10),
        SessionEvent::Acp(AcpEvent::Response {
            request_id: 20.into(),
            response: Ok(acp::AgentResponse::ForkSessionResponse(
                acp::ForkSessionResponse::new("child"),
            )),
        }),
        phase(11),
        session_usage("child", 150, 1000),
        end(11),
    ])
    .await
    .context_timeline();
    assert_eq!(timeline.current.as_ref().unwrap().usage.used, 150);
    assert_eq!(timeline.turns[1].delta(), Some(20));
}

#[tokio::test]
#[serial_test::serial]
async fn resuming_a_crash_truncated_prompt_marks_its_end_unknown() {
    // SAFETY: mock-agent environment users run serially.
    unsafe {
        std::env::set_var("MOCK_AGENT_SUPPORT_LOAD_SESSION", "1");
    }
    let _guard = EnvGuard(vec!["MOCK_AGENT_SUPPORT_LOAD_SESSION"]);
    let transcript = load(vec![phase(10), usage(100, 1000)]).await;
    let dir = tempfile::tempdir().expect("resume directory");
    let mut session = launch(
        dir.path(),
        nori_harness::runtime::SessionStart::Resume(nori_harness::runtime::SessionResume {
            acp_session_id: Some("agent-session".into()),
            transcript: Some(transcript),
        }),
    )
    .await;
    let current = started(&mut session).await;
    let timeline = session
        .handle
        .context_timeline()
        .await
        .expect("interrupted history");
    assert_eq!(timeline.turns.len(), 1);
    assert!(
        timeline.turns[0]
            .end
            .as_ref()
            .expect("interrupted turn is no longer running")
            .observation
            .is_none()
    );
    assert!(timeline.incomplete_history);
    assert_eq!(timeline.current.as_ref().unwrap().usage.used, 100);
    assert_eq!(
        saved(
            &session,
            dir.path(),
            current.transcript_id.as_ref().unwrap()
        )
        .await
        .context_timeline(),
        timeline
    );
    session.handle.shutdown().await.expect("shutdown");
}
