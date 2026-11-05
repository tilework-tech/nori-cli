//! Run with `cargo test -p nori-harness --test context_timeline profile -- --ignored --nocapture`.

use super::*;
use pretty_assertions::assert_eq;
use std::io::Write;
use std::path::Path;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Instant;
use tracing::instrument::WithSubscriber;

#[expect(
    clippy::expect_used,
    reason = "fixture construction diagnoses invalid test data"
)]
fn rich_usage(turn: i64) -> SessionEvent {
    let metadata = json!({
        "provider": "profile-fixture", "model": "profile-model",
        "tokens": {"input": turn * 10, "output": 10, "cached": 1000},
        "details": {"source": "session_usage", "labels": ["context", "checkpoint"]}
    });
    SessionEvent::Acp(AcpEvent::Notification(
        acp::AgentNotification::SessionNotification(acp::SessionNotification::new(
            "agent-session",
            acp::SessionUpdate::UsageUpdate(
                acp::UsageUpdate::new(
                    (1000 + turn * 10)
                        .try_into()
                        .expect("fixture usage fits the protocol token count"),
                    1_000_000,
                )
                .meta(
                    metadata
                        .as_object()
                        .expect("fixture metadata is a JSON object")
                        .clone(),
                ),
            ),
        )),
    ))
}

fn write_fixture(home: &Path, turns: i64, message_bytes: i64) -> anyhow::Result<i64> {
    let directory = home.join("transcripts/by-project/project/sessions");
    std::fs::create_dir_all(&directory)?;
    let path = directory.join("timeline.jsonl");
    let mut file = std::io::BufWriter::new(std::fs::File::create(&path)?);
    writeln!(
        file,
        "{}",
        json!({
            "ts":"2026-01-01T00:00:00Z", "v":3, "type":"session_meta",
            "session_id":"timeline", "project_id":"project", "started_at":"2026-01-01T00:00:00Z",
            "cwd":home, "cli_version":"profile", "acp_session_id":"agent-session"
        })
    )?;
    let mut index = 0_i64;
    let mut record = |event: SessionEvent| -> anyhow::Result<()> {
        let observed_at = (chrono::DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z")?
            + chrono::Duration::milliseconds(index))
        .to_rfc3339();
        writeln!(
            file,
            "{}",
            json!({
                "ts":observed_at, "v":3, "type":"session_event", "event":event,
                "observation_id":format!("profile-{index}"), "observed_at":observed_at
            })
        )?;
        index += 1;
        Ok(())
    };
    record(rich_usage(0))?;
    let text = "x".repeat(message_bytes.try_into()?);
    for turn in 1..=turns {
        record(phase(turn))?;
        for update in [
            acp::SessionUpdate::UserMessageChunk(acp::ContentChunk::new(acp::ContentBlock::Text(
                acp::TextContent::new(text.clone()),
            ))),
            acp::SessionUpdate::AgentMessageChunk(acp::ContentChunk::new(acp::ContentBlock::Text(
                acp::TextContent::new(text.clone()),
            ))),
        ] {
            record(SessionEvent::Acp(AcpEvent::Notification(
                acp::AgentNotification::SessionNotification(acp::SessionNotification::new(
                    "agent-session",
                    update,
                )),
            )))?;
        }
        record(rich_usage(turn))?;
        record(end(turn))?;
    }
    file.flush()?;
    Ok(std::fs::metadata(path)?.len().try_into()?)
}

#[expect(
    clippy::expect_used,
    reason = "fixture assertions diagnose missing expected evidence"
)]
fn validate(timeline: &nori_harness::context_timeline::ContextTimeline, turns: i64) {
    assert_eq!(
        i64::try_from(timeline.turns.len()).expect("fixture turn count fits i64"),
        turns
    );
    assert_eq!(
        timeline
            .current
            .as_ref()
            .expect("fixture has a current usage observation")
            .usage
            .used,
        u64::try_from(1000 + turns * 10).expect("fixture usage is nonnegative")
    );
    assert_eq!(
        timeline
            .current
            .as_ref()
            .expect("fixture has a current usage observation")
            .usage
            .meta
            .as_ref()
            .expect("fixture usage retains provider metadata")["provider"],
        "profile-fixture"
    );
    assert!(!timeline.incomplete_history);
    assert!(timeline.turns.iter().all(|turn| turn.end.is_some()));
    assert_eq!(
        timeline
            .segments
            .iter()
            .map(|segment| segment.tokens)
            .sum::<i64>(),
        1000 + turns * 10
    );
}

#[derive(Clone)]
struct TraceCapture(Arc<Mutex<Vec<u8>>>);

impl Write for TraceCapture {
    #[expect(
        clippy::expect_used,
        reason = "a poisoned trace capture must fail the test"
    )]
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0
            .lock()
            .expect("trace capture mutex is not poisoned")
            .extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[tokio::test]
