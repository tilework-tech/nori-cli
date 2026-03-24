use super::*;
use codex_protocol::protocol::FileChange;
use pretty_assertions::assert_eq;

/// Test that when a ToolCall is skipped (generic title, no useful raw_input)
/// and then a ToolCallUpdate arrives with a good title followed by a completion
/// ToolCallUpdate with no title, the accumulated title is used.
///
/// This simulates the real claude-agent-acp flow:
/// 1. tool_call: title="Read File", raw_input={}  (skipped)
/// 2. tool_call_update: title="Read /path/to/file.rs", kind=Read, raw_input={path: ...} (accumulated)
/// 3. tool_call_update: status=Completed, no title (should use accumulated title)
#[test]
fn test_accumulated_title_used_on_completion() {
    let mut pending_patches = std::collections::HashMap::new();
    let mut pending_tool_calls = std::collections::HashMap::new();

    // Step 1: Generic ToolCall - should be skipped but data stored
    let tool_call = acp::SessionUpdate::ToolCall(
        acp::ToolCall::new(
            acp::ToolCallId::from("toolu_abc123".to_string()),
            "Read File",
        )
        .kind(acp::ToolKind::Read)
        .status(acp::ToolCallStatus::Pending),
    );
    let events = translate_session_update_to_events(
        &tool_call,
        &mut pending_patches,
        &mut pending_tool_calls,
    );
    assert!(events.is_empty(), "Generic ToolCall should be skipped");

    // Step 2: Intermediate update with good title (not completed)
    let update_with_title = acp::SessionUpdate::ToolCallUpdate(acp::ToolCallUpdate::new(
        acp::ToolCallId::from("toolu_abc123".to_string()),
        acp::ToolCallUpdateFields::new()
            .title("Read /home/user/src/main.rs")
            .kind(acp::ToolKind::Read)
            .raw_input(serde_json::json!({"path": "/home/user/src/main.rs"})),
    ));
    let events = translate_session_update_to_events(
        &update_with_title,
        &mut pending_patches,
        &mut pending_tool_calls,
    );
    assert!(
        events.is_empty(),
        "Non-completed update should not emit events"
    );

    // Step 3: Completion update with no title - should use accumulated data
    let completion = acp::SessionUpdate::ToolCallUpdate(acp::ToolCallUpdate::new(
        acp::ToolCallId::from("toolu_abc123".to_string()),
        acp::ToolCallUpdateFields::new().status(acp::ToolCallStatus::Completed),
    ));
    let events = translate_session_update_to_events(
        &completion,
        &mut pending_patches,
        &mut pending_tool_calls,
    );
    assert_eq!(events.len(), 1, "Completion should emit exactly one event");

    match &events[0] {
        EventMsg::ExecCommandEnd(end) => {
            assert_eq!(
                end.command[0], "Read /home/user/src/main.rs",
                "Should use accumulated title from intermediate update"
            );
        }
        _ => panic!("Expected ExecCommandEnd event, got: {:?}", events[0]),
    }
}

/// Test that _meta.claudeCode.toolName is used as a fallback when title is missing.
#[test]
fn test_meta_tool_name_used_as_fallback() {
    let mut pending_patches = std::collections::HashMap::new();
    let mut pending_tool_calls = std::collections::HashMap::new();

    // ToolCallUpdate with status=Completed, no title, but _meta.claudeCode.toolName = "Bash"
    let mut meta = serde_json::Map::new();
    meta.insert(
        "claudeCode".to_string(),
        serde_json::json!({"toolName": "Bash"}),
    );

    let mut update = acp::ToolCallUpdate::new(
        acp::ToolCallId::from("toolu_xyz789".to_string()),
        acp::ToolCallUpdateFields::new().status(acp::ToolCallStatus::Completed),
    );
    update = update.meta(meta);

    let events = translate_session_update_to_events(
        &acp::SessionUpdate::ToolCallUpdate(update),
        &mut pending_patches,
        &mut pending_tool_calls,
    );
    assert_eq!(events.len(), 1);

    match &events[0] {
        EventMsg::ExecCommandEnd(end) => {
            assert_eq!(
                end.command[0], "Bash",
                "Should use meta toolName 'Bash' as fallback"
            );
        }
        _ => panic!("Expected ExecCommandEnd event"),
    }
}

