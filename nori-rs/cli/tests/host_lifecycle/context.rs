//! A session's context report on the host: its usage at prompt boundaries,
//! the latest reading, and the instruction files its agent loads from the
//! session's folder on the host. Any terminal may read it.

use super::acp_client::Acp;
use super::acp_v1::controller;
use super::acp_v1::host_with;
use super::acp_v1::mock_agent_binary;
use super::acp_v1::new_session;
use super::acp_v1::text;
use super::acp_v1::watcher;
use super::*;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;

async fn context(acp: &mut Acp, id: &str) -> Result<Value, super::acp_client::RpcError> {
    acp.call("_nori/session/context", json!({"sessionId": id}))
        .await
}

/// Run one turn in which the agent reports `used` of `size` tokens.
async fn turn_reporting(acp: &mut Acp, id: &str, used: i64, size: i64) {
    acp.call(
        "session/prompt",
        json!({"sessionId": id, "prompt": text(&format!("mock:usage {used} {size}"))}),
    )
    .await
    .unwrap();
}

/// The usage a report carries at the end of each turn, then its latest one.
fn readings(report: &Value) -> (Vec<Value>, Value) {
    let ends = report["timeline"]["turns"]
        .as_array()
        .unwrap()
        .iter()
        .map(|turn| turn["end"]["observation"]["usage"]["used"].clone())
        .collect();
    (ends, report["usage"].clone())
}

#[tokio::test(flavor = "multi_thread")]
async fn initialize_advertises_session_context() {
    let host = host_with("", &[]);
    let mut acp = controller(&host);
    let result = acp
        .call("initialize", json!({"protocolVersion": 1}))
        .await
        .unwrap();
    assert_eq!(
        result["capabilities"]["nori_sessions"]["context"],
        json!(true),
        "{result}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_turn_reporting_usage_shows_in_the_report() {
    let host = host_with("", &[]);
    let folder = TempDir::new().unwrap();
    let mut acp = controller(&host);
    let (id, _) = new_session(&mut acp, folder.path()).await;

    turn_reporting(&mut acp, &id, 1_200, 8_000).await;
    let report = context(&mut acp, &id).await.unwrap();

    assert_eq!(
        readings(&report),
        (vec![json!(1_200)], json!({"used": 1_200, "size": 8_000})),
        "{report}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn each_session_reports_its_own_usage() {
    let host = host_with("", &[]);
    let folder = TempDir::new().unwrap();
    let mut acp = controller(&host);
    let (first, _) = new_session(&mut acp, folder.path()).await;
    let (second, _) = new_session(&mut acp, folder.path()).await;

    turn_reporting(&mut acp, &first, 500, 4_000).await;
    turn_reporting(&mut acp, &second, 2_500, 9_000).await;
    turn_reporting(&mut acp, &first, 900, 4_000).await;

    assert_eq!(
        readings(&context(&mut acp, &first).await.unwrap()),
        (
            vec![json!(500), json!(900)],
            json!({"used": 900, "size": 4_000})
        )
    );
    assert_eq!(
        readings(&context(&mut acp, &second).await.unwrap()),
        (vec![json!(2_500)], json!({"used": 2_500, "size": 9_000}))
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_watcher_reads_the_report() {
    let host = host_with("", &[]);
    let folder = TempDir::new().unwrap();
    let mut acp = controller(&host);
    let (id, _) = new_session(&mut acp, folder.path()).await;
    turn_reporting(&mut acp, &id, 3_000, 10_000).await;

    let mut watching = watcher(&host);
    let report = context(&mut watching, &id).await.unwrap();

    assert_eq!(report["usage"], json!({"used": 3_000, "size": 10_000}));
}

#[tokio::test(flavor = "multi_thread")]
async fn instruction_files_come_from_the_session_folder_on_the_host() {
    let host = host_with(
        &format!(
            "[[agents]]\nname = \"Codex Mock\"\nslug = \"codex-mock\"\n\n[agents.distribution.local]\ncommand = \"{}\"\n",
            mock_agent_binary()
        ),
        &[],
    );
    let folder = TempDir::new().unwrap();
    std::fs::write(
        folder.path().join("AGENTS.md"),
        "Keep the garden paths swept.\n",
    )
    .unwrap();
    let mut acp = controller(&host);
    let started = acp
        .call(
            "_nori/session/new",
            json!({"cwd": canonical(folder.path()), "agent": "codex-mock"}),
        )
        .await
        .unwrap();
    let id = started["session_id"].as_str().unwrap();

    let report = context(&mut acp, id).await.unwrap();
    let agents_md = format!("{}/AGENTS.md", canonical(folder.path()));
    let listed: Vec<&Value> = report["instructions"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|file| file["path"] == agents_md.as_str())
        .collect();

    assert_eq!(listed.len(), 1, "{report}");
    assert!(listed[0]["tokens"].as_i64().unwrap() > 0, "{report}");
}

#[tokio::test(flavor = "multi_thread")]
async fn an_unknown_session_is_refused() {
    let host = host_with("", &[]);
    let mut acp = controller(&host);
    let unknown = nori_harness::ConversationId::new().to_string();

    let refused = context(&mut acp, &unknown).await.unwrap_err();

    assert_eq!(refused.code, -32002, "{refused:?}");
}
