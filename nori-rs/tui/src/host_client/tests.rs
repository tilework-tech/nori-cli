use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use tokio::sync::mpsc;

use super::ConnectionEvent;
use super::HostCall;
use super::HostEvent;
use super::HostSessionClient;

fn client() -> (
    HostSessionClient<&'static str>,
    mpsc::UnboundedReceiver<Value>,
) {
    let (tx, rx) = mpsc::unbounded_channel();
    (HostSessionClient::new(tx), rx)
}

fn sent(rx: &mut mpsc::UnboundedReceiver<Value>) -> Vec<Value> {
    std::iter::from_fn(|| rx.try_recv().ok()).collect()
}

#[test]
fn a_reply_settles_the_request_it_answers() {
    let (mut client, mut rx) = client();
    client.request(HostCall::List, "list").unwrap();
    client.request(HostCall::TakeControl, "take").unwrap();
    let outgoing = sent(&mut rx);
    assert_eq!(outgoing[0]["method"], "_nori/session/list");
    assert_eq!(outgoing[1]["method"], "_nori/control/take");
    assert!(client.has_pending(|tag| *tag == "list"));

    let reply = client.incoming(ConnectionEvent::Message(
        json!({"jsonrpc":"2.0","id":outgoing[1]["id"],"result":{"role":"controller"}}),
    ));
    assert!(matches!(
        reply,
        HostEvent::Reply { tag: "take", result: Ok(result) } if result["role"] == "controller"
    ));
    let failed = client.incoming(ConnectionEvent::Message(
        json!({"jsonrpc":"2.0","id":outgoing[0]["id"],"error":{"code":-32016,"message":"watching"}}),
    ));
    assert!(matches!(
        failed,
        HostEvent::Reply { tag: "list", result: Err(error) } if error.message.as_deref() == Some("watching")
    ));
    assert!(!client.has_pending(|tag| *tag == "list"));
    let repeated = client.incoming(ConnectionEvent::Message(
        json!({"jsonrpc":"2.0","id":outgoing[0]["id"],"result":{}}),
    ));
    assert!(matches!(repeated, HostEvent::Ignored));
}

#[test]
fn a_request_from_the_host_never_settles_a_pending_call() {
    let (mut client, mut rx) = client();
    client.request(HostCall::List, "list").unwrap();
    let id = sent(&mut rx)[0]["id"].clone();

    let request = client.incoming(ConnectionEvent::Message(json!({
        "jsonrpc":"2.0","id":id,"method":"session/request_permission","params":{}
    })));
    assert!(matches!(request, HostEvent::Ignored));
    let reply = client.incoming(ConnectionEvent::Message(
        json!({"jsonrpc":"2.0","id":id,"result":{"sessions":[]}}),
    ));
    assert!(matches!(reply, HostEvent::Reply { tag: "list", .. }));
}

#[test]
fn session_events_and_role_changes_are_surfaced() {
    let (mut client, _rx) = client();
    let event = client.incoming(ConnectionEvent::Message(json!({
        "jsonrpc":"2.0","method":"_nori/session/event","params":{"conversation_id":"one"}
    })));
    assert!(matches!(event, HostEvent::Session(params) if params["conversation_id"] == "one"));
    let role = client.incoming(ConnectionEvent::Message(json!({
        "jsonrpc":"2.0","method":"_nori/control/changed","params":{"role":"controller"}
    })));
    assert!(matches!(role, HostEvent::RoleChanged(role) if role == "controller"));
    let defaulted = client.incoming(ConnectionEvent::Message(json!({
        "jsonrpc":"2.0","method":"_nori/control/changed","params":{}
    })));
    assert!(matches!(defaulted, HostEvent::RoleChanged(role) if role == "watcher"));
}

#[test]
fn a_dropped_connection_returns_every_unanswered_call() {
    let (mut client, mut rx) = client();
    client.request(HostCall::List, "list").unwrap();
    client
        .request(
            HostCall::Close {
                session_id: "one".into(),
            },
            "close",
        )
        .unwrap();
    let ids: Vec<Value> = sent(&mut rx).into_iter().map(|m| m["id"].clone()).collect();

    let HostEvent::Disconnected {
        reason,
        mut unanswered,
    } = client.incoming(ConnectionEvent::Disconnected("socket closed".into()))
    else {
        panic!("expected a disconnect");
    };
    unanswered.sort_unstable();
    assert_eq!(reason, "socket closed");
    assert_eq!(unanswered, vec!["close", "list"]);
    let late = client.incoming(ConnectionEvent::Message(
        json!({"jsonrpc":"2.0","id":ids[1],"result":{}}),
    ));
    assert!(matches!(late, HostEvent::Ignored));
}

