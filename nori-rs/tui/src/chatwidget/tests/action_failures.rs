use super::*;
use crate::app_event::HarnessAction;
use nori_harness::session_manager::ManagedSession;
use nori_harness::session_manager::SessionAgent;
use nori_harness::session_manager::SessionSource;
use nori_harness::session_manager::StartSession;
use nori_protocol::NoriEvent;
use nori_protocol::SessionEvent;
use std::sync::Arc;
use std::time::Duration;

async fn started_session(
    chat: &mut ChatWidget,
    home: &std::path::Path,
) -> (ManagedSession, SessionEvent) {
    let mut session = chat
        .session_manager
        .start(StartSession {
            agent: SessionAgent::Prepare(nori_harness::runtime::AgentPrepareSpec {
                config: Arc::new(nori_config::NoriConfig {
                    active_agent: "mock-model".into(),
                    cwd: home.into(),
                    nori_home: home.into(),
                    multi_session: true,
                    ..Default::default()
                }),
                cli_version: "action-failure-test".into(),
                session_context: None,
                initial_context: None,
            }),
            start: nori_harness::runtime::SessionStart::New,
            parent: None,
            source: SessionSource::User,
        })
        .expect("start real mock process");
    let started = tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let event = session.events.recv().await.expect("session stream").event;
            if matches!(event, SessionEvent::Nori(NoriEvent::SessionStarted(_))) {
                break event;
            }
        }
    })
    .await
    .expect("mock starts");
    chat.managed_conversation_id = Some(session.conversation_id);
    chat.harness_handle = Some(session.handle.clone());
    (session, started)
}

async fn close_session(session: &mut ManagedSession) {
    session.handle.shutdown().await.expect("normal shutdown");
    tokio::time::timeout(Duration::from_secs(15), async {
        while session.events.recv().await.is_some() {}
    })
    .await
    .expect("process and event stream close");
}

fn action_error(event: &AppEvent) -> bool {
    match event {
        AppEvent::HarnessActionFailed(_) => true,
        AppEvent::SessionHistory { event, .. } => action_error(event),
        _ => false,
    }
}

// Replay can arrive after the manager releases the child process. Both adoption
// schedules (retained old handle and no live handle) must remain quiet.
async fn completed_child_replay(keep_handle: bool) {
    let home = tempfile::tempdir().unwrap();
    let (mut chat, mut rx, _) = make_chatwidget_manual();
    let (mut session, started) = started_session(&mut chat, home.path()).await;
    close_session(&mut session).await;
    assert!(
        chat.session_manager
            .handle(&session.conversation_id)
            .is_none()
    );
    assert!(
        session.handle.custom_prompts().await.is_err(),
        "closed transport reproduces the race"
    );
    chat.delegated_child = true;
    if !keep_handle {
        chat.harness_handle = None;
    }
    while rx.try_recv().is_ok() {}
    chat.handle_session_event(chat.session_generation(), started);
    // Closing the widget's sender leaves only actual action tasks owning
    // senders. EOF is the deterministic task-completion barrier.
    drop(chat);
    tokio::time::timeout(Duration::from_secs(15), async {
        while let Some(event) = rx.recv().await {
            assert!(
                !action_error(&event),
                "successful child teardown emitted {event:?}; retained handle={keep_handle}"
            );
        }
    })
    .await
    .expect("all action tasks finish");
}

#[tokio::test]
async fn completed_child_retained_handle_replay_does_not_report_action_failure() {
    completed_child_replay(true).await;
}

#[tokio::test]
async fn completed_child_without_handle_replay_does_not_report_action_failure() {
    completed_child_replay(false).await;
}

#[tokio::test]
async fn visible_session_real_action_failure_is_rendered() {
    let home = tempfile::tempdir().unwrap();
    let (mut chat, mut rx, _) = make_chatwidget_manual();
    let (mut session, _) = started_session(&mut chat, home.path()).await;
    while rx.try_recv().is_ok() {}
    let session_generation = chat.session_generation();
    chat.app_event_tx = chat.app_event_tx.for_session(session_generation);
    chat.submit_harness_action(HarnessAction::Branch);
    let message = tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            match rx.recv().await.expect("action event") {
                AppEvent::SessionHistory { generation, event }
                    if generation == session_generation =>
                {
                    if let AppEvent::HarnessActionFailed(message) = *event {
                        break message;
                    }
                }
                _ => {}
            }
        }
    })
    .await
    .expect("real failed operation remains visible");
    let mut deck = Default::default();
    crate::app::route_session_history_event(
        &mut chat,
        &mut deck,
        None,
        session_generation,
        AppEvent::HarnessActionFailed(message),
    );
    let mut text = String::new();
    while let Ok(event) = rx.try_recv() {
        if let AppEvent::SessionHistory { generation, event } = event
            && let Some(AppEvent::InsertHistoryCell(cell)) = crate::app::route_session_history_event(
                &mut chat, &mut deck, None, generation, *event,
            )
        {
            text.push_str(&lines_to_single_string(&cell.display_lines(80)));
        }
    }
    assert!(
        !text.trim().is_empty(),
        "visible failure must produce history"
    );
    insta::assert_snapshot!("visible_session_action_failure", text);
    close_session(&mut session).await;
}

#[tokio::test]
async fn shutdown_classification_preserves_real_backend_failures() {
    let home = tempfile::tempdir().unwrap();
    let (mut chat, _, _) = make_chatwidget_manual();
    let (mut session, _) = started_session(&mut chat, home.path()).await;
    let backend_error = session
        .handle
        .branch()
        .await
        .expect_err("unsupported branch is a real failure");
    assert!(!session.handle.is_expected_shutdown_error(&backend_error));
    close_session(&mut session).await;
    assert!(
        !session.handle.is_expected_shutdown_error(&backend_error),
        "shutdown cannot erase an already returned real error"
    );
    let transport_error = session
        .handle
        .custom_prompts()
        .await
        .expect_err("closed transport");
    assert!(session.handle.is_expected_shutdown_error(&transport_error));
}

#[tokio::test]
async fn crashed_session_action_failure_is_not_normal_shutdown() {
    let home = tempfile::tempdir().unwrap();
    let (mut chat, _, _) = make_chatwidget_manual();
    let (mut session, _) = started_session(&mut chat, home.path()).await;
    session
        .handle
        .prompt(vec![nori_protocol::acp::v1::ContentBlock::Text(
            nori_protocol::acp::v1::TextContent::new("mock:exit-during-prompt"),
        )])
        .await
        .expect("submit crash marker");
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let event = session.events.recv().await.expect("crash event").event;
            if matches!(event, SessionEvent::Nori(NoriEvent::SessionEnded(_))) {
                break;
            }
        }
    })
    .await
    .expect("process reports its crash");
    let error = tokio::time::timeout(
        Duration::from_secs(15),
        session.handle.prompt_with_meta_if_idle(
            vec![nori_protocol::acp::v1::ContentBlock::Text(
                nori_protocol::acp::v1::TextContent::new("after crash"),
            )],
            None,
        ),
    )
    .await
    .expect("failed process answers action")
    .expect_err("crashed transport");
    assert!(
        !session.handle.is_expected_shutdown_error(&error),
        "a crash must remain a user-facing failure"
    );
}
