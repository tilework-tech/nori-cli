//! ACP dialogue with one independently supervised, read-only adapter.

use std::path::Path;
use std::time::Duration;

use anyhow::Context;
use anyhow::Result;
use anyhow::ensure;
use serde_json::Value;
use serde_json::json;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tokio_util::sync::DropGuard;

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

fn option<'a>(result: &'a Value, category: &str) -> Option<&'a Value> {
    result["configOptions"]
        .as_array()?
        .iter()
        .find(|item| item["category"] == category)
}

async fn select_option(
    acp: &mut StandardAcpConnection,
    result: &mut Value,
    sid: &str,
    category: &str,
    value: &str,
) -> Result<()> {
    let item = option(result, category).context("missing advertised side option")?;
    if item["currentValue"] == value {
        return Ok(());
    }
    let id = item["id"]
        .as_str()
        .context("missing side configuration ID")?;
    let mut response = exchange(
        acp,
        "session/set_config_option",
        json!({"sessionId":sid,"configId":id,"value":value}),
    )
    .await?;
    result["configOptions"] = response["configOptions"].take();
    ensure!(
        option(result, category).is_some_and(|o| o["currentValue"] == value),
        "side adapter did not apply the requested {category}"
    );
    Ok(())
}

struct FixedMode {
    value: &'static str,
    config_id: Option<String>,
}

async fn open(
    acp: &mut StandardAcpConnection,
    mut init: Value,
    cwd: &Path,
    model: Option<&str>,
) -> Result<(Value, String, FixedMode)> {
    // A side process can ask for permissions. Filesystem and terminal services
    // supplied by an ACP client would run outside its read-only mount.
    init["clientCapabilities"] = json!({});
    exchange(acp, "initialize", init).await?;
    let mut result = exchange(acp, "session/new", json!({"cwd":cwd,"mcpServers":[]})).await?;
    let sid = result["sessionId"]
        .as_str()
        .filter(|id| !id.is_empty())
        .context("missing side adapter session ID")?
        .to_owned();
    if let Some(model) = model {
        if option(&result, "model").is_some() {
            select_option(acp, &mut result, &sid, "model", model).await?;
        } else if result["models"]["currentModelId"]
            .as_str()
            .is_some_and(|current| current != model)
        {
            exchange(
                acp,
                "session/set_model",
                json!({"sessionId":sid,"modelId":model}),
            )
            .await?;
            result["models"]["currentModelId"] = json!(model);
        }
    }

    let select = option(&result, "mode");
    let choices = select
        .map_or(
            &result["modes"]["availableModes"],
            |option| &option["options"],
        )
        .as_array()
        .context("side adapter has no selectable permission mode")?;
    let value = ["read-only", "plan", "explore", "ask", "default"]
        .into_iter()
        .find(|name| {
            choices.iter().any(|entry| {
                if select.is_some() {
                    entry["value"] == *name
                        || entry["options"]
                            .as_array()
                            .is_some_and(|group| group.iter().any(|item| item["value"] == *name))
                } else {
                    entry["id"] == *name
                }
            })
        })
        .context("side adapter has no supported restrictive mode")?;
    let config_id = if let Some(option) = select {
        Some(
            option["id"]
                .as_str()
                .context("missing mode option ID")?
                .to_owned(),
        )
    } else {
        None
    };
    if config_id.is_some() {
        select_option(acp, &mut result, &sid, "mode", value).await?;
    } else if result["modes"]["currentModeId"] != value {
        exchange(
            acp,
            "session/set_mode",
            json!({"sessionId":sid,"modeId":value}),
        )
        .await?;
        result["modes"]["currentModeId"] = json!(value);
    }
    Ok((result, sid, FixedMode { value, config_id }))
}

pub(super) struct SideInput {
    pub sender: mpsc::Sender<Value>,
    _cancel: DropGuard,
}

pub(super) fn start_side(
    launch: SideLaunch,
    init_params: Value,
    request_id: Value,
    public_sid: String,
    events: Events,
) -> SideInput {
    let (sender, mut requests) = mpsc::channel(32);
    let cancel = CancellationToken::new();
    let input = SideInput {
        sender,
        _cancel: cancel.clone().drop_guard(),
    };
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
            connection.cwd.clone(),
            additions,
            false,
            false,
        );
        let send = |message| events.send((public_sid.clone(), Some(message)));
        let run = async {
            let setup = tokio::time::timeout(
                Duration::from_secs(30),
                open(
                    &mut acp,
                    init_params,
                    &connection.cwd,
                    spec.model.as_deref(),
                ),
            )
            .await
            .context("side adapter setup timed out")
            .and_then(|inner| inner);
            match setup {
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
            }
        };
        let final_reply = tokio::select! {
            biased;
            _ = cancel.cancelled() => None,
            reply = run => reply,
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
    mode: &FixedMode,
) -> Option<Value> {
    loop {
        tokio::select! {
            request = requests.recv() => {
                let mut request = request?;
                if let Some(method) = request["method"].as_str() {
                    if method == "session/close" {
                        return Some(json!({"jsonrpc":"2.0","id":request["id"],"result":{}}));
                    }
                    if (method == "session/set_mode" && request["params"]["modeId"] != mode.value)
                        || (method == "session/set_config_option"
                            && !(mode.config_id.as_deref().is_some_and(|id| request["params"]["configId"] == id)
                                && request["params"]["value"] == mode.value)) {
                        events.send((public_sid.into(), Some(failure(request["id"].clone(), -32602, "side session permission mode is fixed")))).await.ok()?;
                        continue;
                    }
                    request["params"]["sessionId"] = json!(internal_sid);
                }
                acp.input.try_send(request).ok()?;
            }
            message = acp.output.recv() => {
                let mut message = message?;
                if let Some(method) = message["method"].as_str() {
                    if !matches!(method, "session/update" | "session/request_permission") || message["params"]["sessionId"] != internal_sid {
                        if let Some(id) = message.get("id") {
                            acp.input.try_send(failure(id.clone(), -32601, "client service unavailable on a side session")).ok()?;
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
