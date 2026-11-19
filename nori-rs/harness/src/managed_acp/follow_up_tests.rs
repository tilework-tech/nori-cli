use super::connect;
use crate::backend::managed_additions::ManagedSessionAdditions;
use anyhow::Context;
use anyhow::Result;
use futures::SinkExt;
use futures::StreamExt;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use tokio_util::codec::Framed;
use tokio_util::codec::LinesCodec;

#[tokio::test]
async fn permission_and_terminal_reply_do_not_let_a_late_steer_target_the_next_prompt() -> Result<()>
{
    tokio::time::timeout(std::time::Duration::from_secs(5), exercise(false)).await?
}

#[tokio::test]
async fn missing_steer_reply_preserves_the_fence_and_never_replays_timed_out_input() -> Result<()> {
    tokio::time::timeout(std::time::Duration::from_secs(9), exercise(true)).await?
}

async fn exercise(late: bool) -> Result<()> {
    let root = tempfile::tempdir()?;
    let cwd = root.path();
    let (agent, host) = tokio::io::duplex(16384);
    let mut adapter = Framed::new(agent, LinesCodec::new());
    let (reader, writer) = tokio::io::split(host);
    let additions =
        ManagedSessionAdditions::new(cwd.into(), cwd.into(), "fixture".into(), false, false);
    let mut connection = connect(reader, writer, cwd.into(), additions, false, false);
    let rpc = |id, method, params| json!({"jsonrpc":"2.0","id":id,"method":method,"params":params});
    let response = |id: Value, result: Value| json!({"jsonrpc":"2.0","id":id,"result":result});
    let content = json!([{"type":"text","text":"same words"}]);
    let follow = |id, msg, intent| {
        rpc(
            id,
            "_nori/session/follow_up",
            json!({"session_id":"s","expected_turn":2,"message_id":msg,"intent":intent,"prompt":content}),
        )
    };
    let owner = rpc(
        2,
        "session/prompt",
        json!({"sessionId":"s","prompt":content}),
    );
    let init = rpc(1, "initialize", json!({"protocolVersion":1}));
    for (request, reply) in [
        (
            init,
            response(
                json!(1),
                json!({"protocolVersion":1, "agentCapabilities":{}, "agentInfo":{"name":"@agentclientprotocol/codex-acp","version":"2.7.0"},"_meta":{"steering":{"supported":true}}}),
            ),
        ),
        (
            rpc(9, "session/new", json!({"cwd":cwd,"mcpServers":[]})),
            response(json!(9), json!({"sessionId":"s"})),
        ),
    ] {
        connection.input.send(request.clone()).await?;
        assert_eq!(
            serde_json::from_str::<Value>(&adapter.next().await.context("request")??)?,
            request
        );
        adapter.send(reply.to_string()).await?;
        assert_eq!(connection.output.recv().await, Some(reply));
    }
    connection.input.send(owner.clone()).await?;
    assert_eq!(
        serde_json::from_str::<Value>(&adapter.next().await.context("owner")??)?,
        owner
    );
    let permission = rpc(
        42,
        "session/request_permission",
        json!({"sessionId":"s","options":[]}),
    );
    adapter.send(permission.to_string()).await?;
    assert_eq!(connection.output.recv().await, Some(permission));
    connection.input.send(follow(3, "one", "steer")).await?;
    // An unsupported implementation returns a JSON-RPC error to the caller here.
    let steer: Value = tokio::select! {
        line = adapter.next() => serde_json::from_str(&line.context("steer")??)?,
        reply = connection.output.recv() => anyhow::bail!("steer did not reach the adapter: {reply:?}"),
    };
    assert_eq!(steer["method"], "_session/steering");
    assert_eq!(
        steer["params"]["_meta"]["steering"]["idleBehavior"],
        "promptRequired"
    );
    connection.input.send(follow(4, "two", "queue")).await?;
    let queued = connection.output.recv().await.context("queue receipt")?;
    assert_eq!(queued["result"]["status"], "queued");
    let done = response(
        json!(2),
        json!({"stopReason":"end_turn","usage":{"inputTokens":7}}),
    );
    adapter.send(done.to_string()).await?;
    assert_eq!(connection.output.recv().await, Some(done));
    connection
        .input
        .send(rpc(
            5,
            "session/prompt",
            json!({"sessionId":"s","prompt":content}),
        ))
        .await?;
    assert_eq!(
        connection.output.recv().await.context("busy")?["error"]["code"],
        -32015
    );
    if late {
        connection
            .input
            .send(follow(7, "later-steer", "steer"))
            .await?;
        let mut receipts = std::collections::HashMap::new();
        for _ in 0..2 {
            let receipt = connection
                .output
                .recv()
                .await
                .context("uncertain or refused")?;
            receipts.insert(receipt["id"].clone(), receipt["result"]["status"].clone());
        }
        assert_eq!(receipts[&json!(3)], "unknown");
        assert_eq!(receipts[&json!(7)], "idle_queued");
    }
    adapter
        .send(
            response(
                steer["id"].clone(),
                json!({"outcome":"promptRequired","reason":"noRunningTurn"}),
            )
            .to_string(),
        )
        .await?;
    let expected_status = if late {
        "unknown"
    } else {
        let raced = connection.output.recv().await.context("race receipt")?;
        assert_eq!(raced["result"]["status"], "idle_queued");
        "idle_queued"
    };
    let answer = response(json!(42), json!({"outcome":{"outcome":"cancelled"}}));
    connection.input.send(answer.clone()).await?;
    assert_eq!(
        serde_json::from_str::<Value>(&adapter.next().await.context("permission answer")??)?,
        answer
    );
    for msg in if late {
        ["two", "later-steer"]
    } else {
        ["one", "two"]
    } {
        let prompt: Value = serde_json::from_str(&adapter.next().await.context("queued prompt")??)?;
        assert_eq!(prompt["method"], "session/prompt");
        assert_eq!(prompt["params"]["prompt"], content);
        assert_eq!(
            prompt["params"]["_meta"][nori_protocol::PROMPT_ECHO_ID_META_KEY],
            msg
        );
        adapter
            .send(response(prompt["id"].clone(), json!({"stopReason":"end_turn"})).to_string())
            .await?;
    }
    connection.input.send(follow(6, "one", "steer")).await?;
    while let Some(reply) = connection.output.recv().await {
        if reply["id"] == 6 {
            assert_eq!(reply["result"]["status"], expected_status);
            return Ok(());
        }
    }
    anyhow::bail!("no retry receipt")
}
