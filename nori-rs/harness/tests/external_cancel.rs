//! Guard cancellations by the activity the caller observed, even when the agent
//! starts the first turn during session bootstrap.
use std::sync::Arc;
use std::time::Duration;

use nori_config::NoriConfig;
use nori_harness::runtime::AgentPrepareSpec;
use nori_harness::runtime::LaunchedSession;
use nori_harness::runtime::SessionLaunchSpec;
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

#[tokio::test]
async fn immediate_external_turns_fence_late_cancels_and_keep_unconfirmed_work_live()
-> anyhow::Result<()> {
    use ExternalTurnState::*;
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
    let mut session = launch_session(SessionLaunchSpec {
        agent,
        start: SessionStart::New,
    });
    let first = next_turn(&mut session, Working).await?;
    session
        .handle
        .cancel_external_turn(first.id.clone())
        .await?;
    assert_eq!(next_turn(&mut session, CancelRequested).await?.id, first.id);
    assert_eq!(next_turn(&mut session, Cancelled).await?.id, first.id);
    let next = next_turn(&mut session, Working).await?;
    assert_ne!(first.id, next.id);
    assert!(session.handle.cancel_external_turn(first.id).await.is_err());
    session.handle.cancel_external_turn(next.id.clone()).await?;
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