#[test]
fn a_stopped_connection_sends_nothing_more() {
    let (mut client, mut rx) = client();
    client.request(HostCall::List, "list").unwrap();
    let id = sent(&mut rx)[0]["id"].clone();
    let stopped = client.incoming(ConnectionEvent::Stopped("Host restarted".into()));
    assert!(matches!(stopped, HostEvent::Stopped(reason) if reason == "Host restarted"));
    let late = client.incoming(ConnectionEvent::Message(
        json!({"jsonrpc":"2.0","id":id,"result":{}}),
    ));
    assert!(matches!(late, HostEvent::Ignored));

    assert!(client.request(HostCall::List, "list").is_err());
    assert!(sent(&mut rx).is_empty());
}

#[test]
fn resume_uses_the_session_folder_or_falls_back_for_older_hosts() {
    let (mut client, mut rx) = client();
    client
        .request(
            HostCall::Resume {
                session_id: "one".into(),
                cwd: Some("/work/parser".into()),
            },
            "resume",
        )
        .unwrap();
    client
        .request(
            HostCall::Resume {
                session_id: "two".into(),
                cwd: None,
            },
            "resume",
        )
        .unwrap();
    let outgoing = sent(&mut rx);
    assert_eq!(outgoing[0]["method"], "session/resume");
    assert_eq!(
        outgoing[0]["params"],
        json!({"sessionId":"one","cwd":"/work/parser","mcpServers":[]})
    );
    assert_eq!(outgoing[1]["method"], "_nori/session/resume");
    assert_eq!(outgoing[1]["params"], json!({"session_id":"two"}));
}

#[test]
fn a_closed_worker_reports_the_call_as_unsent() {
    let (mut client, rx) = client();
    drop(rx);
    assert!(client.request(HostCall::List, "list").is_err());
    assert!(!client.has_pending(|tag| *tag == "list"));
    let HostEvent::Disconnected { unanswered, .. } =
        client.incoming(ConnectionEvent::Disconnected("gone".into()))
    else {
        panic!("expected a disconnect");
    };
    assert!(unanswered.is_empty());
}

#[test]
fn every_call_reaches_the_host_method_it_names() {
    let (mut client, mut rx) = client();
    let response = nori_protocol::acp::v1::RequestPermissionResponse::new(
        nori_protocol::acp::v1::RequestPermissionOutcome::Cancelled,
    );
    let calls = vec![
        HostCall::Initialize,
        HostCall::Subscribe {
            session_id: "one".into(),
            cursor: 7,
        },
        HostCall::NewSession(json!({"agent":"claude"})),
        HostCall::Prompt(json!({"session_id":"one"})),
        HostCall::FollowUp {
            session_id: "one".into(),
            expected_turn: nori_protocol::acp::v1::RequestId::Number(3),
            message_id: "m1".into(),
            intent: nori_harness::FollowUpIntent::Queue,
            prompt: vec![nori_protocol::acp::v1::ContentBlock::Text(
                nori_protocol::acp::v1::TextContent::new("and the docs"),
            )],
        },
        HostCall::CancelObserved {
            session_id: "one".into(),
            observed_turn_id: "t1".into(),
        },
        HostCall::Respond {
            session_id: "one".into(),
            request_id: json!("r1"),
            response: response.clone(),
        },
        HostCall::Delegate {
            session_id: "one".into(),
            agent: "codex".into(),
            task: "fix the parser".into(),
        },
        HostCall::ResolveDelegation {
            session_id: "one".into(),
            request_id: json!("d1"),
            approved: true,
        },
    ];
    for (tag, call) in calls.into_iter().enumerate() {
        client
            .request(call, ["a", "b", "c", "d", "e", "f", "g", "h", "i"][tag])
            .unwrap();
    }
    let outgoing: Vec<(Value, Value)> = sent(&mut rx)
        .into_iter()
        .map(|message| {
            assert_eq!(message["jsonrpc"], "2.0");
            (message["method"].clone(), message["params"].clone())
        })
        .collect();
    assert_eq!(
        outgoing,
        vec![
            (
                json!("initialize"),
                json!({"capabilities":{"nori_sessions":{"approval_modes":true}}})
            ),
            (
                json!("_nori/session/subscribe"),
                json!({"session_id":"one","cursors":{"one":7}})
            ),
            (json!("_nori/session/new"), json!({"agent":"claude"})),
            (json!("_nori/session/prompt"), json!({"session_id":"one"})),
            (
                json!("_nori/session/follow_up"),
                json!({"session_id":"one","expected_turn":3,"message_id":"m1","intent":"queue",
                    "prompt":[{"type":"text","text":"and the docs"}]})
            ),
            (
                json!("_nori/session/cancel_observed"),
                json!({"sessionId":"one","observedTurnId":"t1"})
            ),
            (
                json!("_nori/session/respond"),
                json!({"session_id":"one","request_id":"r1","response":response})
            ),
            (
                json!("_nori/session/delegate"),
                json!({"session_id":"one","agent":"codex","task":"fix the parser"})
            ),
            (
                json!("_nori/session/delegation/resolve"),
                json!({"session_id":"one","request_id":"d1","approved":true})
            ),
        ]
    );
}

