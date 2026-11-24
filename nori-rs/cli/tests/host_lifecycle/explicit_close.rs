//! The host's explicit close contract, observed from the adapter and its peers.
use super::acp_client::Acp;
use super::acp_v1::controller;
use super::acp_v1::host_with;
use super::acp_v1::new_session;
use super::acp_v1::text;
use super::*;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;

fn fixture_host(extra: &str) -> Host {
    let script = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/standard_acp/fixture.py");
    host_with(
        &format!(
            r#"{extra}
[[agents]]
name = "Amber"
slug = "amber"
[agents.distribution.local]
command = "python3"
args = ["{script}", "amber"]
"#
        ),
        &[],
    )
}

async fn amber(acp: &mut Acp, folder: &std::path::Path) -> Value {
    acp.call(
        "session/new",
        json!({"cwd":canonical(folder),"mcpServers":[],"_meta":{"nori/agent":"amber"}}),
    )
    .await
    .unwrap()["sessionId"]
        .clone()
}

#[tokio::test(flavor = "multi_thread")]
async fn explicit_close_preserves_a_permission_on_refusal_and_retries_once() {
    let host = fixture_host("");
    let folder = TempDir::new().unwrap();
    let mut acp = controller(&host);
    let id = amber(&mut acp, folder.path()).await;
    let (sibling, _) = new_session(&mut acp, folder.path()).await;
    for file in ["fixture-hold-close", "fixture-traffic"] {
        std::fs::write(folder.path().join(file), "").unwrap();
    }
    acp.send(json!({"jsonrpc":"2.0","id":"amber","method":"session/prompt","params":{"sessionId":id,"prompt":text("ask first")}})).await;
    let permission = acp
        .until(|frame| frame["method"] == "session/request_permission")
        .await;
    acp.send(
        json!({"jsonrpc":"2.0","id":"close","method":"session/close","params":{"sessionId":id}}),
    )
    .await;
    tokio::time::timeout(Duration::from_secs(5), async {
        while !folder.path().join("fixture-close-started").exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("adapter received explicit close");
    acp.call("session/close", json!({"sessionId":id}))
        .await
        .expect_err("duplicate in flight");
    acp.call(
        "session/prompt",
        json!({"sessionId":sibling,"prompt":text("sibling still live")}),
    )
    .await
    .unwrap();
    acp.send(json!({"jsonrpc":"2.0","id":permission["id"],"result":{"outcome":{"outcome":"selected","optionId":"allow-amber"}}})).await;
    let mut replies = Vec::new();
    for _ in 0..2 {
        replies.push(
            acp.until(|f| f["id"] == "close" || f["id"] == "amber")
                .await,
        );
    }
    let refusal = replies.iter().find(|f| f["id"] == "close").unwrap();
    assert_eq!(refusal["error"]["code"], -32004);
    assert!(
        refusal["error"]["message"]
            .as_str()
            .unwrap()
            .contains("sample close declined"),
        "{refusal}"
    );
    assert!(
        replies
            .iter()
            .any(|f| f["id"] == "amber" && f["result"]["stopReason"] == "end_turn")
    );
    acp.call(
        "session/prompt",
        json!({"sessionId":id, "prompt":text("finish")}),
    )
    .await
    .unwrap();
    for _ in 0..2 {
        acp.call("session/close", json!({"sessionId":id}))
            .await
            .unwrap();
    }
    let trace = std::fs::read_to_string(folder.path().join("fixture-traffic")).unwrap();
    assert_eq!(
        trace
            .lines()
            .filter(|line| line.starts_with("session/close "))
            .count(),
        2,
        "{trace}"
    );
    assert!(!trace.contains("session/cancel"), "{trace}");
    acp.call(
        "session/prompt",
        json!({"sessionId":sibling,"prompt":text("sibling survived")}),
    )
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn refusal_preserves_a_delegated_child_and_acknowledgement_retires_it() {
    let host = fixture_host("");
    let folder = TempDir::new().unwrap();
    let mut acp = controller(&host);
    let id = amber(&mut acp, folder.path()).await;
    acp.call(
        "_nori/session/delegate",
        json!({"session_id":id,"agent":"amber","task":"child awaits permission"}),
    )
    .await
    .unwrap();
    let list = acp.call("_nori/session/list", json!({})).await.unwrap();
    let child = list["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["parent"] == id)
        .unwrap()["session_id"]
        .clone();
    std::fs::write(folder.path().join("fixture-refuse-close"), "").unwrap();
    acp.call("session/close", json!({"sessionId":id}))
        .await
        .expect_err("adapter refused");
    let list = acp.call("_nori/session/list", json!({})).await.unwrap();
    let status = |list: &Value| {
        list["sessions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["session_id"] == child)
            .unwrap()["status"]
            .clone()
    };
    assert!(!matches!(
        status(&list).as_str(),
        Some("closed" | "failed" | "done")
    ));
    acp.call("session/close", json!({"sessionId":id}))
        .await
        .unwrap();
    let list = acp.call("_nori/session/list", json!({})).await.unwrap();
    assert_eq!(status(&list), json!("closed"));
}

#[tokio::test(flavor = "multi_thread")]
async fn reloading_after_acknowledgement_spares_the_new_process() {
    let folder = TempDir::new().unwrap();
    let gate = folder.path().join("end.sh");
    std::fs::write(
        &gate,
        r#"cd "$(dirname "$0")"
touch ending
while [ ! -f release ]; do sleep 0.02; done
"#,
    )
    .unwrap();
    let host = fixture_host(&format!("[hooks]\nsession_end = [{gate:?}]"));
    let mut acp = controller(&host);
    let id = amber(&mut acp, folder.path()).await;
    acp.call(
        "session/prompt",
        json!({"sessionId":id,"prompt":text("finish")}),
    )
    .await
    .unwrap();
    acp.send(json!({"jsonrpc":"2.0","id":"old-close","method":"session/close","params":{"sessionId":id}})).await;
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let list = acp.call("_nori/session/list", json!({})).await.unwrap();
            if folder.path().join("ending").exists()
                && list["sessions"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|r| r["session_id"] == id && r["status"] == "closed")
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("acknowledged session visible before hooks finish");
    acp.call(
        "session/load",
        json!({"sessionId":id,"cwd":canonical(folder.path()),"mcpServers":[]}),
    )
    .await
    .unwrap();
    std::fs::write(folder.path().join("release"), "").unwrap();
    let old = acp.until(|f| f["id"] == "old-close").await;
    assert_eq!(old["result"], json!({}));
    let prompt = acp
        .call(
            "session/prompt",
            json!({"sessionId":id,"prompt":text("finish")}),
        )
        .await
        .unwrap();
    assert_eq!(prompt["stopReason"], "end_turn");
}

#[tokio::test(flavor = "multi_thread")]
async fn explicit_close_honors_capability_and_an_already_released_session() {
    let host = fixture_host("");
    let mut acp = controller(&host);
    for (flag, expected_requests) in [("fixture-no-close", 0), ("fixture-session-missing", 1)] {
        let folder = TempDir::new().unwrap();
        std::fs::write(folder.path().join(flag), "").unwrap();
        std::fs::write(folder.path().join("fixture-traffic"), "").unwrap();
        let id = amber(&mut acp, folder.path()).await;
        for _ in 0..2 {
            acp.call("session/close", json!({"sessionId":id}))
                .await
                .unwrap();
        }
        let trace = std::fs::read_to_string(folder.path().join("fixture-traffic")).unwrap();
        assert_eq!(
            trace.matches("session/close ").count(),
            expected_requests,
            "{trace}"
        );
    }
}
