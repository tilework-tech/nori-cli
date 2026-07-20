use nori_harness::transcript::TranscriptLoader;
use nori_harness::transcript::TranscriptRecord;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn v2_storage_decodes_to_a_schema_independent_public_projection() {
    let nori_home = tempfile::tempdir().expect("create Nori home");
    let session_dir = nori_home
        .path()
        .join("transcripts/by-project/project/sessions");
    tokio::fs::create_dir_all(&session_dir)
        .await
        .expect("create transcript directory");
    tokio::fs::write(
        session_dir.join("session.jsonl"),
        concat!(
            r#"{"ts":"2025-01-01T00:00:00Z","v":2,"type":"session_meta","session_id":"session","project_id":"project","started_at":"2025-01-01T00:00:00Z","cwd":"/repo","cli_version":"0.1.0"}"#,
            "\n",
            r#"{"ts":"2025-01-01T00:00:01Z","v":2,"type":"user","id":"user-1","content":"Stored question","attachments":[]}"#,
            "\n",
            r#"{"ts":"2025-01-01T00:00:02Z","v":2,"type":"assistant","id":"assistant-1","content":[{"type":"thinking","thinking":"Stored thought"},{"type":"text","text":"Stored answer"}]}"#,
            "\n",
        ),
    )
    .await
    .expect("write v2 transcript");

    let transcript = TranscriptLoader::new(nori_home.path().to_path_buf())
        .load_transcript("project", "session")
        .await
        .expect("load v2 transcript");

    assert_eq!(
        transcript.records().collect::<Vec<_>>(),
        vec![
            TranscriptRecord::User {
                content: "Stored question",
            },
            TranscriptRecord::Thinking {
                content: "Stored thought",
            },
            TranscriptRecord::Assistant {
                content: "Stored answer",
            },
        ]
    );
}
