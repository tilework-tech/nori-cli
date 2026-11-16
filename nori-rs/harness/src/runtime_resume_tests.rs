//! Stable transcript continuation through the real ACP runtime.

use super::*;
use crate::ConversationId;
use crate::transcript::AssistantEntry;
use crate::transcript::ContentBlock;
use crate::transcript::TranscriptEntry;
use crate::transcript::TranscriptLoader;
use crate::transcript::TranscriptRecorder;
use crate::transcript::UserEntry;
use pretty_assertions::assert_eq;
use serial_test::serial;

async fn seeded_transcript(home: &std::path::Path) -> (TranscriptRecorder, Transcript) {
    let recorder = TranscriptRecorder::new_forked(
        home,
        home,
        Some("mock-model".to_string()),
        "original-version",
        "original-agent-session".to_string(),
        "original-parent".to_string(),
        vec![
            TranscriptEntry::User(UserEntry {
                id: "original-question".to_string(),
                content: "question before failure".to_string(),
                attachments: Vec::new(),
            }),
            TranscriptEntry::Assistant(AssistantEntry {
                id: "original-answer".to_string(),
                content: vec![ContentBlock::Text {
                    text: "answer before failure".to_string(),
                }],
                agent: Some("mock-model".to_string()),
            }),
        ],
    )
    .await
    .expect("seed transcript");
    let transcript = TranscriptLoader::new(home.to_path_buf())
        .load_transcript(recorder.project_id(), recorder.session_id())
        .await
        .expect("load seed");
    (recorder, transcript)
}

fn resume_same_id(home: &std::path::Path, transcript: Transcript) -> LaunchedSession {
    let mut config = build_backend_config(AgentPrepareSpec {
        config: Arc::new(crate::config::NoriConfig {
            active_agent: "mock-model".to_string(),
            cwd: home.to_path_buf(),
            nori_home: home.to_path_buf(),
            ..Default::default()
        }),
        cli_version: "resumed-version".to_string(),
        session_context: None,
        initial_context: None,
    });
    config.conversation_id = Some(
        ConversationId::from_string(&transcript.meta.session_id).expect("valid conversation id"),
    );
    launch_session_from(
        SessionAgentSource::Prepare(Box::new(config)),
        SessionStart::Resume(SessionResume {
            acp_session_id: transcript.meta.acp_session_id.clone(),
            transcript: Some(transcript),
        }),
    )
}

async fn receive_until(
    session: &mut LaunchedSession,
    done: impl Fn(&SessionEvent) -> bool,
) -> Vec<SessionEvent> {
    tokio::time::timeout(std::time::Duration::from_secs(15), async {
        let mut events = Vec::new();
        loop {
            let event = session.events.recv().await.expect("session stream open");
            let finished = done(&event);
            events.push(event);
            if finished {
                return events;
            }
        }
    })
    .await
    .expect("runtime reaches expected event")
}

fn conversation_text(transcript: &Transcript) -> Vec<String> {
    crate::backend::transcript_to_replay_session_events(transcript)
        .into_iter()
        .filter_map(|event| match event {
            SessionEvent::Acp(nori_protocol::AcpEvent::Notification(
                nori_protocol::acp::v1::AgentNotification::SessionNotification(notification),
            )) => match notification.update {
                nori_protocol::acp::v1::SessionUpdate::UserMessageChunk(chunk)
                | nori_protocol::acp::v1::SessionUpdate::AgentMessageChunk(chunk) => {
                    match chunk.content {
                        nori_protocol::acp::v1::ContentBlock::Text(text) => Some(text.text),
                        _ => None,
                    }
                }
                _ => None,
            },
            _ => None,
        })
        .collect()
}

