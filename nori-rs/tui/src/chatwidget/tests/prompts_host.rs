//! A session on the host lists the custom prompts kept in the host's home
//! and runs their scripts on the host, so every terminal on that host sees
//! the same prompts wherever its own home is.

use super::*;
use crate::app_event_sender::HostReply;
use crate::host_client::HostCall;
use pretty_assertions::assert_eq;
use serde_json::json;

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
    while let Ok(event) = rx.try_recv() {
        match event {
            AppEvent::HostRequest { call, reply } => requests.push((call, reply)),
            AppEvent::HarnessAction(action) => actions.push(action),
            _ => {}
        }
    }
    for action in actions {
        chat.submit_harness_action(action);
    }
    while let Ok(event) = rx.try_recv() {
        match event {
            AppEvent::HostRequest { call, reply } => requests.push((call, reply)),
            AppEvent::HarnessActionFailed(message) => panic!("{message}"),
            _ => {}
        }
    }
    requests
}

/// Wait for the first event the widget sends that `pick` accepts.
async fn next_event<T>(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<AppEvent>,
    mut pick: impl FnMut(AppEvent) -> Option<T>,
) -> T {
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            match rx.recv().await {
                Some(event) => {
                    if let Some(found) = pick(event) {
                        return found;
                    }
                }
                None => panic!("the widget stopped sending"),
            }
        }
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn the_slash_popup_offers_the_prompts_kept_on_the_host() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    host_session(&mut chat);
    while rx.try_recv().is_ok() {}

    chat.submit_harness_action(crate::app_event::HarnessAction::LoadCustomPrompts);
    let mut requests = host_requests(&mut chat, &mut rx);
    assert_eq!(requests.len(), 1);
    let (call, reply) = requests.remove(0);
    assert!(matches!(call, HostCall::ListPrompts), "{call:?}");
    reply.deliver(Ok(json!({"prompts": [{
        "name": "weather",
        "description": null,
        "argumentHint": null,
        "kind": {"type": "script", "interpreter": "bash"},
        "content": "",
    }]})));
    let prompts = next_event(&mut rx, |event| match event {
        AppEvent::CustomPromptsLoaded(prompts) => Some(prompts),
        _ => None,
    })
    .await;
    chat.on_custom_prompts_loaded(prompts);

    type_text(&mut chat, "/prompts:weather north");
    chat.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    let ran = next_event(&mut rx, |event| match event {
        AppEvent::ExecuteScript { prompt, args } => Some((prompt.name, args)),
        _ => None,
    })
    .await;
    assert_eq!(ran, ("weather".to_string(), vec!["north".to_string()]));
}

#[tokio::test]
async fn a_script_prompt_runs_on_the_host_and_its_output_comes_back() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    let session = host_session(&mut chat);
    while rx.try_recv().is_ok() {}
    let prompt = nori_harness::CustomPrompt {
        name: "weather".into(),
        path: std::path::PathBuf::new(),
        content: String::new(),
        description: None,
        argument_hint: None,
        kind: nori_harness::CustomPromptKind::Script {
            interpreter: "bash".into(),
        },
    };

    assert!(
        chat.run_script_on_host(prompt, vec!["north".into()])
            .is_none(),
        "a host session's script is the host's to run"
    );

    let (call, reply) = next_event(&mut rx, |event| match event {
        AppEvent::HostRequest { call, reply } => Some((call, reply)),
        _ => None,
    })
    .await;
    match call {
        HostCall::RunPrompt {
            session_id,
            name,
            args,
        } => assert_eq!(
            (session_id, name, args),
            (session, "weather".to_string(), vec!["north".to_string()])
        ),
        other => panic!("{other:?}"),
    }
    reply.deliver(Ok(json!({"stdout": "sunny\n"})));
    let completed = next_event(&mut rx, |event| match event {
        AppEvent::ScriptExecutionComplete { name, result } => Some((name, result)),
        _ => None,
    })
    .await;
    assert_eq!(
        completed,
        ("weather".to_string(), Ok("sunny\n".to_string()))
    );
}

#[tokio::test]
async fn a_widget_without_a_host_session_runs_its_script_itself() {
    let (chat, mut rx, _op_rx) = make_chatwidget_manual();
    while rx.try_recv().is_ok() {}
    let prompt = nori_harness::CustomPrompt {
        name: "weather".into(),
        path: std::path::PathBuf::from("/commands/weather.sh"),
        content: String::new(),
        description: None,
        argument_hint: None,
        kind: nori_harness::CustomPromptKind::Script {
            interpreter: "bash".into(),
        },
    };

    let returned = chat.run_script_on_host(prompt.clone(), vec![]);

    assert_eq!(returned, Some((prompt, vec![])));
    assert!(rx.try_recv().is_err());
}
