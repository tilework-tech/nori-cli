//! One-shot `nori exec "prompt"`: a thin ACP v1 client of `nori host`.
//!
//! It runs on the home's running host when that host speaks this contract
//! and gives it control, and on an in-process host otherwise. Either way it
//! starts one session, sends one prompt, collects the agent's text, rejects
//! every permission request, and closes the session.

use std::path::Path;
use std::time::Duration;

use anyhow::Context;
use nori_config::NoriConfig;
use nori_config::NoriConfigOverrides;
use nori_harness::remote_agent::ManagedClient;
use nori_harness::remote_agent::RemoteAcpServer;
use serde_json::Value;
use serde_json::json;
use tokio::sync::mpsc;

/// What one prompt produced.
pub(crate) struct Outcome {
    /// The agent's text from the turn.
    pub(crate) output: String,
    /// Whether a permission request was rejected.
    pub(crate) permission_denied: bool,
}

/// How the prompt runs.
pub(crate) struct Request {
    pub(crate) prompt: String,
    /// The agent named on the command line, if any.
    pub(crate) agent: Option<String>,
    /// Approve every request instead of asking.
    pub(crate) bypass: bool,
    /// The run carries configuration a running host would not apply, so it
    /// must have a host of its own.
    pub(crate) own_host: bool,
    /// Told once the prompt has reached the agent.
    pub(crate) analytics: Option<nori_installed::AnalyticsReporter>,
}

/// Run `request` on a running host or on one of its own.
pub(crate) async fn run(
    config: (NoriConfig, NoriConfigOverrides),
    request: Request,
) -> anyhow::Result<Outcome> {
    let outcome = run_on_a_host(config, &request).await;
    if let Some(analytics) = &request.analytics {
        analytics.flush();
    }
    outcome
}

async fn run_on_a_host(
    (config, overrides): (NoriConfig, NoriConfigOverrides),
    request: &Request,
) -> anyhow::Result<Outcome> {
    let cwd = config.cwd.clone();
    if !request.own_host
        && let Some(client) = running_host(&config.nori_home).await
    {
        return prompt(&mut Wire::Socket(Box::new(client)), &cwd, request).await;
    }
    let host = super::local_stdio::local_host(config, overrides)?;
    let (to_host, from_client) = mpsc::channel(64);
    let (to_client, from_host) = mpsc::channel(1024);
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let mut served = tokio::spawn(RemoteAcpServer::serve_host_pipe(
        host.clone(),
        from_client,
        to_client,
        stopped,
    ));
    let mut wire = Wire::Pipe {
        input: to_host,
        output: from_host,
    };
    let outcome = prompt(&mut wire, &cwd, request).await;
    let _ = stop.send(());
    // Keep reading until the host has stopped, so nothing it still sends
    // finds the pipe closed.
    let served = loop {
        tokio::select! {
            served = &mut served => break served,
            message = wire.recv() => if message.is_err() {
                break served.await;
            },
        }
    };
    host.manager.shutdown(Duration::ZERO).await;
    let outcome = outcome?;
    served??;
    Ok(outcome)
}

/// A client of the home's running host, initialized as its controller, when
/// that host speaks this contract and no other client controls it.
async fn running_host(home: &Path) -> Option<ManagedClient> {
    // Read discovery without touching the host lock; connecting proves the
    // host is alive.
    let path = home.join("host").join("host.json");
    if !path.exists() {
        return None;
    }
    let discovery: nori_protocol::HostDiscovery =
        serde_json::from_reader(super::state::private_open(&path, false).ok()?).ok()?;
    if discovery.contract.acp != 1 || discovery.contract.nori != super::acp::handshake::CONTRACT {
        return None;
    }
    tokio::time::timeout(Duration::from_secs(3), async {
        let mut client = ManagedClient::connect(&discovery.endpoint, &discovery.token)
            .await
            .ok()?;
        let initialized = client
            .request("initialize", initialize_params())
            .await
            .ok()?;
        (initialized["control"]["role"] == "controller").then_some(client)
    })
    .await
    .ok()
    .flatten()
}

fn initialize_params() -> Value {
    json!({
        "protocolVersion": 1,
        "clientCapabilities": {},
        "clientInfo": {"name": "nori-exec", "version": env!("CARGO_PKG_VERSION")},
    })
}

/// The client end of a connection to a host.
enum Wire {
    Socket(Box<ManagedClient>),
    Pipe {
        input: mpsc::Sender<String>,
        output: mpsc::Receiver<String>,
    },
}

impl Wire {
    async fn send(&mut self, message: Value) -> anyhow::Result<()> {
        match self {
            Self::Socket(client) => client.send(message).await,
            Self::Pipe { input, .. } => input
                .send(message.to_string())
                .await
                .map_err(|_| anyhow::anyhow!("the host stopped")),
        }
    }

    async fn recv(&mut self) -> anyhow::Result<Value> {
        match self {
            Self::Socket(client) => client.recv().await,
            Self::Pipe { output, .. } => {
                let line = output.recv().await.context("the host stopped")?;
                Ok(serde_json::from_str(&line)?)
            }
        }
    }

