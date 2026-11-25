//! `!` commands: a session on the host runs them on that host, and every
//! widget shows the command, its output and how it ended from the session's
//! own `user_shell` events, live or replayed.

use super::*;
use crate::app_event_sender::HostReply;
use crate::host_client::HostCall;
use insta::assert_snapshot;
use nori_protocol::UserShellEvent;
use nori_protocol::UserShellStream;
use pretty_assertions::assert_eq;

fn host_session(chat: &mut ChatWidget) -> String {
    let id = nori_harness::ConversationId::new();
    chat.managed_conversation_id = Some(id);
    id.to_string()
}

/// Act as the App: hand the widget's own harness actions back to it, and
/// return the host requests that result. Nothing may reach an in-process
/// harness.
fn host_requests(
    chat: &mut ChatWidget,
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<AppEvent>,
) -> Vec<(HostCall, HostReply)> {
    let mut requests = Vec::new();
    let mut actions = Vec::new();
    let take = |event, actions: &mut Vec<_>, requests: &mut Vec<_>| match event {
        AppEvent::HostRequest { call, reply } => requests.push((call, reply)),
        AppEvent::HarnessAction(action) => actions.push(action),
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

fn shell_event(chat: &mut ChatWidget, event: UserShellEvent) {
    let generation = chat.session_generation;
    chat.handle_session_event(
        generation,
        nori_protocol::SessionEvent::Nori(nori_protocol::NoriEvent::UserShell(event)),
    );
}

fn run(chat: &mut ChatWidget, id: &str, command: &str, stdout: &str, stderr: &str, exit: i32) {
    shell_event(
        chat,
        UserShellEvent::Started {
            operation_id: id.into(),
            command: command.into(),
            cwd: PathBuf::from("/srv/lantern"),
        },
    );
    for (stream, text) in [
        (UserShellStream::Stdout, stdout),
        (UserShellStream::Stderr, stderr),
    ] {
        if !text.is_empty() {
            shell_event(
                chat,
                UserShellEvent::Output {
                    operation_id: id.into(),
                    stream,
                    chunk: text.as_bytes().to_vec(),
                },
            );
        }
    }
    shell_event(
        chat,
        UserShellEvent::Finished {
            operation_id: id.into(),
            exit_code: exit,
        },
    );
}

fn transcript(rx: &mut tokio::sync::mpsc::UnboundedReceiver<AppEvent>) -> String {
    drain_insert_history(rx)
        .iter()
        .map(|lines| lines_to_single_string(lines))
        .collect()
}

#[tokio::test]
async fn a_bang_command_runs_on_the_host_in_the_session_on_screen() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    let session = host_session(&mut chat);

    chat.submit_user_message("!ls -la".to_string().into());

    let calls: Vec<(String, String)> = host_requests(&mut chat, &mut rx)
        .into_iter()
        .map(|(call, _)| match call {
            HostCall::Shell {
                session_id,
                command,
            } => (session_id, command),
            other => panic!("unexpected {other:?}"),
        })
        .collect();
    assert_eq!(calls, vec![(session, "ls -la".to_string())]);
}

#[tokio::test]
async fn a_bang_command_without_a_host_session_stays_in_process() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();

    chat.submit_user_message("!ls".to_string().into());

    let mut in_process = Vec::new();
    while let Ok(event) = rx.try_recv() {
        match event {
            AppEvent::HarnessActionFailed(message) => in_process.push(message),
            AppEvent::HostRequest { call, .. } => panic!("sent {call:?} to a host"),
            _ => {}
        }
    }
    assert_eq!(in_process, vec!["No active harness session".to_string()]);
}

#[tokio::test]
async fn a_refused_bang_command_goes_back_to_the_prompt() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    host_session(&mut chat);
    chat.submit_user_message("!make lint".to_string().into());
    let (_, reply) = host_requests(&mut chat, &mut rx).pop().unwrap();

    reply.deliver(Err(anyhow::anyhow!(
        "watcher is read-only; take control before changing a session"
    )));
    let mut refusals = Vec::new();
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            match rx.recv().await {
                Some(AppEvent::UserShellRefused { command, .. }) => {
                    chat.on_user_shell_refused(command);
                    return;
                }
                Some(AppEvent::HarnessActionFailed(message)) => refusals.push(message),
                Some(_) => {}
                None => panic!("the widget stopped sending"),
            }
        }
    })
    .await
    .unwrap();

    assert_eq!(
        (chat.composer_text(), refusals.len()),
        ("!make lint".to_string(), 1),
        "{refusals:?}"
    );
}

#[tokio::test]
async fn a_finished_command_shows_its_output() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();

    run(
        &mut chat,
        "op-1",
        "ls",
        "Cargo.toml\nREADME.md\nsrc\n",
        "",
        0,
    );

    assert_snapshot!(transcript(&mut rx));
}

#[tokio::test]
async fn a_failed_command_shows_its_errors_and_exit_code() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();

    run(
        &mut chat,
        "op-2",
        "cargo check",
        "",
        "error: could not find `Cargo.toml` in `/srv/lantern`\n",
        101,
    );

    let cells = drain_insert_history(&mut rx);
    assert_eq!(
        cells[0][0].spans[0].style.fg,
        Some(ratatui::style::Color::Red),
        "a failed command's bullet is red"
    );
    assert_snapshot!(
        cells
            .iter()
            .map(|lines| lines_to_single_string(lines))
            .collect::<String>()
    );
}

#[tokio::test]
async fn a_command_from_the_replay_shows_like_a_live_one() {
    let (mut live, mut live_rx, _op_rx) = make_chatwidget_manual();
    let (mut replayed, mut replay_rx, _op_rx2) = make_chatwidget_manual();
    run(&mut live, "op-3", "echo hi", "hi\n", "", 0);
    let generation = replayed.session_generation;
    replayed.handle_session_event(
        generation,
        nori_protocol::SessionEvent::Nori(nori_protocol::NoriEvent::ReplayStarted(
            nori_protocol::ReplayStarted {
                source: nori_protocol::ReplaySource::Transcript,
            },
        )),
    );
    run(&mut replayed, "op-3", "echo hi", "hi\n", "", 0);
    replayed.handle_session_event(
        generation,
        nori_protocol::SessionEvent::Nori(nori_protocol::NoriEvent::ReplayFinished),
    );

    assert_eq!(transcript(&mut replay_rx), transcript(&mut live_rx));
}

#[tokio::test]
async fn a_bang_command_on_an_older_joined_host_is_refused_and_kept() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    host_session(&mut chat);
    chat.set_host_connection(crate::host_client::HostConnection {
        role: nori_protocol::ControlRole::Controller,
        connected: true,
        stopped: None,
        features: crate::host_client::HostFeatures::default(),
    });
    chat.submit_user_message("!make lint".to_string().into());

    let mut refusals = Vec::new();
    while let Ok(event) = rx.try_recv() {
        match event {
            AppEvent::HarnessAction(action) => chat.submit_harness_action(action),
            AppEvent::UserShellRefused { command, .. } => chat.on_user_shell_refused(command),
            AppEvent::HarnessActionFailed(message) => refusals.push(message),
            AppEvent::HostRequest { call, .. } => panic!("sent {call:?} to the host"),
            _ => {}
        }
    }
    assert_eq!(
        (chat.composer_text(), refusals),
        (
            "!make lint".to_string(),
            vec!["!make lint: not available on this host yet".to_string()]
        )
    );
}
