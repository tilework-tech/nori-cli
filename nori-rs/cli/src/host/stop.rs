//! Stop over an authenticated connection, then wait for that generation to exit.

use std::fs::TryLockError;
use std::time::Duration;
use std::time::Instant;

use anyhow::Context;
use nori_harness::remote_agent::ManagedClient;
use nori_host_runtime::state;
use nori_host_runtime::state::HostFiles;
use nori_host_runtime::state::HostState;
use serde_json::json;

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct StopDiscovery {
    #[serde(flatten)]
    host: HostState,
    token: String,
    instance_id: uuid::Uuid,
}

pub(super) async fn stop(files: &HostFiles) -> anyhow::Result<()> {
    if !files.running()? {
        println!("No host running.");
        return Ok(());
    }
    let generation = state::private_open(&files.directory.join("state.json"), false)?;
    let state: HostState = serde_json::from_reader(&generation)?;
    // An older server leaves this inode unlocked. Modern servers lock it
    // before signaling readiness and unlink it before releasing ownership.
    let has_generation_lock = match generation.try_lock_shared() {
        Ok(()) => {
            generation.unlock()?;
            false
        }
        Err(TryLockError::WouldBlock) => true,
        Err(error) => return Err(error.into()),
    };
    let token = state::read_token(&files.directory.join("token"))?;
    let discovery_path = files.directory.join("host.json");
    let discovery: Option<StopDiscovery> = if discovery_path.try_exists()? {
        Some(serde_json::from_reader(state::private_open(
            &discovery_path,
            false,
        )?)?)
    } else {
        None
    };
    if let Some(discovery) = &discovery {
        anyhow::ensure!(
            discovery.host.pid == state.pid
                && discovery.host.endpoint == state.endpoint
                && discovery.token == token,
            "host discovery changed; retry stop"
        );
    }
    // In managed mode there is no host.json. Pin the state inode across the
    // separate credential read so a replacement cannot supply its own token.
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let original = generation.metadata()?;
        let current = std::fs::metadata(files.directory.join("state.json"))?;
        anyhow::ensure!(
            (original.dev(), original.ino()) == (current.dev(), current.ino()),
            "host discovery changed; retry stop"
        );
    }
    tokio::time::timeout(Duration::from_secs(5), async {
        let mut client = ManagedClient::connect_lifecycle(&state.endpoint, &token).await?;
        if let Some(discovery) = discovery {
            anyhow::ensure!(
                Some(discovery.instance_id) == client.host_instance(),
                "the connected host is a different instance; retry stop"
            );
        }
        client
            .request("initialize", json!({"observe":true}))
            .await?;
        // A listener may close the socket while stopping, before its reply.
        client
            .send(json!({"jsonrpc":"2.0","id":2,"method":"_nori/host/stop","params":{}}))
            .await
    })
    .await
    .context("host management connection timed out")??;
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if has_generation_lock {
            match generation.try_lock_shared() {
                Ok(()) => break,
                Err(TryLockError::WouldBlock) => {}
                Err(error) => return Err(error.into()),
            }
        } else if !files.running()?
            || state::read_token(&files.directory.join("token"))
                .is_ok_and(|current| current != token)
        {
            break;
        }
        anyhow::ensure!(
            Instant::now() < deadline,
            "host is still shutting down; check {}",
            files.directory.join("host.log").display()
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    println!("Host stopped.");
    Ok(())
}