#[tokio::test]
#[serial]
async fn same_id_resume_preserves_history_metadata_and_isolates_the_previous_writer() {
    let home = tempfile::tempdir().expect("temporary home");
    let (old_recorder, original) = seeded_transcript(home.path()).await;
    let loader = TranscriptLoader::new(home.path().to_path_buf());
    let mut transcript = original.clone();
    for _ in 0..2 {
        let mut resumed = resume_same_id(home.path(), transcript);
        receive_until(&mut resumed, |event| {
            matches!(event, SessionEvent::Nori(NoriEvent::ReplayFinished))
        })
        .await;
        resumed
            .handle
            .flush_transcript()
            .await
            .expect("flush resume");
        // An earlier runtime can still own its writer when a replacement starts.
        old_recorder
            .record_user_message("late-old-write", "obsolete writer tail", Vec::new())
            .await
            .expect("old writer remains alive");
        old_recorder.flush().await.expect("flush old writer");
        transcript = loader
            .load_transcript(&original.meta.project_id, &original.meta.session_id)
            .await
            .expect("same canonical session is readable");
        assert!(
            transcript.records().all(|record| !matches!(
                record,
                crate::transcript::TranscriptRecord::User {
                    content: "obsolete writer tail"
                }
            )),
            "the superseded writer cannot append to the canonical transcript"
        );
        assert_eq!(
            conversation_text(&transcript),
            vec!["question before failure", "answer before failure"],
            "resume retains each prior message once and excludes writes from the superseded runtime"
        );
        let mut expected_meta = original.meta.clone();
        expected_meta.acp_session_id = transcript.meta.acp_session_id.clone();
        assert_eq!(transcript.meta, expected_meta);
        resumed.handle.shutdown().await.expect("shutdown resume");
        receive_until(&mut resumed, |event| {
            matches!(event, SessionEvent::Nori(NoriEvent::SessionEnded(_)))
        })
        .await;
        transcript = loader
            .load_transcript(&original.meta.project_id, &original.meta.session_id)
            .await
            .expect("load complete transcript for next resume");
    }
    old_recorder.shutdown().await.expect("shutdown old writer");
}

#[tokio::test]
#[serial]
async fn same_id_resume_publishes_a_complete_transcript_before_restoration_events() {
    let home = tempfile::tempdir().expect("temporary home");
    let (recorder, original) = seeded_transcript(home.path()).await;
    let mut resumed = resume_same_id(home.path(), original.clone());
    receive_until(&mut resumed, |event| {
        matches!(event, SessionEvent::Nori(NoriEvent::SessionStarted(_)))
    })
    .await;
    let restored = TranscriptLoader::new(home.path().to_path_buf())
        .load_transcript(recorder.project_id(), recorder.session_id())
        .await
        .expect("canonical transcript exists before restoration finishes");
    assert_eq!(conversation_text(&restored), conversation_text(&original));
    assert_eq!(restored.meta.started_at, original.meta.started_at);
    resumed.handle.shutdown().await.expect("shutdown resume");
    receive_until(&mut resumed, |event| {
        matches!(event, SessionEvent::Nori(NoriEvent::SessionEnded(_)))
    })
    .await;
    let ended = TranscriptLoader::new(home.path().to_path_buf())
        .load_transcript(recorder.project_id(), recorder.session_id())
        .await
        .expect("read terminal transcript without a sleep or explicit flush");
    assert!(ended.records().any(|record| matches!(
        record,
        crate::transcript::TranscriptRecord::SessionEvent(SessionEvent::Nori(
            NoriEvent::SessionEnded(_)
        ))
    )));
    recorder
        .shutdown()
        .await
        .expect("shutdown original recorder");
}

#[tokio::test]
#[serial]
async fn same_id_resume_persists_authoritative_native_load_history_once() {
    struct LoadEnvironment;
    impl Drop for LoadEnvironment {
        fn drop(&mut self) {
            // SAFETY: all environment-mutating runtime tests run serially.
            unsafe {
                std::env::remove_var("MOCK_AGENT_SUPPORT_LOAD_SESSION");
                std::env::remove_var("MOCK_AGENT_LOAD_SESSION_NOTIFICATION_COUNT");
            }
        }
    }
    // SAFETY: all environment-mutating runtime tests run serially.
    unsafe {
        std::env::set_var("MOCK_AGENT_SUPPORT_LOAD_SESSION", "1");
        std::env::set_var("MOCK_AGENT_LOAD_SESSION_NOTIFICATION_COUNT", "2");
    }
    let _environment = LoadEnvironment;
    let home = tempfile::tempdir().expect("temporary home");
    let (recorder, original) = seeded_transcript(home.path()).await;
    let mut resumed = resume_same_id(home.path(), original.clone());
    receive_until(&mut resumed, |event| {
        matches!(event, SessionEvent::Nori(NoriEvent::ReplayFinished))
    })
    .await;
    resumed.handle.flush_transcript().await.expect("flush load");
    let loaded = TranscriptLoader::new(home.path().to_path_buf())
        .load_transcript(recorder.project_id(), recorder.session_id())
        .await
        .expect("load native restored transcript by stable id");
    assert_eq!(
        conversation_text(&loaded),
        vec!["replay chunk 0", "replay chunk 1"],
        "successful native replay is authoritative even when the local transcript is older"
    );
    assert_eq!(loaded.meta, original.meta);
    resumed.handle.shutdown().await.expect("shutdown resume");
    recorder.shutdown().await.expect("shutdown original writer");
}
