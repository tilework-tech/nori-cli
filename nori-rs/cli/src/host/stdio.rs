//! One managed launch served as standard ACP on stdin and stdout.
//!
//! A machine bridge that already pipes an adapter's stdio can run the host in
//! its place. The relay uses the host's own loopback control and managed
//! sockets, so admission, side sessions and process cleanup are the host's.

use std::path::Path;
use std::time::Duration;
use std::time::Instant;

use anyhow::Context;
use nori_harness::remote_agent::ManagedClient;
use serde_json::Value;
use serde_json::json;
use tokio::io::AsyncWriteExt;

use super::state::HostFiles;

const GENERATION: i64 = 1;

/// Admit `launch` on the dedicated binding and carry ACP between this
/// process's stdio and the adapter until stdin closes. The host is asked to
/// stop when the relay ends for any reason.
pub(super) async fn relay(
    files: &HostFiles,
    binding: &str,
    launch: &Path,
    inherit_env: bool,
) -> anyhow::Result<()> {
    let outcome = async {
        let deadline = Instant::now() + Duration::from_secs(15);
        let (mut control, host) = loop {
            match super::connect(files).await {
                Ok(connected) => break connected,
                Err(error) if Instant::now() >= deadline => {
                    return Err(error).context("the host did not start");
                }
                Err(_) => tokio::time::sleep(Duration::from_millis(25)).await,
            }
        };
        let mut spec: Value = serde_json::from_slice(
            &std::fs::read(launch).context("reading the launch specification")?,
        )
        .context("the launch specification is not JSON")?;
        if inherit_env {
            let env = spec
                .as_object_mut()
                .context("the launch specification must be a JSON object")?
                .entry("env")
                .or_insert_with(|| json!({}))
                .as_object_mut()
                .context("the launch specification env must be an object")?;
            for (key, value) in std::env::vars_os() {
                let (Some(key), Some(value)) = (key.to_str(), value.to_str()) else {
                    continue;
                };
                if !nori_harness::managed_launch::reserved_launch_env(key)
                    && !key.is_empty()
                    && !key.contains('=')
                    && !value.contains('\0')
                {
                    env.entry(key).or_insert_with(|| json!(value));
                }
            }
        }

        control
            .request(
                "_nori/session/ensure",
                json!({
                    "commandId": "stdio",
                    "conversationId": uuid::Uuid::new_v4().to_string(),
                    "bindingId": binding,
                    "generation": GENERATION,
                    "spec": spec,
                }),
            )
            .await
            .context("the host refused the launch")?;
        let (_, token) = files.read()?;
        let mut managed =
            ManagedClient::connect(&host.endpoint.replace("/sessions", "/managed"), &token).await?;

        // A pending read on Tokio's stdin keeps the runtime from shutting
        // down, so a host stopped by a signal while its pipe is still open
        // would never exit. A plain thread ends with the process.
        let (lines, mut input) = tokio::sync::mpsc::channel(64);
        std::thread::spawn(move || {
            for line in std::io::stdin().lines() {
                if lines.blocking_send(line).is_err() {
                    break;
                }
            }
        });
        let mut output = tokio::io::stdout();
        loop {
            let line = tokio::select! {
                line = input.recv() => {
                    let Some(line) = line.transpose()? else {
                        return Ok(());
                    };
                    if line.trim().is_empty() {
                        continue;
                    }
                    match serde_json::from_str::<Value>(&line) {
                        Ok(message) => {
                            managed
                                .send(json!({
                                    "bindingId": binding,
                                    "generation": GENERATION,
                                    "message": message,
                                }))
                                .await?;
                            continue;
                        }
                        Err(_) => json!({
                            "jsonrpc": "2.0",
                            "id": null,
                            "error": {"code": -32700, "message": "Parse error"},
                        }),
                    }
                }
                frame = managed.recv() => {
                    let mut frame = frame?;
                    anyhow::ensure!(frame["closed"] != true, "the adapter connection closed");
                    match frame.get_mut("message") {
                        Some(message) => message.take(),
                        None => continue,
                    }
                }
            };
            output.write_all(format!("{line}\n").as_bytes()).await?;
            output.flush().await?;
        }
    }
    .await;
    if let Ok((mut control, _)) = super::connect(files).await {
        // Shutdown may close the socket before its reply.
        let _ = control
            .send(json!({"jsonrpc":"2.0","id":2,"method":"_nori/host/stop","params":{}}))
            .await;
    }
    outcome
}