    /// Send a request and wait for its result, answering anything the host
    /// asks meanwhile with `method not found`.
    async fn call(&mut self, id: &str, method: &str, params: Value) -> anyhow::Result<Value> {
        self.send(json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))
            .await?;
        loop {
            let message = self.recv().await?;
            if message.get("method").is_none() && message["id"] == id {
                return result(message);
            }
            self.refuse(&message).await?;
        }
    }

    /// Answer a request from the host that this client does not handle.
    async fn refuse(&mut self, message: &Value) -> anyhow::Result<()> {
        if message["method"].is_string()
            && let Some(id) = message.get("id")
        {
            self.send(json!({"jsonrpc":"2.0","id":id,"error":{"code":-32601,"message":"method not found"}}))
                .await?;
        }
        Ok(())
    }
}

fn result(message: Value) -> anyhow::Result<Value> {
    match message.get("error") {
        Some(error) => Err(anyhow::anyhow!(
            "{}",
            error["message"]
                .as_str()
                .unwrap_or("the host refused the request")
        )),
        None => Ok(message["result"].clone()),
    }
}

async fn prompt(wire: &mut Wire, cwd: &Path, request: &Request) -> anyhow::Result<Outcome> {
    if let Wire::Pipe { .. } = wire {
        wire.call("initialize", "initialize", initialize_params())
            .await?;
    }
    let mut params = json!({
        "cwd": cwd.to_str().context("folder is not valid UTF-8")?,
        "mcpServers": [],
    });
    if let Some(agent) = &request.agent {
        params["_meta"] = serde_json::to_value(nori_protocol::NewSessionMeta {
            agent: Some(agent.clone()),
        })?;
    }

    let created = wire.call("new", "session/new", params).await?;
    let session = created["sessionId"]
        .as_str()
        .context("the host did not name the new session")?
        .to_owned();
    let mode = if request.bypass {
        "approve-all"
    } else {
        "agent"
    };
    wire.call(
        "mode",
        "session/set_mode",
        json!({"sessionId": session, "modeId": mode}),
    )
    .await?;
    wire.send(
        json!({"jsonrpc":"2.0","id":"prompt","method":"session/prompt","params":{
            "sessionId": session,
            "prompt": [{"type": "text", "text": request.prompt}],
        }}),
    )
    .await?;
    let mut outcome = Outcome {
        output: String::new(),
        permission_denied: false,
    };
    let interrupted = interrupted();
    tokio::pin!(interrupted);
    let ended = loop {
        let message = tokio::select! {
            message = wire.recv() => message?,
            signal = &mut interrupted => {
                // The turn would otherwise go on without anyone reading it.
                let _ = wire
                    .send(json!({"jsonrpc":"2.0","method":"session/cancel","params":{"sessionId": session}}))
                    .await;
                let _ = tokio::time::timeout(
                    Duration::from_secs(5),
                    wire.call("close", "session/close", json!({"sessionId": session})),
                )
                .await;
                anyhow::bail!("{signal}");
            }
        };
        match message["method"].as_str() {
            Some("session/update") if message["params"]["sessionId"] == session.as_str() => {
                let update = &message["params"]["update"];
                if update["sessionUpdate"] == "agent_message_chunk"
                    && update["content"]["type"] == "text"
                    && let Some(text) = update["content"]["text"].as_str()
                {
                    outcome.output.push_str(text);
                }
            }
            Some("session/request_permission")
                if message.get("id").is_some()
                    && message["params"]["sessionId"] == session.as_str() =>
            {
                outcome.permission_denied = true;
                wire.send(json!({"jsonrpc":"2.0","id":message["id"],"result":{"outcome":reject(&message["params"]["options"])}}))
                    .await?;
            }
            Some(_) => wire.refuse(&message).await?,
            None if message["id"] == "prompt" => {
                if let Some(analytics) = &request.analytics {
                    analytics.session_started();
                }
                break result(message);
            }
            None => {}
        }
    };
    // The turn's result stands even if the session cannot be closed, for
    // example because another client took control meanwhile.
    let closed = wire
        .call("close", "session/close", json!({"sessionId": session}))
        .await;
    ended?;
    if let Err(error) = closed {
        tracing::warn!(%error, "the finished session could not be closed");
    }
    Ok(outcome)
}

/// Completes with a description when this process is asked to stop.
async fn interrupted() -> &'static str {
    #[cfg(unix)]
    {
        let Ok(mut terminate) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        else {
            let _ = tokio::signal::ctrl_c().await;
            return "interrupted";
        };
        tokio::select! {
            _ = tokio::signal::ctrl_c() => "interrupted",
            _ = terminate.recv() => "terminated",
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
        "interrupted"
    }
}

/// The outcome that rejects a permission request: its reject option, or
/// `cancelled` when it offers none.
fn reject(options: &Value) -> Value {
    options
        .as_array()
        .into_iter()
        .flatten()
        .find(|option| option["kind"] == "reject_once" || option["kind"] == "reject_always")
        .map_or_else(
            || json!({"outcome": "cancelled"}),
            |option| json!({"outcome": "selected", "optionId": option["optionId"]}),
        )
}
