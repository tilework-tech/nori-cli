//! A session on the host reads and changes its goal through the host, so
//! every terminal on that host shows the goal of the session on screen and
//! never another session's.

use super::*;
use crate::app_event_sender::HostReply;
use crate::host_client::HostCall;
use nori_protocol::GoalOp;
use pretty_assertions::assert_eq;
use serde_json::json;

fn host_session(chat: &mut ChatWidget) -> String {
    let id = nori_harness::ConversationId::new();
    chat.managed_conversation_id = Some(id);
    id.to_string()
}

fn submit(chat: &mut ChatWidget, text: &str) {
    chat.submit_user_message(text.to_string().into());
}

fn session_started(chat: &mut ChatWidget) {
    let generation = chat.session_generation;
    chat.handle_session_event(
        generation,
        nori_protocol::SessionEvent::Nori(nori_protocol::NoriEvent::SessionStarted(
            nori_protocol::SessionStarted {
                transcript_id: None,
                acp_session_id: nori_protocol::acp::v1::SessionId::new("0"),
                cwd: std::path::PathBuf::from("/workspace"),
                transcript_path: None,
                history_log_id: 0,
                history_entry_count: 0,
                config_options: Vec::new(),
            },
        )),
    );
}

fn goal(objective: &str) -> serde_json::Value {
    json!({"objective": objective, "status": "active", "tokens_used": 0,
        "time_used_seconds": 0, "created_at": 1, "updated_at": 1})
}

/// Act as the App: hand the widget's own harness actions back to it, and
/// return the goal requests that result. Nothing may reach an in-process
/// harness.
fn goal_requests(
    chat: &mut ChatWidget,
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<AppEvent>,
) -> Vec<(HostCall, HostReply)> {
    let mut requests = Vec::new();
    let mut actions = Vec::new();
    let take = |event, actions: &mut Vec<_>, requests: &mut Vec<_>| match event {
        AppEvent::HostRequest { call, reply }
            if matches!(call, HostCall::GetGoal { .. } | HostCall::SetGoal { .. }) =>
        {
            requests.push((call, reply));
        }
        AppEvent::HarnessAction(action) => actions.push(action),
        AppEvent::HarnessActionFailed(message) => panic!("{message}"),
        _ => {}
    };
    while let Ok(event) = rx.try_recv() {
        take(event, &mut actions, &mut requests);
    }
    for action in std::mem::take(&mut actions) {
        chat.submit_harness_action(action);
    }
    while let Ok(event) = rx.try_recv() {
        take(event, &mut actions, &mut requests);
    }
    assert!(actions.is_empty(), "sent {actions:?} in process");
    requests
}

/// Answer `reply` with `result` and hand the widget the goal the App would
/// route back to whichever widget is on screen.
async fn answer(
    chat: &mut ChatWidget,
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<AppEvent>,
    reply: HostReply,
    result: serde_json::Value,
) {
    reply.deliver(Ok(result));
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            match rx.recv().await {
                Some(AppEvent::HostGoalLoaded {
                    session_id,
                    goal,
                    asked,
                }) => return chat.on_host_goal_loaded(&session_id, goal, asked),
                Some(_) => {}
                None => panic!("the widget stopped sending"),
            }
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn goal_commands_change_the_goal_of_the_session_on_screen_through_the_host() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    let session = host_session(&mut chat);

    let mut sent = Vec::new();
    for command in [
        "/goal ship the lantern",
        "/goal pause",
        "/goal resume",
        "/goal clear",
    ] {
        submit(&mut chat, command);
        sent.extend(
            goal_requests(&mut chat, &mut rx)
                .into_iter()
                .map(|(call, _)| call),
        );
    }

    let ops: Vec<(String, GoalOp)> = sent
        .into_iter()
        .map(|call| match call {
            HostCall::SetGoal { session_id, op } => (session_id, op),
            other => panic!("unexpected {other:?}"),
        })
        .collect();
    assert_eq!(
        ops,
        vec![
            (
                session.clone(),
                GoalOp::Set {
                    objective: "ship the lantern".into()
                }
            ),
            (session.clone(), GoalOp::Pause),
            (session.clone(), GoalOp::Resume),
            (session, GoalOp::Clear),
        ]
    );
}

#[tokio::test]
async fn a_session_shown_on_screen_takes_its_goal_from_the_host() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    let session = host_session(&mut chat);

    session_started(&mut chat);
    let mut requests = goal_requests(&mut chat, &mut rx);
    assert_eq!(requests.len(), 1);
    let (call, reply) = requests.pop().unwrap();
    assert!(
        matches!(&call, HostCall::GetGoal { session_id } if *session_id == session),
        "{call:?}"
    );
    answer(
        &mut chat,
        &mut rx,
        reply,
        json!({"goal": goal("tidy the pantry")}),
    )
    .await;
    submit(&mut chat, "/goal edit");

    assert_eq!(chat.composer_text(), "/goal tidy the pantry");
    assert!(
        goal_requests(&mut chat, &mut rx).is_empty(),
        "the goal the host gave is the one edited"
    );
}