#[serial_test::serial]
async fn profile_fixture_preserves_checkpoints_and_emits_content_free_snapshot_timing()
-> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    write_fixture(dir.path(), 2, 1024)?;
    let transcript = TranscriptLoader::new(dir.path().into())
        .load_transcript("project", "timeline")
        .await?;
    assert_eq!(transcript.records().count(), 11);
    let expected = transcript.context_timeline();
    validate(&expected, 2);
    // SAFETY: tests that configure mock-agent environment variables are serialized.
    unsafe {
        std::env::set_var("MOCK_AGENT_SUPPORT_LOAD_SESSION", "1");
    }
    let _guard = EnvGuard(vec!["MOCK_AGENT_SUPPORT_LOAD_SESSION"]);
    let mut session = launch(
        dir.path(),
        nori_harness::runtime::SessionStart::Resume(nori_harness::runtime::SessionResume {
            acp_session_id: transcript.meta.acp_session_id.clone(),
            transcript: Some(transcript),
        }),
    )
    .await;
    started(&mut session).await;
    let captured = TraceCapture(Arc::default());
    let writer = captured.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .with_ansi(false)
        .without_time()
        .with_writer(move || writer.clone())
        .finish();
    let actual = session
        .handle
        .context_timeline()
        .with_subscriber(subscriber)
        .await?;
    session.handle.shutdown().await?;
    assert_eq!(actual, expected);
    let trace = String::from_utf8(captured.0.lock().unwrap().clone())?;
    for field in [
        "nori_context_profile",
        "harness_snapshot",
        "lock_wait_us",
        "clone_us",
        "turn_count",
        "segment_count",
    ] {
        assert!(
            trace.contains(field),
            "missing {field} in snapshot trace: {trace}"
        );
    }
    assert!(!trace.contains("profile-fixture"));
    assert!(!trace.contains("agent-session"));
    Ok(())
}

fn report(
    file: &mut std::fs::File,
    turns: i64,
    message_bytes: i64,
    file_bytes: i64,
    stage: &str,
    samples: &[f64],
) -> anyhow::Result<()> {
    let mut ordered = samples.to_vec();
    ordered.sort_by(f64::total_cmp);
    let row = json!({"component":"harness", "stage":stage, "turns":turns,
        "message_bytes":message_bytes, "fixture_bytes":file_bytes, "samples_ms":samples,
        "median_ms":ordered[ordered.len()/2], "max_ms":ordered.last(),
        "build":if cfg!(debug_assertions) { "debug" } else { "release" },
        "cache_state":"warm_after_first_sample", "includes_fixture_generation":false});
    writeln!(file, "{row}")?;
    println!("{row}");
    Ok(())
}

#[tokio::test]
#[ignore = "manual context profiling; generates large real transcripts and launches mock ACP"]
#[serial_test::serial]
async fn profile_load_reduce_and_live_snapshot() -> anyhow::Result<()> {
    let output = std::env::var_os("NORI_CONTEXT_PROFILE_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.local/context-profile/integration")
        });
    std::fs::create_dir_all(&output)?;
    let mut results = std::fs::File::create(output.join("harness.jsonl"))?;
    // SAFETY: tests that configure mock-agent environment variables are serialized.
    unsafe {
        std::env::set_var("MOCK_AGENT_SUPPORT_LOAD_SESSION", "1");
    }
    let _guard = EnvGuard(vec!["MOCK_AGENT_SUPPORT_LOAD_SESSION"]);
    for (turns, message_bytes) in [
        (0, 1024),
        (100, 1024),
        (1000, 1024),
        (10_000, 1024),
        (1000, 16_384),
    ] {
        let dir = tempfile::tempdir()?;
        let file_bytes = write_fixture(dir.path(), turns, message_bytes)?;
        let loader = TranscriptLoader::new(dir.path().into());
        let mut load_samples = Vec::new();
        let mut reduce_samples = Vec::new();
        for _ in 0..7 {
            let timer = Instant::now();
            let transcript = loader.load_transcript("project", "timeline").await?;
            load_samples.push(timer.elapsed().as_secs_f64() * 1000.0);
            assert_eq!(i64::try_from(transcript.records().count())?, 1 + turns * 5);
            let timer = Instant::now();
            let timeline = transcript.context_timeline();
            reduce_samples.push(timer.elapsed().as_secs_f64() * 1000.0);
            validate(&timeline, turns);
        }
        report(
            &mut results,
            turns,
            message_bytes,
            file_bytes,
            "jsonl_load",
            &load_samples,
        )?;
        report(
            &mut results,
            turns,
            message_bytes,
            file_bytes,
            "timeline_reduce",
            &reduce_samples,
        )?;
        let transcript = loader.load_transcript("project", "timeline").await?;
        let expected = transcript.context_timeline();
        let mut session = launch(
            dir.path(),
            nori_harness::runtime::SessionStart::Resume(nori_harness::runtime::SessionResume {
                acp_session_id: transcript.meta.acp_session_id.clone(),
                transcript: Some(transcript),
            }),
        )
        .await;
        started(&mut session).await;
        let mut live_samples = Vec::new();
        for _ in 0..7 {
            let timer = Instant::now();
            let timeline = session.handle.context_timeline().await?;
            live_samples.push(timer.elapsed().as_secs_f64() * 1000.0);
            assert_eq!(timeline, expected);
        }
        session.handle.shutdown().await?;
        report(
            &mut results,
            turns,
            message_bytes,
            file_bytes,
            "live_snapshot",
            &live_samples,
        )?;
    }
    Ok(())
}
