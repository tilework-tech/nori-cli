//! Cancellation follows the active conversation across controller handoffs.

use super::Host;
use super::acp_client::Acp;
use super::acp_client::is_update_mentioning;
use super::acp_v1::controller;
use super::acp_v1::new_session;
use super::acp_v1::text;
use super::acp_v1::watcher;
use super::canonical;
use super::success;
use pretty_assertions::assert_eq;
use serde_json::json;
use tempfile::TempDir;

async fn set_activity(client: &mut Acp, id: &str, working: bool, cancels: i32) {
    let (speed, status) = if working {
        ("fast", "working")
    } else {
        ("slow", "idle")
    };
    client
        .call(
            "session/set_config_option",
            json!({"sessionId": id, "configId": "speed", "value": speed}),
        )
        .await
        .unwrap();
    let observed = format!("{status} with {cancels} cancels");
    client.until(is_update_mentioning(id, &observed)).await;
}

fn external_turn_host() -> Host {
    let host = Host::new();
    let script = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/standard_acp/fixture.py");
    std::fs::write(
        host.0.path().join("config.toml"),
        format!(
            r#"agent = "external-turn"
[[agents]]
name = "External Turn"
slug = "external-turn"

[agents.distribution.local]
command = "python3"
args = ["{script}", "external-turn"]
"#
        ),
    )
    .unwrap();
    success(host.run(&["host", "--port", "0"]));
    host
}

#[tokio::test(flavor = "multi_thread")]
async fn controller_handoff_cancels_external_and_owned_turns_once() {
    let host = external_turn_host();
    let folder = TempDir::new().unwrap();
    let mut first = controller(&host);
    let mut second = controller(&host);
    let mut observer = watcher(&host);
    let (id, _) = new_session(&mut first, folder.path()).await;
    for client in [&mut second, &mut observer] {
        client
            .call(
                "session/resume",
                json!({"sessionId": id, "cwd": canonical(folder.path())}),
            )
            .await
            .unwrap();
    }
    let (other, _) = new_session(&mut first, folder.path()).await;
    let cancel = json!({"jsonrpc":"2.0","method":"session/cancel","params":{"sessionId":id}});

    // The adapter starts work without a prompt. Viewing its output grants no write access.
    set_activity(&mut first, &id, true, 0).await;
    first
        .send(json!({"jsonrpc":"2.0","method":"session/cancel","params":{"sessionId":other}}))
        .await;
    observer.send(cancel.clone()).await;
    assert_eq!(
        observer
            .call("_nori/control/status", json!({}))
            .await
            .unwrap()["role"],
        "watcher"
    );
    set_activity(&mut first, &id, true, 0).await;
    first.send(cancel.clone()).await;
    first
        .until(is_update_mentioning(&id, "Received cancel 1"))
        .await;

    // Neither another notification nor a controller change is a new turn.
    first.send(cancel.clone()).await;
    first.call("_nori/control/status", json!({})).await.unwrap();
    second.call("_nori/control/take", json!({})).await.unwrap();
    second.send(cancel.clone()).await;
    set_activity(&mut second, &id, false, 1).await;
    second.send(cancel.clone()).await;
    let idle = second
        .call("_nori/session/cancel", json!({"session_id": id}))
        .await
        .expect_err("a late cancel reports idle");

    assert!(idle.message.contains("active turn"), "{idle:?}");
    set_activity(&mut second, &id, true, 1).await;
    first.send(cancel.clone()).await;
    first.call("_nori/control/status", json!({})).await.unwrap();
    set_activity(&mut second, &id, true, 1).await;
    second
        .call("_nori/session/cancel", json!({"session_id": id}))
        .await
        .unwrap();
    second
        .until(is_update_mentioning(&id, "Received cancel 2"))
        .await;
    set_activity(&mut second, &id, false, 2).await;

    // The current controller can interrupt a prompt. Its response still belongs to the submitter.
    second
        .send(
            json!({"jsonrpc":"2.0","id":"owned","method":"session/prompt","params":{
                "sessionId":id,"prompt":text("run a local task")
            }}),
        )
        .await;
    second
        .until(is_update_mentioning(&id, "Awaiting interrupt"))
        .await;
    first.call("_nori/control/take", json!({})).await.unwrap();
    first.send(cancel).await;
    let response = second.until(|frame| frame["id"] == "owned").await;
    assert_eq!(response["result"]["stopReason"], "cancelled");
    set_activity(&mut first, &id, false, 3).await;
    set_activity(&mut first, &other, false, 0).await;
}