/// Test that when a ToolCallUpdate arrives with status=Completed and a good title,
/// no accumulation is needed - the title from the update itself is used directly.
#[test]
fn test_direct_completion_with_title() {
    let mut pending_patches = std::collections::HashMap::new();
    let mut pending_tool_calls = std::collections::HashMap::new();

    let update = acp::SessionUpdate::ToolCallUpdate(acp::ToolCallUpdate::new(
        acp::ToolCallId::from("call-direct".to_string()),
        acp::ToolCallUpdateFields::new()
            .status(acp::ToolCallStatus::Completed)
            .title("Terminal(git status)")
            .kind(acp::ToolKind::Execute),
    ));

    let events =
        translate_session_update_to_events(&update, &mut pending_patches, &mut pending_tool_calls);
    assert_eq!(events.len(), 1);

    match &events[0] {
        EventMsg::ExecCommandEnd(end) => {
            assert_eq!(
                end.command[0], "Terminal(git status)",
                "Should use the update's own title directly"
            );
        }
        _ => panic!("Expected ExecCommandEnd event"),
    }
}

/// Test that when only kind is available (no title from any source), the
/// kind-based display name is used as fallback.
#[test]
fn test_kind_based_fallback_when_no_title() {
    let mut pending_patches = std::collections::HashMap::new();
    let mut pending_tool_calls = std::collections::HashMap::new();

    // Directly set up accumulated state with kind but no usable title
    pending_tool_calls.insert(
        "call-kind-only".to_string(),
        AccumulatedToolCall {
            title: None,
            kind: Some(acp::ToolKind::Read),
            raw_input: None,
            meta_tool_name: None,
        },
    );

    // Completion with no title, no meta — should fall back to kind display name
    let completion = acp::SessionUpdate::ToolCallUpdate(acp::ToolCallUpdate::new(
        acp::ToolCallId::from("call-kind-only".to_string()),
        acp::ToolCallUpdateFields::new().status(acp::ToolCallStatus::Completed),
    ));
    let events = translate_session_update_to_events(
        &completion,
        &mut pending_patches,
        &mut pending_tool_calls,
    );
    assert_eq!(events.len(), 1);

    match &events[0] {
        EventMsg::ExecCommandEnd(end) => {
            assert_eq!(
                end.command[0], "Read",
                "Should use kind_to_display_name fallback"
            );
        }
        _ => panic!("Expected ExecCommandEnd event"),
    }
}

#[test]
fn test_completion_with_patch_raw_input_emits_patch_apply_begin() {
    let mut pending_patches = std::collections::HashMap::new();
    let mut pending_tool_calls = std::collections::HashMap::new();

    let update = acp::SessionUpdate::ToolCallUpdate(acp::ToolCallUpdate::new(
        acp::ToolCallId::from("call-edit-complete".to_string()),
        acp::ToolCallUpdateFields::new()
            .status(acp::ToolCallStatus::Completed)
            .title("Edit")
            .kind(acp::ToolKind::Edit)
            .raw_input(serde_json::json!({
                "file_path": "/repo/src/main.rs",
                "old_string": "fn old() {}\n",
                "new_string": "fn new() {}\n",
            })),
    ));

    let events =
        translate_session_update_to_events(&update, &mut pending_patches, &mut pending_tool_calls);
    assert_eq!(events.len(), 1, "Completion should emit exactly one event");

    match &events[0] {
        EventMsg::PatchApplyBegin(begin) => {
            assert_eq!(begin.call_id, "call-edit-complete");
            assert_eq!(begin.changes.len(), 1);
            match begin.changes.get(&PathBuf::from("/repo/src/main.rs")) {
                Some(FileChange::Update { .. }) => {}
                other => panic!("Expected FileChange::Update, got {other:?}"),
            }
        }
        other => panic!("Expected PatchApplyBegin event, got {other:?}"),
    }
}

