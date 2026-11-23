//! Joining the home's running host from a stdio client.
//!
//! `nori host --stdio` and `nori exec --acp` relay their client to the
//! home's running background host when that host speaks this contract and
//! gives the client control, so the client shares that host's sessions.
//! Otherwise the caller serves the client from a host of its own.

use std::path::Path;
use std::time::Duration;

use nori_harness::remote_agent::ManagedClient;
use serde_json::Value;
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc;

/// What became of the client after trying to join the running host.
pub(super) enum Joined {
    /// The client was relayed to the running host until it left.
    Relayed,
    /// The client needs a host of its own; it starts with this line, already
    /// read from stdin.
    Declined(Option<Vec<u8>>),
}

/// The home's running host's discovery, when that host speaks this
/// contract. Reading it does not touch the host lock; only connecting proves
/// the host is alive.
pub(super) fn compatible_discovery(home: &Path) -> Option<nori_protocol::HostDiscovery> {
    let path = home.join("host").join("host.json");
    if !path.exists() {
        return None;
    }
    let discovery: nori_protocol::HostDiscovery =
        serde_json::from_reader(super::state::private_open(&path, false).ok()?).ok()?;
    (discovery.contract.acp == 1 && discovery.contract.nori == super::acp::handshake::CONTRACT)
        .then_some(discovery)
}

/// Relay the client on `lines` and stdout to the home's running host if
/// that host gives its `initialize` control.
pub(super) async fn relay(
    home: &Path,
    lines: &mut mpsc::Receiver<std::io::Result<Vec<u8>>>,
) -> anyhow::Result<Joined> {
    let Some(discovery) = compatible_discovery(home) else {
        return Ok(Joined::Declined(None));
    };
    let first = loop {
        match lines.recv().await.transpose()? {
            None => return Ok(Joined::Declined(None)),
            Some(line) if line.trim_ascii().is_empty() => {}
            Some(line) => break line,
        }
    };
    let request: Option<Value> = serde_json::from_slice(&first).ok();
    let observing = request
        .as_ref()
        .is_some_and(|request| request["params"]["observe"] == true);
    let Some(id) = request
        .filter(|request| request["method"] == "initialize")
        .and_then(|request| request.get("id").cloned())
    else {
        return Ok(Joined::Declined(Some(first)));
    };
    let Some((client, reply)) = initialize(&discovery, &first, &id).await else {
        return Ok(Joined::Declined(Some(first)));
    };
    let reply: Value = serde_json::from_str(&reply)?;
    // A client that asked to watch watches the shared host; any other
    // client needs control, which it never takes from another client.
    if reply["result"]["control"]["role"] != "controller" && !observing {
        client.close().await;
        return Ok(Joined::Declined(Some(first)));
    }
    forward(client, reply.to_string(), lines).await?;
    Ok(Joined::Relayed)
}

/// Connect to the running host and send it the client's `initialize`,
/// returning the connection and its reply, or `None` if the host cannot be
/// reached.
async fn initialize(
    discovery: &nori_protocol::HostDiscovery,
    first: &[u8],
    id: &Value,
) -> Option<(ManagedClient, String)> {
    tokio::time::timeout(Duration::from_secs(3), async {
        let mut client = ManagedClient::connect(&discovery.endpoint, &discovery.token)
            .await
            .ok()?;
        client
            .send_text(String::from_utf8(first.to_vec()).ok()?)
            .await
            .ok()?;
        loop {
            let reply = client.recv_text().await.ok()??;
            let message: Value = serde_json::from_str(&reply).ok()?;
            if message.get("method").is_none() && message.get("id") == Some(id) {
                return Some((client, reply));
            }
        }
    })
    .await
    .ok()
    .flatten()
}

/// Copy lines to the host and the host's messages to stdout until either
/// side leaves. Once stdin closes, the replies to requests already sent are
/// still copied. Leaving disconnects only this client; the host and its
/// sessions keep running.
async fn forward(
    mut client: ManagedClient,
    reply: String,
    lines: &mut mpsc::Receiver<std::io::Result<Vec<u8>>>,
) -> anyhow::Result<()> {
    let mut stdout = tokio::io::stdout();
    stdout.write_all(format!("{reply}\n").as_bytes()).await?;
    stdout.flush().await?;
    #[cfg(unix)]
    let mut interrupt = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())?;
    #[cfg(unix)]
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    // Requests sent to the host and not yet answered.
    let mut pending = std::collections::HashSet::new();
    let mut input_open = true;
    loop {
        if !input_open && pending.is_empty() {
            client.close().await;
            return Ok(());
        }
        #[cfg(unix)]
        let signalled = async {
            tokio::select! {
                _ = interrupt.recv() => "interrupted",
                _ = terminate.recv() => "terminated",
            }
        };
        #[cfg(not(unix))]
        let signalled = async {
            let _ = tokio::signal::ctrl_c().await;
            "interrupted"
        };
        tokio::select! {
            line = lines.recv(), if input_open => {
                let Some(line) = line.transpose()? else {
                    input_open = false;
                    continue;
                };
                if line.trim_ascii().is_empty() {
                    continue;
                }
                // Not UTF-8, so not JSON either: the host answers it with a
                // parse error, as it does any other line that is not JSON.
                let text = String::from_utf8(line)
                    .unwrap_or_else(|_| char::REPLACEMENT_CHARACTER.to_string());
                if let Ok(request) = serde_json::from_str::<Value>(&text)
                    && request["method"].is_string()
                    && let Some(id) = request.get("id")
                {
                    pending.insert(id.to_string());
                }
                client.send_text(text).await?;
            }
            message = client.recv_text() => {
                let Some(message) = message? else {
                    eprintln!("nori: the host closed this client's connection");
                    return Ok(());
                };
                if let Ok(frame) = serde_json::from_str::<Value>(&message) {
                    match (frame.get("method"), frame.get("id")) {
                        (None, Some(id)) => {
                            pending.remove(&id.to_string());
                        }
                        // A request this client can no longer answer.
                        (Some(_), Some(_)) if !input_open => pending.clear(),
                        _ => {}
                    }
                }
                stdout.write_all(format!("{message}\n").as_bytes()).await?;
                stdout.flush().await?;
            }
            signal = signalled => {
                client.close().await;
                anyhow::bail!("{signal}");
            }
        }
    }
}
