//! `!` commands on the host: the controller asks the host to run one in a
//! session, it runs on the host in that session's folder, and every terminal
//! following that session sees the command, its output and its exit code on
//! that session's own stream.

use super::acp_client::Acp;
use super::acp_v1::controller;
use super::acp_v1::host_with;
use super::acp_v1::new_session;
use super::acp_v1::watcher;
use super::*;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;

async fn shell(
    acp: &mut Acp,
    id: &str,
    command: &str,
) -> Result<Value, super::acp_client::RpcError> {
    acp.call(
        "_nori/session/shell",
        json!({"sessionId": id, "command": command}),
    )
    .await
}

async fn follow(acp: &mut Acp, id: &str) {
    acp.call("_nori/session/subscribe", json!({"session_id": id}))
        .await
        .unwrap();
}

/// One `!` command as a follower saw it.
#[derive(Debug, PartialEq)]
struct Run {
    session: String,
    command: String,
    cwd: String,
    stdout: String,
    exit_code: i64,
}

/// Wait for the next `!` command to finish, and return it with the session
/// each of its events belonged to.
async fn next_run(acp: &mut Acp) -> Run {
    let mut run = Run {
        session: String::new(),
        command: String::new(),
        cwd: String::new(),
        stdout: String::new(),
        exit_code: -1,
    };
    loop {
        let frame = acp
            .until(|frame| {
                frame["method"] == "_nori/session/event"
                    && frame["params"]["event"]["event"]["event_type"] == "user_shell"
            })
            .await;
        let session = frame["params"]["conversation_id"]
            .as_str()
            .unwrap()
            .to_owned();
        let event = &frame["params"]["event"]["event"]["event"];
        match event["state"].as_str().unwrap() {
            "started" => {
                run.session = session;
                run.command = event["command"].as_str().unwrap().to_owned();
                run.cwd = event["cwd"].as_str().unwrap().to_owned();
            }
            "output" => {
                assert_eq!(session, run.session);
                if event["stream"] == "stdout" {
                    let bytes: Vec<u8> = serde_json::from_value(event["chunk"].clone()).unwrap();
                    run.stdout.push_str(&String::from_utf8(bytes).unwrap());
                }
            }
            "finished" => {
                assert_eq!(session, run.session);
                run.exit_code = event["exit_code"].as_i64().unwrap();
                return run;
            }
            other => panic!("unexpected state {other}"),
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn initialize_advertises_shell_commands() {
    let host = host_with("", &[]);
    let mut acp = controller(&host);
    let result = acp
        .call("initialize", json!({"protocolVersion": 1}))
        .await
        .unwrap();
    assert_eq!(
        result["capabilities"]["nori_sessions"]["shell"],
        json!(true),
        "{result}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_command_runs_in_the_session_folder_and_every_follower_sees_it() {
    let host = host_with("", &[]);
    let first_folder = TempDir::new().unwrap();
    let second_folder = TempDir::new().unwrap();
    let mut acp = controller(&host);
    let (first, _) = new_session(&mut acp, first_folder.path()).await;
    let (second, _) = new_session(&mut acp, second_folder.path()).await;
    let mut watching = watcher(&host);
    follow(&mut watching, "*").await;
    let folder = canonical(first_folder.path());
    assert_ne!(
        Some(folder.as_str()),
        std::env::current_dir()
            .ok()
            .as_deref()
            .and_then(|cwd| cwd.to_str()),
    );

    shell(&mut acp, &first, "pwd").await.unwrap();

    let run = next_run(&mut watching).await;
    assert_eq!(
        run,
        Run {
            session: first.clone(),
            command: "pwd".into(),
            cwd: folder.clone(),
            stdout: format!("{folder}\n"),
            exit_code: 0,
        }
    );
    assert_ne!(run.session, second);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failing_command_reports_its_exit_code() {
    let host = host_with("", &[]);
    let folder = TempDir::new().unwrap();
    let mut acp = controller(&host);
    let (id, _) = new_session(&mut acp, folder.path()).await;
    let mut watching = watcher(&host);
    follow(&mut watching, &id).await;

    shell(&mut acp, &id, "echo partial; exit 3").await.unwrap();

    let run = next_run(&mut watching).await;
    assert_eq!(
        (run.stdout.as_str(), run.exit_code),
        ("partial\n", 3),
        "{run:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_watcher_cannot_run_a_command() {
    let host = host_with("", &[]);
    let folder = TempDir::new().unwrap();
    let mut acp = controller(&host);
    let (id, _) = new_session(&mut acp, folder.path()).await;
    let mut watching = watcher(&host);

    let refused = shell(&mut watching, &id, "pwd").await.unwrap_err();

    assert_eq!(refused.code, -32016, "{refused:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn an_empty_command_is_refused() {
    let host = host_with("", &[]);
    let folder = TempDir::new().unwrap();
    let mut acp = controller(&host);
    let (id, _) = new_session(&mut acp, folder.path()).await;

    let refused = shell(&mut acp, &id, "   ").await.unwrap_err();

    assert_eq!(refused.code, -32602, "{refused:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn an_unknown_session_is_refused() {
    let host = host_with("", &[]);
    let mut acp = controller(&host);
    let unknown = nori_harness::ConversationId::new().to_string();

    let refused = shell(&mut acp, &unknown, "pwd").await.unwrap_err();

    assert_eq!(refused.code, -32002, "{refused:?}");
}