async fn working_observation(observer: &mut Acp, session_id: &str) -> String {
    let update = observer
        .until(|frame| {
            let params = &frame["params"];
            let event = &params["event"]["event"];
            frame["method"] == "_nori/session/event"
                && params["conversation_id"] == session_id
                && event["event_type"] == "external_turn_changed"
                && event["event"]["state"] == "working"
        })
        .await;
    update["params"]["event"]["event"]["event"]["id"]
        .as_str()
        .unwrap()
        .to_owned()
}

#[tokio::test(flavor = "multi_thread")]
async fn observed_cancel_rejects_a_late_or_foreign_turn_and_only_the_controller_can_send_it() {
    let host = external_turn_host();
    let folder = TempDir::new().unwrap();
    let mut control = controller(&host);
    let mut observer = watcher(&host);
    observer
        .call("_nori/session/subscribe", json!({"session_id":"*"}))
        .await
        .unwrap();

    let (id, _) = new_session(&mut control, folder.path()).await;
    let (sibling, _) = new_session(&mut control, folder.path()).await;
    set_activity(&mut control, &id, true, 0).await;
    let amber = working_observation(&mut observer, &id).await;
    set_activity(&mut control, &id, false, 0).await;
    set_activity(&mut control, &id, true, 0).await;
    let blue = working_observation(&mut observer, &id).await;
    set_activity(&mut control, &sibling, true, 0).await;
    let sibling_turn = working_observation(&mut observer, &sibling).await;
    let method = "_nori/session/cancel_observed";

    for guard in [json!(amber), json!(sibling_turn)] {
        let error = control
            .call(method, json!({"sessionId": id, "observedTurnId": guard}))
            .await
            .unwrap_err();
        assert!(error.message.contains("no longer active"), "{error:?}");
    }
    for malformed in [
        json!({"sessionId":id}),
        json!({"observedTurnId":blue}),
        json!({"sessionId":id, "observedTurnId": ""}),
        json!({"sessionId":id, "observedTurnId": 7}),
    ] {
        let error = control.call(method, malformed).await.unwrap_err();
        assert_eq!(error.code, -32602, "{error:?}");
    }
    let current = json!({"sessionId": id, "observedTurnId": blue});
    let forbidden = observer.call(method, current.clone()).await.unwrap_err();
    assert_eq!(forbidden.code, -32016);
    set_activity(&mut control, &id, true, 0).await;
    set_activity(&mut control, &sibling, true, 0).await;

    for _ in 0..2 {
        let result = control.call(method, current.clone()).await.unwrap();
        assert_eq!(result, json!({"sessionId":id}));
    }
    control
        .until(is_update_mentioning(&id, "Received cancel 1"))
        .await;
    set_activity(&mut control, &id, false, 1).await;
    set_activity(&mut control, &sibling, false, 0).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn prompt_receipt_fences_successor_and_sibling_cancellation() {
    let host = external_turn_host();
    let folder = TempDir::new().unwrap();
    let mut control = controller(&host);
    let mut observations = watcher(&host);
    observations
        .call("_nori/session/subscribe", json!({"session_id":"*"}))
        .await
        .unwrap();
    let (id, _) = new_session(&mut control, folder.path()).await;
    let (sibling, _) = new_session(&mut control, folder.path()).await;
    let method = "_nori/session/cancel_observed";
    let submit = |session: &str| json!({"session_id": session, "prompt":text("check sample")});
    let first = control
        .call("_nori/session/prompt", submit(&id))
        .await
        .unwrap();
    let other = control
        .call("_nori/session/prompt", submit(&sibling))
        .await
        .unwrap();
    let guard = |session: &str, receipt: &serde_json::Value| json!({"sessionId":session, "observedTurnId": receipt["request_id"].as_str().expect("host observation")});
    control.call(method, guard(&id, &other)).await.unwrap_err();
    control.call(method, guard(&id, &first)).await.unwrap();
    control
        .until(is_update_mentioning(&id, "Received cancel 1"))
        .await;
    for phase in ["cancelling", "idle"] {
        observations
            .until(|frame| {
                frame["params"]["conversation_id"] == id
                    && frame["params"]["event"]["event"]["event_type"] == "session_phase_changed"
                    && frame["params"]["event"]["event"]["event"]["phase"] == phase
            })
            .await;
    }
    let second = control
        .call("_nori/session/prompt", submit(&id))
        .await
        .unwrap();
    control.call(method, guard(&id, &first)).await.unwrap_err();
    control.call(method, guard(&id, &second)).await.unwrap();
    control
        .until(is_update_mentioning(&id, "Received cancel 2"))
        .await;
    control.call(method, guard(&sibling, &other)).await.unwrap();
    control
        .until(is_update_mentioning(&sibling, "Received cancel 1"))
        .await;
}
