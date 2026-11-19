use std::sync::Arc;
use std::time::Duration;

use nori_config::NoriConfig;
use nori_harness::FollowUpIntent;
use nori_harness::FollowUpStatus;
use nori_harness::runtime::AgentPrepareSpec;
use nori_harness::runtime::SessionLaunchSpec;
use nori_harness::runtime::SessionStart;
use nori_harness::runtime::launch_session;
use nori_harness::runtime::prepare_agent;
use nori_protocol::acp::v1 as acp;
use pretty_assertions::assert_eq;
use serde_json::json;

fn content(text: &str) -> Vec<acp::ContentBlock> {
    vec![acp::ContentBlock::Text(acp::TextContent::new(text))]
}

#[tokio::test]
async fn follow_ups_stay_with_the_target_turn_and_retries_do_not_deliver_twice()
-> anyhow::Result<()> {
    let (dir, mut session) = launch("0.102.0").await?;
    let owner = session.handle.prompt(content("original")).await?;
    let submit = |id: &str, intent, text: &str| {
        session.handle.follow_up(
            id.into(),
            owner.clone(),
            intent,
            content(text),
            "operator".into(),
        )
    };
    use FollowUpIntent::Queue;
    use FollowUpIntent::Steer;
    assert_eq!(
        submit("first", Queue, "same").await?.status,
        FollowUpStatus::Queued
    );
    assert_eq!(
        submit("first", Queue, "same").await?.status,
        FollowUpStatus::Queued
    );
    assert!(submit("first", Queue, "changed").await.is_err());
    assert_eq!(
        submit("second", Queue, "same").await?.status,
        FollowUpStatus::Queued
    );
    assert_eq!(
        submit("join", Steer, "correct").await?.status,
        FollowUpStatus::Injected
    );
    assert_eq!(
        submit("join", Steer, "correct").await?.status,
        FollowUpStatus::Injected
    );
    assert_eq!(
        submit("refused", Steer, "refuse").await?.status,
        FollowUpStatus::Rejected
    );
    assert_eq!(
        submit("opaque", Steer, "unexpected result").await?.status,
        FollowUpStatus::Unknown
    );
    assert_eq!(
        submit("opaque", Steer, "unexpected result").await?.status,
        FollowUpStatus::Unknown
    );
    assert_eq!(
        submit("idle", Steer, "at boundary").await?.status,
        FollowUpStatus::IdleQueued
    );
    let permission = tokio::time::timeout(Duration::from_secs(5), async {
        while let Some(event) = session.events.recv().await {
            if let nori_protocol::SessionEvent::Acp(nori_protocol::AcpEvent::Request {
                request_id,
                ..
            }) = event
            {
                return Some(request_id);
            }
        }
        None
    })
    .await?
    .ok_or_else(|| anyhow::anyhow!("missing approval"))?;
    session
        .handle
        .respond_to_agent(
            permission,
            Ok(acp::ClientResponse::RequestPermissionResponse(
                acp::RequestPermissionResponse::new(acp::RequestPermissionOutcome::Selected(
                    acp::SelectedPermissionOutcome::new("deny"),
                )),
            )),
        )
        .await?;
    let observed = wait_for_observed(&dir, "prompt at boundary").await?;
    assert_eq!(
        observed.lines().collect::<Vec<_>>(),
        vec![
            "prompt original",
            "steer correct",
            "steer refuse",
            "steer unexpected result",
            "steer at boundary",
            "decision selected",
            "prompt same",
            "prompt same",
            "prompt at boundary",
        ]
    );
    assert_eq!(
        submit("late", Steer, "wrong turn").await?.status,
        FollowUpStatus::Rejected
    );
    session.handle.shutdown().await?;
    Ok(())
}