#[test]
fn prepared_agent_calls_use_the_registered_host_methods() {
    let (mut client, mut rx) = client();
    let folder = std::path::PathBuf::from("/srv/harbor");
    let calls = vec![
        HostCall::PrepareAgent {
            agent: "birch".into(),
            cwd: folder.clone(),
        },
        HostCall::ListPrepared {
            selection: "sel-1".into(),
            cwd: folder.clone(),
        },
        HostCall::ActivatePrepared {
            selection: "sel-1".into(),
            cwd: folder.clone(),
            saved_session_id: Some("saved-1".into()),
        },
        HostCall::ActivatePrepared {
            selection: "sel-2".into(),
            cwd: folder,
            saved_session_id: None,
        },
        HostCall::DiscardPrepared {
            selection: "sel-3".into(),
        },
    ];
    for (tag, call) in calls.into_iter().enumerate() {
        client
            .request(call, ["a", "b", "c", "d", "e"][tag])
            .unwrap();
    }
    let outgoing: Vec<(Value, Value)> = sent(&mut rx)
        .into_iter()
        .map(|message| (message["method"].clone(), message["params"].clone()))
        .collect();
    assert_eq!(
        outgoing,
        vec![
            (
                json!("_nori/host/prepare_agent"),
                json!({"agent":"birch","cwd":"/srv/harbor"})
            ),
            (
                json!("session/list"),
                json!({"cwd":"/srv/harbor","_meta":{"nori/preparedAgent":{"id":"sel-1"}}})
            ),
            (
                json!("session/new"),
                json!({"cwd":"/srv/harbor","mcpServers":[],
                    "_meta":{"nori/preparedAgent":{"id":"sel-1","savedSessionId":"saved-1"}}})
            ),
            (
                json!("session/new"),
                json!({"cwd":"/srv/harbor","mcpServers":[],
                    "_meta":{"nori/preparedAgent":{"id":"sel-2"}}})
            ),
            (json!("_nori/host/discard_agent"), json!({"id":"sel-3"})),
        ]
    );
}

#[test]
fn a_late_reply_from_before_a_reconnect_never_settles_a_new_call() {
    let (mut client, mut rx) = client();
    client.request(HostCall::List, "before").unwrap();
    let old_id = sent(&mut rx)[0]["id"].clone();
    client.incoming(ConnectionEvent::Disconnected("dropped".into()));
    assert!(matches!(
        client.incoming(ConnectionEvent::Connected),
        HostEvent::Connected
    ));
    client.request(HostCall::List, "after").unwrap();
    let new_id = sent(&mut rx)[0]["id"].clone();
    assert_ne!(old_id, new_id);

    let late = client.incoming(ConnectionEvent::Message(
        json!({"jsonrpc":"2.0","id":old_id,"result":{}}),
    ));
    assert!(matches!(late, HostEvent::Ignored));
    let current = client.incoming(ConnectionEvent::Message(
        json!({"jsonrpc":"2.0","id":new_id,"result":{}}),
    ));
    assert!(matches!(current, HostEvent::Reply { tag: "after", .. }));
}

#[test]
fn an_error_without_a_message_is_reported_as_such() {
    let (mut client, mut rx) = client();
    client.request(HostCall::TakeControl, "take").unwrap();
    let id = sent(&mut rx)[0]["id"].clone();
    let reply = client.incoming(ConnectionEvent::Message(
        json!({"jsonrpc":"2.0","id":id,"error":{"code":-32000}}),
    ));
    assert!(matches!(
        reply,
        HostEvent::Reply { tag: "take", result: Err(error) } if error.message.is_none()
    ));
}