#[test]
fn test_skipped_generic_patch_call_can_still_emit_patch_apply_begin_on_completion() {
    let mut pending_patches = std::collections::HashMap::new();
    let mut pending_tool_calls = std::collections::HashMap::new();

    let tool_call = acp::SessionUpdate::ToolCall(
        acp::ToolCall::new(acp::ToolCallId::from("call-edit-late".to_string()), "Edit")
            .kind(acp::ToolKind::Edit)
            .status(acp::ToolCallStatus::Pending),
    );
    let events = translate_session_update_to_events(
        &tool_call,
        &mut pending_patches,
        &mut pending_tool_calls,
    );
    assert!(events.is_empty(), "Generic edit ToolCall should be skipped");

    let detailed_update = acp::SessionUpdate::ToolCallUpdate(acp::ToolCallUpdate::new(
        acp::ToolCallId::from("call-edit-late".to_string()),
        acp::ToolCallUpdateFields::new()
            .kind(acp::ToolKind::Edit)
            .raw_input(serde_json::json!({
                "file_path": "/repo/tests/example.rs",
                "old_string": "before\n",
                "new_string": "after\n",
            })),
    ));
    let events = translate_session_update_to_events(
        &detailed_update,
        &mut pending_patches,
        &mut pending_tool_calls,
    );
    assert!(
        events.is_empty(),
        "Intermediate update should only accumulate state"
    );

    let completion = acp::SessionUpdate::ToolCallUpdate(acp::ToolCallUpdate::new(
        acp::ToolCallId::from("call-edit-late".to_string()),
        acp::ToolCallUpdateFields::new().status(acp::ToolCallStatus::Completed),
    ));
    let events = translate_session_update_to_events(
        &completion,
        &mut pending_patches,
        &mut pending_tool_calls,
    );
    assert_eq!(events.len(), 1, "Completion should emit exactly one event");

    match &events[0] {
        EventMsg::PatchApplyBegin(begin) => {
            assert_eq!(begin.call_id, "call-edit-late");
            assert!(
                begin
                    .changes
                    .contains_key(&PathBuf::from("/repo/tests/example.rs"))
            );
        }
        other => panic!("Expected PatchApplyBegin event, got {other:?}"),
    }
}

#[tokio::test]
async fn test_transcript_recording_uses_completion_fallback_patch_metadata() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let recorder = crate::transcript::TranscriptRecorder::new(
        temp_dir.path(),
        temp_dir.path(),
        None,
        "0.1.0",
        None,
    )
    .await
    .unwrap();

    let update = acp::SessionUpdate::ToolCallUpdate(acp::ToolCallUpdate::new(
        acp::ToolCallId::from("call-record-fallback".to_string()),
        acp::ToolCallUpdateFields::new().status(acp::ToolCallStatus::Completed),
    ));
    let mut recorded_call_ids = std::collections::HashSet::new();
    let pending_patch_changes = std::collections::HashMap::new();
    let pending_tool_calls = std::collections::HashMap::from([(
        "call-record-fallback".to_string(),
        AccumulatedToolCall {
            title: Some("Edit".to_string()),
            kind: Some(acp::ToolKind::Edit),
            raw_input: Some(serde_json::json!({
                "file_path": "/repo/src/lib.rs",
                "old_string": "before\n",
                "new_string": "after\n",
            })),
            meta_tool_name: None,
        },
    )]);

    record_tool_events_to_transcript(
        &update,
        &recorder,
        &mut recorded_call_ids,
        &pending_patch_changes,
        &pending_tool_calls,
    )
    .await;
    recorder.flush().await.unwrap();
    recorder.shutdown().await.unwrap();

    let content = tokio::fs::read_to_string(recorder.transcript_path())
        .await
        .unwrap();
    let lines: Vec<&str> = content.lines().collect();
    assert_eq!(lines.len(), 2, "SessionMeta + PatchApply");

    let line: crate::transcript::TranscriptLine = serde_json::from_str(lines[1]).unwrap();
    match line.entry {
        crate::transcript::TranscriptEntry::PatchApply(patch) => {
            assert_eq!(patch.call_id, "call-record-fallback");
            assert_eq!(patch.operation, crate::transcript::PatchOperationType::Edit);
            assert_eq!(patch.path, PathBuf::from("/repo/src/lib.rs"));
            assert!(patch.success);
            assert_eq!(patch.error, None);
        }
        other => panic!("Expected PatchApply entry, got {other:?}"),
    }
}

/// Test the title_is_raw_id detection function.
#[test]
fn test_title_is_raw_id_detection() {
    // Should be detected as raw IDs
    assert!(
        title_is_raw_id("toolu_015Xtg1GzAd6aPH6oiirx5us"),
        "Should detect standard Anthropic tool_use ID"
    );
    assert!(
        title_is_raw_id("toolu_01BoW1485VX7AF2DFwiTbunD"),
        "Should detect another standard tool_use ID"
    );
    assert!(
        title_is_raw_id("toolu_abc123def456"),
        "Should detect shorter tool_use ID"
    );

    // Should NOT be detected as raw IDs
    assert!(
        !title_is_raw_id("Read /home/user/file.rs"),
        "Human-readable title should not be detected as raw ID"
    );
    assert!(
        !title_is_raw_id("Terminal"),
        "Simple tool name should not be detected as raw ID"
    );
    assert!(
        !title_is_raw_id("Read File"),
        "Generic title should not be detected as raw ID"
    );
    assert!(
        !title_is_raw_id(""),
        "Empty string should not be detected as raw ID"
    );
    assert!(
        !title_is_raw_id("toolu_"),
        "Just prefix should not be detected (too short)"
    );
}