#[tokio::test]
async fn a_goal_for_another_session_is_not_shown() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    host_session(&mut chat);
    let other = nori_harness::ConversationId::new().to_string();
    let goal: nori_protocol::ThreadGoal =
        serde_json::from_value(goal("not this session's")).unwrap();

    chat.on_host_goal_loaded(&other, Some(goal), true);
    submit(&mut chat, "/goal edit");

    assert_eq!(chat.composer_text(), "");
    let requests = goal_requests(&mut chat, &mut rx);
    assert!(
        matches!(requests.as_slice(), [(HostCall::GetGoal { .. }, _)]),
        "editing asks the host for this session's goal"
    );
}

#[tokio::test]
async fn asking_for_the_goal_shows_the_hosts_answer() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    host_session(&mut chat);

    submit(&mut chat, "/goal");
    let mut requests = goal_requests(&mut chat, &mut rx);
    let (_, reply) = requests.pop().unwrap();
    answer(
        &mut chat,
        &mut rx,
        reply,
        json!({"goal": goal("label the jars")}),
    )
    .await;

    let history = drain_insert_history(&mut rx)
        .into_iter()
        .map(|lines| lines_to_single_string(&lines))
        .collect::<String>();
    assert!(history.contains("label the jars"), "{history}");
}

#[tokio::test]
async fn a_live_goal_change_is_not_undone_by_an_older_answer() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    host_session(&mut chat);
    session_started(&mut chat);
    let (_, reply) = goal_requests(&mut chat, &mut rx).pop().unwrap();
    let generation = chat.session_generation;
    let live: nori_protocol::ThreadGoal =
        serde_json::from_value(goal("water the orchard")).unwrap();
    chat.handle_session_event(
        generation,
        nori_protocol::SessionEvent::Nori(nori_protocol::NoriEvent::GoalChanged(Some(live))),
    );

    answer(
        &mut chat,
        &mut rx,
        reply,
        json!({"goal": goal("tidy the pantry")}),
    )
    .await;
    while rx.try_recv().is_ok() {}
    submit(&mut chat, "/goal edit");

    assert_eq!(chat.composer_text(), "/goal water the orchard");
}

#[tokio::test]
async fn a_goal_status_the_host_cannot_set_is_refused_rather_than_run_here() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    host_session(&mut chat);
    while rx.try_recv().is_ok() {}

    chat.submit_harness_action(crate::app_event::HarnessAction::SetGoalStatus(
        nori_protocol::ThreadGoalStatus::Complete,
    ));

    let mut seen = Vec::new();
    while let Ok(event) = rx.try_recv() {
        match event {
            AppEvent::HostRequest { call, .. } => seen.push(format!("{call:?}")),
            AppEvent::HarnessAction(action) => seen.push(format!("{action:?}")),
            AppEvent::HarnessActionFailed(message) => seen.push(message),
            AppEvent::SessionHistory { event, .. } => {
                if let AppEvent::HarnessActionFailed(message) = *event {
                    seen.push(message);
                }
            }
            _ => {}
        }
    }
    assert_eq!(
        seen,
        vec!["The host sets a goal only to active or paused.".to_string()]
    );
}

#[tokio::test]
async fn a_watcher_reads_the_goal_but_cannot_change_it() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    host_session(&mut chat);
    chat.set_host_connection(crate::host_client::HostConnection {
        role: nori_protocol::ControlRole::Watcher,
        connected: true,
        stopped: None,
        features: crate::host_client::HostFeatures {
            goal: true,
            ..crate::host_client::HostFeatures::default()
        },
        agents: Vec::new(),
    });
    while rx.try_recv().is_ok() {}

    chat.submit_harness_action(crate::app_event::HarnessAction::LoadGoal);
    chat.submit_harness_action(crate::app_event::HarnessAction::ClearGoal);

    let mut seen = Vec::new();
    while let Ok(event) = rx.try_recv() {
        match event {
            AppEvent::HostRequest { call, .. } => seen.push(format!("{call:?}")),
            AppEvent::HarnessActionFailed(message) => seen.push(message),
            AppEvent::SessionHistory { event, .. } => {
                if let AppEvent::HarnessActionFailed(message) = *event {
                    seen.push(message);
                }
            }
            _ => {}
        }
    }
    assert_eq!(seen.len(), 2, "{seen:?}");
    assert!(seen[0].starts_with("GetGoal"), "{seen:?}");
    assert!(seen[1].starts_with("Watching"), "{seen:?}");
}
