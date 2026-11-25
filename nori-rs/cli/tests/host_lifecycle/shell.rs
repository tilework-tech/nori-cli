//! `!` commands on the host: a local controller asks the host to run one in
//! a session, it runs on the host in that session's folder, and every
//! terminal following that session sees the command, its output and its exit
//! code on that session's own stream. A controller that cannot prove it runs
//! on the host's machine is remote, and the host runs nothing for it.

use super::acp_client::Acp;
use super::acp_v1::controller as remote_controller;
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

/// A controller holding the host's private local token, as a `nori attach`
/// on the host's own machine does.
fn local_controller(host: &Host) -> Acp {
    Acp::new(tokio::task::block_in_place(|| host.local_client()))
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
async fn initialize_offers_shell_commands_to_local_controllers_only() {
    let host = host_with("", &[]);
    let mut local = local_controller(&host);
    let mut remote = remote_controller(&host);

    let mut offered = Vec::new();
    for acp in [&mut local, &mut remote] {
        let result = acp
            .call("initialize", json!({"protocolVersion": 1}))
            .await
            .unwrap();
        offered.push(result["capabilities"]["nori_sessions"]["shell"].clone());
    }

    assert_eq!(offered, vec![json!(true), json!(false)]);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_command_runs_in_the_session_folder_and_every_follower_sees_it() {
    let host = host_with("", &[]);
    let first_folder = TempDir::new().unwrap();
    let second_folder = TempDir::new().unwrap();
    let mut acp = local_controller(&host);
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
    let mut acp = local_controller(&host);
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

/// The watcher rule comes first: a remote watcher is told it is a watcher.
#[tokio::test(flavor = "multi_thread")]
async fn a_watcher_cannot_run_a_command() {
    let host = host_with("", &[]);
    let folder = TempDir::new().unwrap();
    let mut acp = local_controller(&host);
    let (id, _) = new_session(&mut acp, folder.path()).await;
    let mut watching = watcher(&host);

    let refused = shell(&mut watching, &id, "pwd").await.unwrap_err();

    assert_eq!(refused.code, -32016, "{refused:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn an_empty_command_is_refused() {
    let host = host_with("", &[]);
    let folder = TempDir::new().unwrap();
    let mut acp = local_controller(&host);
    let (id, _) = new_session(&mut acp, folder.path()).await;

    let refused = shell(&mut acp, &id, "   ").await.unwrap_err();

    assert_eq!(refused.code, -32602, "{refused:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn an_unknown_session_is_refused() {
    let host = host_with("", &[]);
    let mut acp = local_controller(&host);
    let unknown = nori_harness::ConversationId::new().to_string();

    let refused = shell(&mut acp, &unknown, "pwd").await.unwrap_err();

    assert_eq!(refused.code, -32002, "{refused:?}");
}

/// Every command the follower saw start, in order, until `last` finishes.
async fn started_until(acp: &mut Acp, last: &str) -> Vec<String> {
    let mut started = Vec::new();
    loop {
        let frame = acp
            .until(|frame| {
                frame["method"] == "_nori/session/event"
                    && frame["params"]["event"]["event"]["event_type"] == "user_shell"
            })
            .await;
        let event = &frame["params"]["event"]["event"]["event"];
        if event["state"] == "started" {
            started.push(event["command"].as_str().unwrap().to_owned());
        }
        if event["state"] == "finished" && started.last().map(String::as_str) == Some(last) {
            return started;
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_remote_controller_is_refused_and_nothing_runs() {
    let host = host_with("", &[]);
    let folder = TempDir::new().unwrap();
    let mut acp = remote_controller(&host);
    let (id, _) = new_session(&mut acp, folder.path()).await;
    let mut watching = watcher(&host);
    follow(&mut watching, &id).await;

    let refused = shell(&mut acp, &id, "touch marker").await.unwrap_err();

    assert_eq!(
        (refused.code, refused.message.as_str()),
        (-32018, "Remote shell commands are not yet supported."),
    );
    // Events on one session arrive in order, so a refused command that ran
    // anyway would start before this local one.
    let mut local = local_controller(&host);
    local.call("_nori/control/take", json!({})).await.unwrap();
    shell(&mut local, &id, "true").await.unwrap();
    assert_eq!(
        (
            started_until(&mut watching, "true").await,
            folder.path().join("marker").exists()
        ),
        (vec!["true".to_string()], false)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_remote_controller_learns_nothing_about_sessions() {
    let host = host_with("", &[]);
    let mut acp = remote_controller(&host);
    let unknown = nori_harness::ConversationId::new().to_string();

    let refused = shell(&mut acp, &unknown, "pwd").await.unwrap_err();

    assert_eq!(refused.code, -32018, "{refused:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_wrong_local_token_connects_as_remote() {
    let host = host_with("", &[]);
    let folder = TempDir::new().unwrap();
    // The connection itself succeeds: the bearer token admits it, and only
    // the local token is wrong.
    let mut acp = Acp::new(tokio::task::block_in_place(|| {
        host.connect_as(json!({}), Some("not-the-local-token"))
    }));
    let (id, _) = new_session(&mut acp, folder.path()).await;
    let offered = acp
        .call("initialize", json!({"protocolVersion": 1}))
        .await
        .unwrap()["capabilities"]["nori_sessions"]["shell"]
        .clone();

    let refused = shell(&mut acp, &id, "pwd").await.unwrap_err();

    assert_eq!((offered, refused.code), (json!(false), -32018));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_local_watcher_still_cannot_run_a_command() {
    let host = host_with("", &[]);
    let folder = TempDir::new().unwrap();
    let mut acp = local_controller(&host);
    let (id, _) = new_session(&mut acp, folder.path()).await;
    let mut watching = Acp::new(tokio::task::block_in_place(|| {
        host.connect_as(json!({"observe": true}), Some(&host.local_token()))
    }));

    let refused = shell(&mut watching, &id, "pwd").await.unwrap_err();

    assert_eq!(refused.code, -32016, "{refused:?}");
}

#[test]
fn no_other_host_file_holds_the_private_local_token() {
    use std::os::unix::fs::PermissionsExt;
    let host = host_with("", &[]);
    let folder = host.0.path().join("host");
    let path = folder.join("local-token");
    let local = std::fs::read_to_string(&path).unwrap();
    let local = local.trim();
    let published: Vec<String> = std::fs::read_dir(&folder)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|other| *other != path && other.is_file())
        .filter(|other| String::from_utf8_lossy(&std::fs::read(other).unwrap()).contains(local))
        .map(|other| other.display().to_string())
        .collect();

    assert_eq!(
        (
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            local.is_empty(),
            published,
        ),
        (0o600, false, Vec::<String>::new()),
    );
}
