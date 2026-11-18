//! ACP dialogue with one independently supervised, read-only adapter.

use std::time::Duration;

use anyhow::Context;
use anyhow::Result;
use anyhow::ensure;
use serde_json::Value;
use serde_json::json;
use tokio::sync::mpsc;

use super::SideLaunch;
use crate::backend::managed_additions::ManagedSessionAdditions;
use crate::managed_acp::StandardAcpConnection;
use crate::managed_acp::connect;

type Events = mpsc::Sender<(String, Option<Value>)>;

fn failure(id: Value, code: i32, reason: &str) -> Value {
    json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":reason}})
}

async fn exchange(acp: &mut StandardAcpConnection, method: &str, params: Value) -> Result<Value> {
    // Setup requests run sequentially before any client requests are admitted.
    acp.input
        .send(json!({"jsonrpc":"2.0","id":0,"method":method,"params":params}))
        .await?;
    loop {
        let message = acp
            .output
            .recv()
            .await
            .context("side adapter disconnected during setup")?;
        if message.get("method").is_some() {
            if let Some(id) = message.get("id") {
                acp.input
                    .send(failure(
                        id.clone(),
                        -32601,
                        "client services unavailable during side setup",
                    ))
                    .await?;
            }
            continue;
        }
        ensure!(message["id"] == 0, "unexpected side setup response");
        if let Some(error) = message.get("error") {
            anyhow::bail!(
                "{method} failed: {}",
                error["message"].as_str().unwrap_or("adapter error")
            );
        }
        ensure!(message["result"].is_object(), "invalid side setup result");
        return Ok(message["result"].clone());
    }
}

async fn open(
    acp: &mut StandardAcpConnection,
    mut init: Value,
    params: Value,
    model: Option<&str>,
) -> Result<(Value, String, String)> {
    // A side process can ask for permissions. Filesystem and terminal services
    // supplied by an ACP client would run outside its read-only mount.
    init["clientCapabilities"] = json!({});
    exchange(acp, "initialize", init).await?;
    let mut result = exchange(
        acp,
        "session/new",
        json!({"cwd":params["cwd"],"mcpServers":[]}),
    )
    .await?;
    let sid = result["sessionId"]
        .as_str()
        .filter(|id| !id.is_empty())
        .context("missing side adapter session ID")?
        .to_owned();
    let modes = result["modes"]["availableModes"]
        .as_array()
        .context("side adapter has no selectable permission mode")?;
    let mode = ["read-only", "plan", "explore", "ask", "default"]
        .into_iter()
        .find(|name| modes.iter().any(|mode| mode["id"] == *name))
        .context("side adapter has no supported restrictive mode")?;
    if result["modes"]["currentModeId"] != mode {
        exchange(
            acp,
            "session/set_mode",
            json!({"sessionId":sid,"modeId":mode}),
        )
        .await?;
        result["modes"]["currentModeId"] = json!(mode);
    }
    if let Some(model) = model
        && let Some(current) = result["models"]["currentModelId"].as_str()
        && current != model
    {
        exchange(
            acp,
            "session/set_model",
            json!({"sessionId":sid,"modelId":model}),
        )
        .await?;
        result["models"]["currentModelId"] = json!(model);
    }
    Ok((result, sid, mode.into()))
}

pub(super) fn start_side(
    launch: SideLaunch,
    init_params: Value,
    new_params: Value,
    request_id: Value,
    public_sid: String,
    events: Events,
) -> mpsc::Sender<Value> {
    let (input, mut requests) = mpsc::channel(32);
    tokio::spawn(async move {
        let SideLaunch { connection, owner } = launch;
        let spec = connection.spec;
        let additions = ManagedSessionAdditions::new(
            connection.home.join(".nori/cli"),
            connection.cwd.clone(),
            spec.agent.clone(),
            false,
            false,
        );
        let mut acp = connect(
            connection.stdout,
            connection.stdin,
            connection.cwd,
            additions,
            false,
            false,
        );
        let send = |message| events.send((public_sid.clone(), Some(message)));
        let setup = tokio::select! {
            result = tokio::time::timeout(Duration::from_secs(30), open(&mut acp, init_params, new_params, spec.model.as_deref())) =>
                result.context("side adapter setup timed out").and_then(|inner| inner),
            _ = requests.recv() => Err(anyhow::anyhow!("side session closed during setup")),
        };
        let final_reply = match setup {
            Ok((mut result, internal_sid, mode)) => {
                result["sessionId"] = json!(public_sid);
                if send(json!({"jsonrpc":"2.0","id":request_id,"result":result}))
                    .await
                    .is_ok()
                {
                    serve(
                        &mut acp,
                        &mut requests,
                        &events,
                        &public_sid,
                        &internal_sid,
                        &mode,
                    )
                    .await
                } else {
                    None
                }
            }
            Err(error) => Some(failure(request_id, -32000, &format!("{error:#}"))),
        };
        drop(acp);
        owner.stop().await;
        if let Some(message) = final_reply {
            let _ = send(message).await;
        }
        let _ = events.send((public_sid, None)).await;
    });
    input
}

async fn serve(
    acp: &mut StandardAcpConnection,
    requests: &mut mpsc::Receiver<Value>,
    events: &Events,
    public_sid: &str,
    internal_sid: &str,
    mode: &str,
) -> Option<Value> {
    loop {
        tokio::select! {
            request = requests.recv() => {
                let mut request = request?;
                if let Some(method) = request["method"].as_str() {
                    if method == "session/close" {
                        return Some(json!({"jsonrpc":"2.0","id":request["id"],"result":{}}));
                    }
                    if method == "session/set_mode" && request["params"]["modeId"] != mode {
                        events.send((public_sid.into(), Some(failure(request["id"].clone(), -32602, "side session permission mode is fixed")))).await.ok()?;
                        continue;
                    }
                    request["params"]["sessionId"] = json!(internal_sid);
                }
                acp.input.send(request).await.ok()?;
            }
            message = acp.output.recv() => {
                let mut message = message?;
                if let Some(method) = message["method"].as_str() {
                    if !matches!(method, "session/update" | "session/request_permission") || message["params"]["sessionId"] != internal_sid {
                        if let Some(id) = message.get("id") {
                            acp.input.send(failure(id.clone(), -32601, "client service unavailable on a side session")).await.ok()?;
                        }
                        continue;
                    }
                    message["params"]["sessionId"] = json!(public_sid);
                }
                events.send((public_sid.into(), Some(message))).await.ok()?;
            }
        }
    }
}
