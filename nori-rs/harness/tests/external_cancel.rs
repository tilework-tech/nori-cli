//! Guard cancellations by the activity the caller observed, even when the agent
//! starts the first turn during session bootstrap.
use std::sync::Arc;
use std::time::Duration;

use nori_config::NoriConfig;
use nori_harness::runtime::AgentPrepareSpec;
use nori_harness::runtime::LaunchedSession;
use nori_harness::runtime::SessionLaunchSpec;
use nori_harness::runtime::SessionResume;
use nori_harness::runtime::SessionStart;
use nori_harness::runtime::launch_session;
use nori_harness::runtime::prepare_agent;
use nori_protocol::ExternalTurn;
use nori_protocol::ExternalTurnState;
use nori_protocol::NoriEvent;
use nori_protocol::SessionEvent;
use pretty_assertions::assert_eq;
use serde_json::json;

async fn next_turn(
    session: &mut LaunchedSession,
    state: ExternalTurnState,
) -> anyhow::Result<ExternalTurn> {
    tokio::time::timeout(Duration::from_secs(10), async {
        while let Some(event) = session.events.recv().await {
            if let SessionEvent::Nori(NoriEvent::ExternalTurnChanged(turn)) = event
                && turn.state == state
            {
                return Ok(turn);
            }
        }
        anyhow::bail!("external turn stream closed")
    })
    .await?
}

async fn hold_listing(
    dir: &tempfile::TempDir,
    session: &LaunchedSession,
) -> anyhow::Result<tokio::task::JoinHandle<anyhow::Result<()>>> {
    let cwd = dir.path().to_owned();
    let handle = session.handle.clone();
    let listing = tokio::spawn(async move {
        handle.list_sessions(cwd).await?;
        Ok(())
    });
    // The agent keeps the control request open until it receives the cancel.
    tokio::time::timeout(Duration::from_secs(5), async {
        while !dir.path().join("listing").exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await?;
    Ok(listing)
}

async fn launch(start: SessionStart) -> anyhow::Result<(tempfile::TempDir, LaunchedSession)> {
    let dir = tempfile::tempdir()?;
    let fixture =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/external_cancel.py");
    nori_harness::registry::initialize_registry(vec![serde_json::from_value(json!({
        "name":"Trail fixture","slug":"trail-fixture",
        "distribution":{"local":{"command":"python3","args":["-u",fixture]}}
    }))?])?;
    let config = NoriConfig {
        active_agent: "trail-fixture".into(),
        cwd: dir.path().into(),
        nori_home: dir.path().into(),
        ..Default::default()
    };
    let agent = prepare_agent(AgentPrepareSpec {
        config: Arc::new(config),
        cli_version: "fixture".into(),
        session_context: None,
        initial_context: None,
    })
    .await?;
    let session = launch_session(SessionLaunchSpec { agent, start });
    Ok((dir, session))
}

#[tokio::test]
async fn immediate_external_turns_fence_late_cancels_and_keep_unconfirmed_work_live()
-> anyhow::Result<()> {
    use ExternalTurnState::*;
    let (dir, mut session) = launch(SessionStart::New).await?;
    let first = next_turn(&mut session, Working).await?;
    let listing = hold_listing(&dir, &session).await?;
    let admitted = session.handle.submit_cancel_current_turn().await?;
    assert_eq!(next_turn(&mut session, CancelRequested).await?.id, first.id);
    admitted.await?;
    listing.await??;
    assert_eq!(next_turn(&mut session, Cancelled).await?.id, first.id);
    let next = next_turn(&mut session, Working).await?;
    assert_ne!(first.id, next.id);
    assert!(session.handle.cancel_external_turn(first.id).await.is_err());
    std::fs::remove_file(dir.path().join("listing"))?;
    let second_listing = hold_listing(&dir, &session).await?;
    tokio::time::timeout(
        Duration::from_secs(5),
        session.handle.cancel_external_turn(next.id.clone()),
    )
    .await??;
    second_listing.await??;
    session.handle.cancel_external_turn(next.id.clone()).await?;
    assert_eq!(next_turn(&mut session, Unconfirmed).await?.id, next.id);
    session.handle.cancel_current_turn().await?;
    assert_eq!(
        std::fs::read_to_string(dir.path().join("cancels"))?,
        "trail\ntrail\n"
    );
    session.handle.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn resumed_history_stays_passive_and_the_next_live_turn_can_be_cancelled()
-> anyhow::Result<()> {
    let (_dir, mut session) = launch(SessionStart::Resume(SessionResume {
        acp_session_id: Some("trail".into()),
        transcript: None,
    }))
    .await?;
    // The fixture replays one historical active marker and starts a new identified turn.
    let mut replay_finished = false;
    let current = tokio::time::timeout(Duration::from_secs(10), async {
        while let Some(event) = session.events.recv().await {
            match event {
                SessionEvent::Nori(NoriEvent::ReplayFinished) => replay_finished = true,
                SessionEvent::Nori(NoriEvent::ExternalTurnChanged(turn)) => {
                    assert!(
                        replay_finished,
                        "historical activity was offered for cancellation"
                    );
                    return Ok(turn);
                }
                _ => {}
            }
        }
        anyhow::bail!("missing live turn after load")
    })
    .await??;
    session
        .handle
        .cancel_external_turn(current.id.clone())
        .await?;
    assert_eq!(
        next_turn(&mut session, ExternalTurnState::Cancelled)
            .await?
            .id,
        current.id
    );
    session.handle.shutdown().await?;
    Ok(())
}
