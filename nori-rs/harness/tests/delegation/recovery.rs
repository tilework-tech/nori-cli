#[tokio::test]
#[serial]
async fn forking_a_child_preserves_delegation_depth() {
    let _http = EnvGuard::set("MOCK_AGENT_MCP_HTTP", "1");
    let _fork = EnvGuard::set("MOCK_AGENT_SUPPORT_SESSION_FORK", "1");
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
    let _fork = EnvGuard::set("MOCK_AGENT_SUPPORT_SESSION_FORK", "1");
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
    let _reply = EnvGuard::set("MOCK_AGENT_RESPONSE", "durable child payload");
    let delay = EnvGuard::set("MOCK_AGENT_DELAY_MS", "5000");
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
    let _echo = EnvGuard::set("MOCK_AGENT_ECHO_PROMPT", "1");
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

#[tokio::test]
#[serial]
async fn cold_import_keeps_delegation_identity_and_terminal_state_without_launching_a_process() {
    let _echo = EnvGuard::set("MOCK_AGENT_ECHO_PROMPT", "1");
    let old_home = tempfile::tempdir().unwrap();
    let m = manager(2);
    let mut parent = start(&m, old_home.path(), true);
    status(&m, &parent.conversation_id, SessionStatus::Ready).await;
    let delegation = m
        .delegate(
            &parent.conversation_id,
            "mock-model".into(),
            "inspect synthetic sample".into(),
        )
        .await
        .unwrap();
    status(&m, &id(&delegation), SessionStatus::Done).await;
    let done = terminal(&m, &parent.conversation_id, &delegation.request_id).await;
    wait_parent_reply(&mut parent, "inspect synthetic sample").await;
    status(&m, &parent.conversation_id, SessionStatus::Ready).await;
    let child_checkpoint = m.checkpoint_session(&id(&delegation), false).await.unwrap();
    assert!(child_checkpoint.private_transcript.is_none());
    let parent_checkpoint = m
        .checkpoint_session(&parent.conversation_id, true)
        .await
        .unwrap();
    assert!(
        parent_checkpoint
            .private_transcript
            .as_deref()
            .unwrap()
            .contains("inspect synthetic sample")
    );
    m.shutdown(Duration::ZERO).await;
    drop(old_home);
    let new_home = tempfile::tempdir().unwrap();
    let fresh = manager(2);
    let policy = nori_config::AskForApproval::Never;
    fresh
        .import_checkpoint(&child_checkpoint, new_home.path(), policy)
        .unwrap();
    fresh
        .import_checkpoint(&parent_checkpoint, new_home.path(), policy)
        .unwrap();
    let child = fresh.get(&id(&delegation)).unwrap();
    assert_eq!(
        (child.parent, child.source, child.status),
        (
            Some(parent.conversation_id),
            SessionSource::Session,
            SessionStatus::Done
        )
    );
    assert_eq!(fresh.delegations(&parent.conversation_id), vec![done]);
    assert_eq!(fresh.free_slots(), 2);
    assert_eq!(
        fresh.get(&parent.conversation_id).unwrap().status,
        SessionStatus::Failed
    );
    assert!(fresh.handle(&parent.conversation_id).is_none());
    assert!(
        fresh
            .import_checkpoint(&parent_checkpoint, new_home.path(), policy)
            .is_err()
    );
    fresh.shutdown(Duration::ZERO).await;
}