async fn launch(
    version: &str,
) -> anyhow::Result<(tempfile::TempDir, nori_harness::runtime::LaunchedSession)> {
    let dir = tempfile::tempdir()?;
    std::fs::write(dir.path().join("version"), version)?;
    let fixture =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/follow_up.py");
    nori_harness::registry::initialize_registry(vec![serde_json::from_value(json!({
        "name":"Follow-up fixture","slug":"follow-up-fixture",
        "distribution":{"local":{"command":"python3","args":["-u",fixture]}}
    }))?])?;
    let config = NoriConfig {
        active_agent: "follow-up-fixture".into(),
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
    let session = launch_session(SessionLaunchSpec {
        agent,
        start: SessionStart::New,
    });
    Ok((dir, session))
}

#[tokio::test]
async fn missing_steer_ack_fences_new_turns_while_control_stays_responsive() -> anyhow::Result<()> {
    let (dir, session) = launch("0.102.0").await?;
    let turn = session.handle.prompt(content("original")).await?;
    let submit = |id: &str, intent, text: &str| {
        session.handle.follow_up(
            id.to_owned(),
            turn.clone(),
            intent,
            content(text),
            "operator".into(),
        )
    };
    assert_eq!(
        submit("queue", FollowUpIntent::Queue, "held").await?.status,
        FollowUpStatus::Queued
    );
    let unknown = submit("uncertain", FollowUpIntent::Steer, "no receipt").await?;
    assert_eq!(unknown.status, FollowUpStatus::Unknown);
    let timeout = Duration::from_secs(2);
    assert_eq!(
        tokio::time::timeout(
            timeout,
            submit("uncertain", FollowUpIntent::Steer, "no receipt")
        )
        .await??,
        unknown
    );
    tokio::time::timeout(timeout, session.handle.cancel()).await??;
    assert_eq!(
        std::fs::read_to_string(dir.path().join("observed"))?,
        "prompt original\nsteer no receipt\n"
    );
    tokio::time::timeout(timeout, session.handle.shutdown()).await??;
    Ok(())
}

#[tokio::test]
async fn advertising_adapter_with_unverified_guard_runs_the_follow_up_as_a_prompt()
-> anyhow::Result<()> {
    let (dir, session) = launch("0.80.0").await?;
    let owner = session.handle.prompt(content("wait for release")).await?;
    let receipt = session
        .handle
        .follow_up(
            "legacy".into(),
            owner,
            FollowUpIntent::Steer,
            content("next"),
            "operator".into(),
        )
        .await?;
    assert_eq!(receipt.status, FollowUpStatus::UnsupportedQueued);
    assert_eq!(
        std::fs::read_to_string(dir.path().join("observed"))?,
        "prompt wait for release\n"
    );
    std::fs::write(dir.path().join("release"), "")?;
    let observed = wait_for_observed(&dir, "prompt next").await?;
    assert_eq!(observed, "prompt wait for release\nprompt next\n");
    session.handle.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn late_steer_reply_releases_the_queue_after_the_original_end_turn() -> anyhow::Result<()> {
    let (dir, session) = launch("0.102.0").await?;
    let owner = session.handle.prompt(content("original")).await?;
    let submit = |id: &str, intent, text: &str| {
        session.handle.follow_up(
            id.into(),
            owner.clone(),
            intent,
            content(text),
            "operator".into(),
        )
    };
    assert_eq!(
        submit("queued", FollowUpIntent::Queue, "held")
            .await?
            .status,
        FollowUpStatus::Queued
    );
    assert_eq!(
        submit("join", FollowUpIntent::Steer, "late receipt")
            .await?
            .status,
        FollowUpStatus::Unknown
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("observed"))?,
        "prompt original\nsteer late receipt\n"
    );
    std::fs::write(dir.path().join("release"), "")?;
    let observed = wait_for_observed(&dir, "prompt held").await?;
    assert_eq!(
        observed,
        "prompt original\nsteer late receipt\nprompt held\n"
    );
    session.handle.shutdown().await?;
    Ok(())
}

async fn wait_for_observed(dir: &tempfile::TempDir, marker: &str) -> anyhow::Result<String> {
    Ok(tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let observed = std::fs::read_to_string(dir.path().join("observed")).unwrap_or_default();
            if observed.contains(marker) {
                return observed;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await?)
}

#[tokio::test]
async fn confirmed_correction_is_published_before_a_fast_adapter_reply() -> anyhow::Result<()> {
    let (_dir, mut session) = launch("0.102.0").await?;
    let owner = session.handle.prompt(content("original")).await?;
    let receipt = session
        .handle
        .follow_up(
            "early".into(),
            owner,
            FollowUpIntent::Steer,
            content("stream early"),
            "operator".into(),
        )
        .await?;
    assert_eq!(receipt.status, FollowUpStatus::Injected);
    let messages = tokio::time::timeout(Duration::from_secs(3), async {
        let mut messages = Vec::new();
        while let Some(event) = session.events.recv().await {
            if let nori_protocol::SessionEvent::Acp(nori_protocol::AcpEvent::Notification(
                acp::AgentNotification::SessionNotification(notification),
            )) = event
                && let acp::SessionUpdate::UserMessageChunk(chunk)
                | acp::SessionUpdate::AgentMessageChunk(chunk) = notification.update
                && let acp::ContentBlock::Text(text) = chunk.content
            {
                messages.push(text.text);
                if messages.len() == 3 {
                    break;
                }
            }
        }
        messages
    })
    .await?;
    assert_eq!(
        messages,
        ["original", "stream early", "agent observed correction"]
    );
    session.handle.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn only_input_from_the_local_composer_can_be_reclaimed() -> anyhow::Result<()> {
    let (dir, session) = launch("0.102.0").await?;
    let owner = session.handle.prompt(content("wait for release")).await?;
    let remote = session
        .handle
        .follow_up(
            "remote".into(),
            owner.clone(),
            FollowUpIntent::Queue,
            content("controller input"),
            "local".into(),
        )
        .await?;
    let local = session
        .handle
        .local_follow_up(
            "local-draft".into(),
            owner,
            FollowUpIntent::Queue,
            content("editable input"),
        )
        .await?;
    assert_eq!(remote.status, FollowUpStatus::Queued);
    assert_eq!(local.status, FollowUpStatus::Queued);
    assert_eq!(
        session
            .handle
            .reclaim_queued_prompt(remote.message_id)
            .await?,
        None
    );
    assert_eq!(
        session
            .handle
            .reclaim_queued_prompt(local.message_id)
            .await?,
        Some("editable input".to_owned())
    );
    std::fs::write(dir.path().join("release"), "")?;
    assert_eq!(
        wait_for_observed(&dir, "prompt controller input").await?,
        "prompt wait for release\nprompt controller input\n"
    );
    session.handle.shutdown().await?;
    Ok(())
}
