#[cfg(unix)]
#[tokio::test]
#[serial]
async fn closing_by_the_failed_id_after_a_resume_closes_the_resumed_session() {
    let home = tempfile::tempdir().expect("temp dir");
    let wire = home.path().join("wire");
    let manager = SessionManager::new(SessionManagerConfig::default());
    let mut failed = start(
        &manager,
        prepare_spec("mock-model", home.path(), Some(wire.clone())),
    );
    wait_for_status(&manager, &failed.conversation_id, SessionStatus::Ready).await;
    let request = failed
        .handle
        .prompt(prompt_text("hello"))
        .await
        .expect("prompt");
    collect_turn(&mut failed.events, &request).await;
    // SAFETY: sends SIGKILL to the mock agent process this test started.
    let killed = unsafe { libc::kill(recorded_agent_pid(&wire), libc::SIGKILL) };
    assert_eq!(killed, 0, "kill the agent process");
    wait_for_status(&manager, &failed.conversation_id, SessionStatus::Failed).await;
    let resumed = manager
        .resume(
            &failed.conversation_id,
            prepare_spec("mock-model", home.path(), None),
        )
        .await
        .expect("resume");
    wait_for_status(&manager, &resumed.conversation_id, SessionStatus::Ready).await;

    manager
        .close(&failed.conversation_id, Duration::ZERO)
        .await
        .expect("close by the failed session's ID");

    wait_for_status(&manager, &resumed.conversation_id, SessionStatus::Closed).await;
    manager.shutdown(Duration::ZERO).await;
}

#[expect(
    clippy::expect_used,
    reason = "test helper fails at the transcript-flush and crash-admission boundaries"
)]
async fn crash(manager: &SessionManager, session: &ManagedSession) {
    session
        .handle
        .flush_transcript()
        .await
        .expect("flush before crash");
    session
        .handle
        .prompt(prompt_text("mock:exit-during-prompt"))
        .await
        .expect("admit crashing prompt");
    wait_for_status(manager, &session.conversation_id, SessionStatus::Failed).await;
}

#[expect(
    clippy::expect_used,
    reason = "test helper fails at the history-replay boundary"
)]
async fn collect_replay(session: &mut ManagedSession) -> Vec<String> {
    tokio::time::timeout(WAIT, async {
        let mut text = Vec::new();
        loop {
            let envelope = session.events.recv().await.expect("resume stream open");
            assert_eq!(
                envelope.conversation_id,
                session.conversation_id.to_string()
            );
            text.extend(agent_text(&envelope.event));
            if matches!(
                envelope.event,
                SessionEvent::Nori(NoriEvent::ReplayFinished)
            ) {
                return text;
            }
        }
    })
    .await
    .expect("history replay completes")
}

