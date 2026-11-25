//! The terminal that queued a prompt takes it back through
//! `_nori/session/queue/take` exactly once.

#![allow(clippy::unwrap_used)]

use std::sync::Arc;
use std::time::Duration;

use nori_config::NoriConfig;
use nori_harness::ConversationId;
use nori_harness::remote_agent::HostClientOrigin;
use nori_harness::remote_agent::ManagedRemoteHost;
use nori_harness::runtime::AgentPrepareSpec;
use nori_harness::runtime::SessionStart;
use nori_harness::session_manager::SessionAgent;
use nori_harness::session_manager::SessionManager;
use nori_harness::session_manager::SessionManagerConfig;
use nori_harness::session_manager::SessionSource;
use nori_harness::session_manager::StartSession;
use nori_protocol::QueuePause;
use nori_protocol::SessionStatus;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;

const WAIT: Duration = Duration::from_secs(15);

async fn call(host: &ManagedRemoteHost, method: &str, params: Value) -> Value {
    host.request_with_origin(
        json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}),
        HostClientOrigin::Foreground,
    )
    .await
    .response
}

fn text(text: &str) -> Value {
    json!([{"type":"text","text":text}])
}

async fn wait_for(
    manager: &SessionManager,
    id: ConversationId,
    done: impl Fn(&nori_protocol::QueueSnapshot) -> bool,
) -> nori_protocol::QueueSnapshot {
    tokio::time::timeout(WAIT, async {
        loop {
            if let Some(queue) = manager.replay_info(&id).and_then(|info| info.queue)
                && done(&queue)
            {
                return queue;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn the_composer_that_queued_a_prompt_takes_it_back_once() {
    let manager = SessionManager::new(SessionManagerConfig::default());
    let home = tempfile::tempdir().unwrap();
    let session = manager
        .start(StartSession {
            agent: SessionAgent::Prepare(AgentPrepareSpec {
                config: Arc::new(NoriConfig {
                    active_agent: "mock-model".into(),
                    cwd: home.path().into(),
                    nori_home: home.path().into(),
                    ..Default::default()
                }),
                cli_version: "queue-take-test".into(),
                session_context: None,
                initial_context: None,
            }),
            start: SessionStart::New,
            parent: None,
            source: SessionSource::User,
        })
        .unwrap();
    let id = session.conversation_id;
    tokio::time::timeout(WAIT, async {
        while manager.get(&id).unwrap().status != SessionStatus::Ready {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let host = ManagedRemoteHost::authenticated_host(manager.clone());
    let session_id = id.to_string();

    let started = call(
        &host,
        "_nori/session/prompt",
        json!({"session_id": session_id, "prompt": text("mock:stream-until-cancel")}),
    )
    .await;
    for (message_id, prompt) in [("first", "keep me"), ("second", "edit me")] {
        let receipt = call(
            &host,
            "_nori/session/follow_up",
            json!({"session_id": session_id, "expected_turn": started["result"]["request_id"],
                "message_id": message_id, "intent": "queue", "prompt": text(prompt)}),
        )
        .await;
        assert_eq!(receipt["result"]["status"], "queued", "{receipt}");
    }
    manager.handle(&id).unwrap().cancel().await.unwrap();
    let queue = wait_for(&manager, id, |queue| {
        queue.paused == Some(QueuePause::Interrupted)
    })
    .await;
    let edit = queue
        .entries
        .iter()
        .find(|entry| entry.text == "edit me")
        .unwrap()
        .id
        .clone();

    let taken = call(
        &host,
        "_nori/session/queue/take",
        json!({"sessionId": session_id, "entryId": edit}),
    )
    .await;
    let again = call(
        &host,
        "_nori/session/queue/take",
        json!({"sessionId": session_id, "entryId": edit}),
    )
    .await;
    let left = wait_for(&manager, id, |queue| queue.entries.len() == 1).await;

    assert_eq!(taken["result"], json!({"text": "edit me"}), "{taken}");
    assert_eq!(again["result"], json!({"text": null}), "{again}");
    assert_eq!(left.prompts, vec!["keep me".to_string()]);
    assert_eq!(left.paused, Some(QueuePause::Interrupted));
    manager.handle(&id).unwrap().shutdown().await.unwrap();
}
