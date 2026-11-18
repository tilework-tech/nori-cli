use std::time::Duration;

use anyhow::Context;
use anyhow::Result;
use futures::SinkExt;
use futures::StreamExt;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use tokio::io::DuplexStream;
use tokio_util::codec::Framed;
use tokio_util::codec::LinesCodec;

use super::connect_with_forks;
use crate::backend::managed_additions::ManagedSessionAdditions;

struct Adapter(Framed<DuplexStream, LinesCodec>);

impl Adapter {
    async fn read(&mut self) -> Result<Value> {
        let line = self.0.next().await.context("adapter input closed")??;
        Ok(serde_json::from_str(&line)?)
    }

    async fn send(&mut self, frame: &Value) -> Result<()> {
        self.0.send(frame.to_string()).await?;
        Ok(())
    }
}

#[tokio::test]
async fn fork_setup_keeps_early_adapter_callbacks_off_the_parent_connection() -> Result<()> {
    tokio::time::timeout(Duration::from_secs(3), open_with_early_permission()).await?
}

async fn open_with_early_permission() -> Result<()> {
    let root = tempfile::tempdir()?;
    let cwd = root.path();
    let (agent, host) = tokio::io::duplex(4096);
    let mut agent = Adapter(Framed::new(agent, LinesCodec::new()));
    let (reader, writer) = tokio::io::split(host);
    let additions =
        ManagedSessionAdditions::new(cwd.into(), cwd.into(), "fixture".into(), false, false);
    let (mut parent, forks) = connect_with_forks(
        reader,
        writer,
        cwd.into(),
        additions,
        false,
        false,
        "fixture".into(),
    );

    let capabilities = json!({
        "agentCapabilities": {
            "sessionCapabilities": {"fork": {}, "close": {}},
            "_meta": {"nori.dev/readOnlyFork": {"version": 1, "modeId": "observe"}}
        }
    });
    for (method, params, result) in [
        ("initialize", json!({"protocolVersion": 1}), capabilities),
        (
            "session/new",
            json!({"cwd": cwd, "mcpServers": []}),
            json!({"sessionId": "parent"}),
        ),
    ] {
        let request = json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params});
        parent.input.send(request.clone()).await?;
        assert_eq!(agent.read().await?, request);
        let response = json!({"jsonrpc": "2.0", "id": 1, "result": result});
        agent.send(&response).await?;
        assert_eq!(parent.output.recv().await, Some(response));
    }

    let marker = json!({"bindingId": "fixture", "generation": 1, "sessionId": "parent"});
    let params = json!({
        "cwd": cwd, "sessionId": "parent", "mcpServers": [],
        "_meta": {"nori.dev/sideSession": marker, "fixture.extra": true}
    });
    let opening = tokio::spawn(async move { forks.open("parent", params).await });
    let fork = agent.read().await?;
    assert_eq!(fork["method"], "session/fork");
    assert_eq!(
        fork["params"]["_meta"],
        json!({"nori.dev/sideSession": marker})
    );

    agent
        .send(&json!({
            "jsonrpc": "2.0", "id": 8, "method": "session/request_permission",
            "params": {"sessionId": "child"}
        }))
        .await?;
    let denied = agent.read().await?;
    assert_eq!(denied["id"], 8);
    assert!(denied["error"].is_object());

    let mut duplicate = json!({"jsonrpc": "2.0", "id": 18,
        "method": "session/request_permission", "params": {"sessionId": "parent"}});
    agent.send(&duplicate).await?;
    assert_eq!(parent.output.recv().await.as_ref(), Some(&duplicate));
    duplicate["params"]["sessionId"] = json!("child");
    agent.send(&duplicate).await?;

    agent
        .send(&json!({
            "jsonrpc": "2.0", "id": fork["id"],
            "result": {"sessionId": "child", "modes": {
                "availableModes": [{"id": "observe", "name": "Observe"}], "currentModeId": "edit"
            }}
        }))
        .await?;
    let set = agent.read().await?;
    assert_eq!(set["method"], "session/set_mode");
    assert_eq!(
        set["params"],
        json!({"sessionId": "child", "modeId": "observe"})
    );
    assert!(!opening.is_finished());
    agent
        .send(&json!({"jsonrpc": "2.0", "id": set["id"], "result": {}}))
        .await?;
    let (side, result) = opening.await??;
    assert_eq!(result["modes"]["currentModeId"], "observe");

    parent
        .input
        .send(json!({"jsonrpc": "2.0", "id": 18,
        "result": {"outcome": {"outcome": "selected", "optionId": "parent-only"}}}))
        .await?;
    let prompt = json!({
        "jsonrpc": "2.0", "id": 2, "method": "session/prompt",
        "params": {"sessionId": "parent", "prompt": []}
    });
    parent.input.send(prompt.clone()).await?;
    assert_eq!(agent.read().await?, prompt);
    let done = json!({"jsonrpc": "2.0", "id": 2, "result": {"stopReason": "end_turn"}});
    agent.send(&done).await?;
    assert_eq!(parent.output.recv().await, Some(done));

    drop(side);
    let close = agent.read().await?;
    assert_eq!(close["method"], "session/close");
    assert_eq!(close["params"]["sessionId"], "child");
    Ok(())
}