#[tokio::test]
#[serial]
async fn repeated_failed_resumes_keep_identity_family_order_and_history() {
    let _response = EnvGuard::set("MOCK_AGENT_RESPONSE_MOCK_MODEL", "first incarnation reply");
    let home = tempfile::tempdir().expect("temp dir");
    let manager = SessionManager::new(SessionManagerConfig::default());
    let mut session = start(&manager, prepare_spec("mock-model", home.path(), None));
    let id = session.conversation_id;
    wait_for_status(&manager, &id, SessionStatus::Ready).await;
    let child = manager
        .start(StartSession {
            agent: SessionAgent::Prepare(prepare_spec("mock-model-alt", home.path(), None)),
            start: SessionStart::New,
            parent: Some(id),
            source: SessionSource::Session,
        })
        .expect("child starts");
    wait_for_status(&manager, &child.conversation_id, SessionStatus::Ready).await;
    let initial_path = manager.get(&id).expect("initial record").transcript_path;
    let first = session
        .handle
        .prompt(prompt_text("original question"))
        .await
        .expect("first prompt");
    collect_turn(&mut session.events, &first).await;
    let mut expected_history = vec!["first incarnation reply".to_string()];
    for response in ["second incarnation reply", "third incarnation reply"] {
        crash(&manager, &session).await;
        let before = manager.replay_info(&id).expect("old cursor").latest_seq;
        let mut follower = manager
            .subscribe_from(Some(id), &HashMap::from([(id.to_string(), before)]), 4096)
            .expect("subscribe while failed");
        assert!(follower.replay.is_empty());
        let _response = EnvGuard::set("MOCK_AGENT_RESPONSE_MOCK_MODEL", response);
        let mut resumed = manager
            .resume(&id, prepare_spec("mock-model", home.path(), None))
            .await
            .expect("resume failed session");
        assert_eq!(resumed.conversation_id, id);
        assert_eq!(collect_replay(&mut resumed).await, expected_history);
        wait_for_status(&manager, &id, SessionStatus::Ready).await;
        let request = resumed
            .handle
            .prompt(prompt_text("continue"))
            .await
            .expect("resumed prompt");
        let turn = collect_turn(&mut resumed.events, &request).await;
        let through = turn.last().expect("turn has events").seq;
        let seqs = tokio::time::timeout(WAIT, async {
            let mut seqs = Vec::new();
            loop {
                let event = follower
                    .events
                    .recv()
                    .await
                    .expect("original ID subscriber stays open");
                assert_eq!(event.conversation_id, id.to_string());
                seqs.push(event.seq);
                if event.seq == through {
                    return seqs;
                }
            }
        })
        .await
        .expect("subscriber receives resumed turn");
        assert_eq!(seqs, ((before + 1)..=through).collect::<Vec<_>>());
        assert_eq!(
            manager
                .list()
                .into_iter()
                .map(|record| (record.conversation_id, record.parent))
                .collect::<Vec<_>>(),
            vec![(id, None), (child.conversation_id, Some(id))]
        );
        assert_eq!(
            manager.get(&id).expect("resumed record").transcript_path,
            initial_path
        );
        resumed
            .handle
            .flush_transcript()
            .await
            .expect("flush resumed history");
        expected_history.push(response.to_string());
        let loader = nori_harness::transcript::TranscriptLoader::new(home.path().to_path_buf());
        let sessions = loader
            .find_session_metadata_for_cwd(home.path())
            .await
            .expect("list stored sessions");
        assert_eq!(
            sessions
                .iter()
                .filter(|entry| entry.session_id == id.to_string())
                .count(),
            1
        );
        let path = initial_path.as_ref().expect("transcript path");
        let transcript = nori_harness::transcript::load_transcript_from_path(path)
            .await
            .expect("load continued transcript");
        assert_eq!(transcript.meta.session_id, id.to_string());
        let stored_text = transcript
            .records()
            .filter_map(|record| match record {
                nori_harness::transcript::TranscriptRecord::SessionEvent(event) => {
                    agent_text(event)
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            stored_text, expected_history,
            "history survives each restart exactly once"
        );
        session = resumed;
    }
    manager.shutdown(Duration::ZERO).await;
}

#[tokio::test]
#[serial]
async fn concurrent_resumes_admit_only_one_replacement_for_the_same_id() {
    let home = tempfile::tempdir().expect("temp dir");
    let manager = SessionManager::new(SessionManagerConfig::default());
    let session = start(&manager, prepare_spec("mock-model", home.path(), None));
    let id = session.conversation_id;
    wait_for_status(&manager, &id, SessionStatus::Ready).await;
    crash(&manager, &session).await;
    let (first, second) = tokio::join!(
        manager.resume(&id, prepare_spec("mock-model", home.path(), None)),
        manager.resume(&id, prepare_spec("mock-model", home.path(), None)),
    );
    let successful = [first, second]
        .into_iter()
        .filter_map(Result::ok)
        .collect::<Vec<_>>();
    assert_eq!(successful.len(), 1, "one restart owns the failed session");
    assert_eq!(successful[0].conversation_id, id);
    wait_for_status(&manager, &id, SessionStatus::Ready).await;
    assert_eq!(manager.list().len(), 1);
    assert_eq!(manager.free_slots(), manager.max_sessions() - 1);
    manager.shutdown(Duration::ZERO).await;
}

#[tokio::test]
#[serial]
async fn failed_resume_loads_the_original_acp_id_when_supported() {
    let _load = EnvGuard::set("MOCK_AGENT_SUPPORT_LOAD_SESSION", "1");
    let _replay = EnvGuard::set("MOCK_AGENT_LOAD_SESSION_NOTIFICATION_COUNT", "2");
    let home = tempfile::tempdir().expect("temp dir");
    let manager = SessionManager::new(SessionManagerConfig::default());
    let session = start(&manager, prepare_spec("mock-model", home.path(), None));
    let id = session.conversation_id;
    wait_for_status(&manager, &id, SessionStatus::Ready).await;
    let acp_id = manager.get(&id).expect("initial record").acp_session_id;
    crash(&manager, &session).await;
    let wire = home.path().join("resumed-wire");
    let mut resumed = manager
        .resume(
            &id,
            prepare_spec("mock-model", home.path(), Some(wire.clone())),
        )
        .await
        .expect("load failed session");
    assert_eq!(resumed.conversation_id, id);
    assert_eq!(
        collect_replay(&mut resumed).await,
        vec!["replay chunk 0".to_string(), "replay chunk 1".to_string()]
    );
    wait_for_status(&manager, &id, SessionStatus::Ready).await;
    assert_eq!(
        manager.get(&id).expect("resumed record").acp_session_id,
        acp_id
    );
    manager.shutdown(Duration::ZERO).await;
    let messages = std::fs::read_dir(&wire)
        .expect("wire directory")
        .map(|entry| entry.expect("wire entry").path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "jsonl")
        })
        .flat_map(|path| {
            std::fs::read_to_string(path)
                .expect("wire log")
                .lines()
                .map(|line| serde_json::from_str::<serde_json::Value>(line).expect("wire JSON"))
                .collect::<Vec<_>>()
        })
        .filter(|record| record["direction"] == "client_to_agent")
        .map(|record| record["message"].clone())
        .collect::<Vec<_>>();
    let load = messages
        .iter()
        .find(|message| message["method"] == "session/load")
        .expect("uses ACP load");
    assert_eq!(load["params"]["sessionId"], acp_id.expect("initial ACP ID"));
    assert!(
        !messages
            .iter()
            .any(|message| message["method"] == "session/new")
    );
}
