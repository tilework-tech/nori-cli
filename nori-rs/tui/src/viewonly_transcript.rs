//! View-only transcript display.
//!
//! This module converts transcript entries into displayable history cells
//! for the view-only transcript viewer.

use nori_harness::transcript::Transcript;
use nori_harness::transcript::TranscriptRecord;

/// A simplified entry for display in the view-only transcript viewer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ViewonlyEntry {
    /// User message
    User { content: String },
    /// Assistant message
    Assistant { content: String },
    /// Thinking/reasoning block
    Thinking { content: String },
    /// Information message (metadata, etc.)
    Info { content: String },
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RawMessageKind {
    Assistant,
    Thinking,
}

fn flush_raw_message(
    entries: &mut Vec<ViewonlyEntry>,
    pending: &mut Option<(RawMessageKind, String)>,
) {
    let Some((kind, content)) = pending.take() else {
        return;
    };
    if content.is_empty() {
        return;
    }
    entries.push(match kind {
        RawMessageKind::Assistant => ViewonlyEntry::Assistant { content },
        RawMessageKind::Thinking => ViewonlyEntry::Thinking { content },
    });
}

fn append_raw_message(
    entries: &mut Vec<ViewonlyEntry>,
    pending: &mut Option<(RawMessageKind, String)>,
    kind: RawMessageKind,
    text: &str,
) {
    if pending
        .as_ref()
        .is_some_and(|(current, _)| *current != kind)
    {
        flush_raw_message(entries, pending);
    }
    pending
        .get_or_insert_with(|| (kind, String::new()))
        .1
        .push_str(text);
}

/// Convert a loaded transcript into displayable entries.
pub fn transcript_to_entries(transcript: &Transcript) -> Vec<ViewonlyEntry> {
    let mut entries = Vec::new();
    let mut normalizer = crate::presentation::ClientEventNormalizer::default();
    let mut pending_raw_message = None;
    let mut agent_info: Option<nori_protocol::acp::v1::Implementation> = None;
    let mut replay_source = None;

    // Add session info header
    entries.push(ViewonlyEntry::Info {
        content: format!(
            "Session from {} ({})",
            format_timestamp(&transcript.meta.started_at),
            transcript
                .meta
                .session_id
                .chars()
                .take(8)
                .collect::<String>()
        ),
    });

    // Convert each public record without depending on transcript storage details.
    for record in transcript.records() {
        match record {
            TranscriptRecord::User { content } => {
                flush_raw_message(&mut entries, &mut pending_raw_message);
                entries.push(ViewonlyEntry::User {
                    content: content.to_string(),
                });
            }
            TranscriptRecord::Assistant { content } => {
                flush_raw_message(&mut entries, &mut pending_raw_message);
                entries.push(ViewonlyEntry::Assistant {
                    content: content.to_string(),
                });
            }
            TranscriptRecord::Thinking { content } => {
                flush_raw_message(&mut entries, &mut pending_raw_message);
                entries.push(ViewonlyEntry::Thinking {
                    content: content.to_string(),
                });
            }
            TranscriptRecord::SessionEvent(event) => {
                match event {
                    nori_protocol::SessionEvent::Acp(nori_protocol::AcpEvent::Response {
                        response:
                            Ok(nori_protocol::acp::v1::AgentResponse::InitializeResponse(response)),
                        ..
                    }) => {
                        agent_info = response.agent_info.clone();
                    }
                    nori_protocol::SessionEvent::Nori(nori_protocol::NoriEvent::ReplayStarted(
                        started,
                    )) => {
                        replay_source = Some(started.source);
                    }
                    nori_protocol::SessionEvent::Nori(nori_protocol::NoriEvent::ReplayFinished) => {
                        replay_source = None;
                    }
                    _ => {}
                }
                if let nori_protocol::SessionEvent::Acp(nori_protocol::AcpEvent::Notification(
                    nori_protocol::acp::v1::AgentNotification::SessionNotification(notification),
                )) = event
                {
                    let raw_message = match &notification.update {
                        nori_protocol::acp::v1::SessionUpdate::AgentMessageChunk(chunk) => {
                            Some((RawMessageKind::Assistant, &chunk.content))
                        }
                        nori_protocol::acp::v1::SessionUpdate::AgentThoughtChunk(chunk) => {
                            Some((RawMessageKind::Thinking, &chunk.content))
                        }
                        _ => None,
                    };
                    if let Some((kind, nori_protocol::acp::v1::ContentBlock::Text(text))) =
                        raw_message
                    {
                        append_raw_message(
                            &mut entries,
                            &mut pending_raw_message,
                            kind,
                            &text.text,
                        );
                        continue;
                    }
                    flush_raw_message(&mut entries, &mut pending_raw_message);
                    for event in normalizer.push_session_update(&notification.update) {
                        entries.extend(viewonly_entries_from_client_event(
                            &event,
                            agent_info.as_ref(),
                            replay_source,
                        ));
                    }
                } else {
                    flush_raw_message(&mut entries, &mut pending_raw_message);
                }
            }
        }
    }

    flush_raw_message(&mut entries, &mut pending_raw_message);

    entries
}

fn viewonly_entries_from_client_event(
    event: &crate::presentation::ClientEvent,
    agent_info: Option<&nori_protocol::acp::v1::Implementation>,
    replay_source: Option<nori_protocol::ReplaySource>,
) -> Vec<ViewonlyEntry> {
    match event {
        // Raw v3 message chunks are accumulated before normalization; v1/v2
        // transcripts use their stable assistant entries.
        crate::presentation::ClientEvent::MessageDelta(_) => vec![],
        crate::presentation::ClientEvent::ReplayEntry(replay_entry) => {
            viewonly_entries_from_replay_entry(replay_entry)
        }
        crate::presentation::ClientEvent::SessionUpdateInfo(
            crate::presentation::SessionUpdateInfo {
                session_info_patch: Some(patch),
                ..
            },
        ) => crate::nori::session_info::display(
            agent_info,
            "Agent",
            patch,
            crate::nori::session_info::SessionInfoOrigin::from_replay_source(replay_source),
            crate::nori::session_info::SessionInfoDetail::for_build(),
        )
        .map(|display| {
            vec![ViewonlyEntry::Info {
                content: display.text(),
            }]
        })
        .unwrap_or_default(),
        _ => format_client_event(event)
            .map(|content| vec![ViewonlyEntry::Info { content }])
            .unwrap_or_default(),
    }
}

fn viewonly_entries_from_replay_entry(
    replay_entry: &crate::presentation::ReplayEntry,
) -> Vec<ViewonlyEntry> {
    match replay_entry {
        crate::presentation::ReplayEntry::UserMessage { text } => vec![ViewonlyEntry::User {
            content: text.clone(),
        }],
        crate::presentation::ReplayEntry::AssistantMessage { text } => {
            vec![ViewonlyEntry::Assistant {
                content: text.clone(),
            }]
        }
        crate::presentation::ReplayEntry::ReasoningMessage { text } => {
            vec![ViewonlyEntry::Thinking {
                content: text.clone(),
            }]
        }
        crate::presentation::ReplayEntry::PlanSnapshot { snapshot } => vec![ViewonlyEntry::Info {
            content: format_client_event(&crate::presentation::ClientEvent::PlanSnapshot(
                snapshot.clone(),
            ))
            .unwrap_or_default(),
        }],
        crate::presentation::ReplayEntry::ToolSnapshot { snapshot } => vec![ViewonlyEntry::Info {
            content: format_client_event(&crate::presentation::ClientEvent::ToolSnapshot(
                snapshot.as_ref().clone(),
            ))
            .unwrap_or_default(),
        }],
    }
}

fn format_client_event(event: &crate::presentation::ClientEvent) -> Option<String> {
    match event {
        crate::presentation::ClientEvent::PlanSnapshot(plan_snapshot) => Some(
            format_tool_event("Updated Plan".to_string(), &None, &[])
                + &if plan_snapshot.entries.is_empty() {
                    String::new()
                } else {
                    format!(
                        "\n{}",
                        plan_snapshot
                            .entries
                            .iter()
                            .map(|entry| format!(
                                "- {} ({})",
                                entry.step,
                                format_plan_status(&entry.status)
                            ))
                            .collect::<Vec<_>>()
                            .join("\n")
                    )
                },
        ),
        crate::presentation::ClientEvent::ToolSnapshot(tool_snapshot) => Some(format_tool_event(
            format!(
                "Tool [{}]: {} ({})",
                format_tool_phase(&tool_snapshot.phase),
                tool_snapshot.title,
                format_tool_kind(&tool_snapshot.kind)
            ),
            &tool_snapshot.invocation,
            &tool_snapshot.artifacts,
        )),
        crate::presentation::ClientEvent::ApprovalRequest(approval) => {
            let crate::presentation::ApprovalSubject::ToolSnapshot(snapshot) = &approval.subject;
            Some(format_tool_event(
                format!(
                    "Approval requested: {} ({})",
                    approval.title,
                    format_tool_kind(&approval.kind)
                ),
                &snapshot.invocation,
                &snapshot.artifacts,
            ))
        }
        crate::presentation::ClientEvent::SessionUpdateInfo(info) => (info.kind
            != crate::presentation::SessionUpdateKind::Usage)
            .then(|| info.message.clone()),
        crate::presentation::ClientEvent::MessageDelta(_)
        | crate::presentation::ClientEvent::SessionPhaseChanged(_)
        | crate::presentation::ClientEvent::PromptCompleted(_)
        | crate::presentation::ClientEvent::LoadCompleted
        | crate::presentation::ClientEvent::QueueChanged(_)
        | crate::presentation::ClientEvent::ContextCompacted(_)
        | crate::presentation::ClientEvent::ReplayEntry(_)
        | crate::presentation::ClientEvent::AgentCommandsUpdate(_)
        | crate::presentation::ClientEvent::SessionCapabilitiesChanged(_)
        | crate::presentation::ClientEvent::SessionConfigUpdate(_)
        | crate::presentation::ClientEvent::SessionModeChanged(_)
        | crate::presentation::ClientEvent::ThreadGoalUpdated(_)
        | crate::presentation::ClientEvent::ThreadGoalCleared
        | crate::presentation::ClientEvent::Warning(_) => None,
    }
}

fn format_plan_status(status: &crate::presentation::PlanStatus) -> &'static str {
    match status {
        crate::presentation::PlanStatus::Pending => "pending",
        crate::presentation::PlanStatus::InProgress => "in_progress",
        crate::presentation::PlanStatus::Completed => "completed",
    }
}

fn format_tool_event(
    header: String,
    invocation: &Option<crate::presentation::Invocation>,
    artifacts: &[crate::presentation::Artifact],
) -> String {
    let mut lines = vec![header];

    if let Some(line) = format_invocation(invocation) {
        lines.push(line);
    }

    lines.extend(format_artifacts(artifacts));

    lines.join("\n")
}

fn format_invocation(invocation: &Option<crate::presentation::Invocation>) -> Option<String> {
    match invocation.as_ref()? {
        crate::presentation::Invocation::FileChanges { changes } => {
            Some(format!("Files changed: {}", format_change_paths(changes)))
        }
        crate::presentation::Invocation::FileOperations { operations } => Some(format!(
            "Files changed: {}",
            format_operation_paths(operations)
        )),
        crate::presentation::Invocation::Command { command } => Some(format!("Command: {command}")),
        crate::presentation::Invocation::Read { path } => Some(format!("Read: {}", path.display())),
        crate::presentation::Invocation::Search { query, path } => match (query, path) {
            (Some(query), Some(path)) => Some(format!("Search: {query} in {}", path.display())),
            (Some(query), None) => Some(format!("Search: {query}")),
            (None, Some(path)) => Some(format!("Search in {}", path.display())),
            (None, None) => None,
        },
        crate::presentation::Invocation::ListFiles { path } => path
            .as_ref()
            .map(|path| format!("List files: {}", path.display()))
            .or_else(|| Some("List files".to_string())),
        crate::presentation::Invocation::Tool { tool_name, input } => match input {
            Some(input) => Some(format!("Tool: {tool_name} {input}")),
            None => Some(format!("Tool: {tool_name}")),
        },
        crate::presentation::Invocation::RawJson(value) => Some(format!("Input: {value}")),
    }
}

fn format_change_paths(changes: &[crate::presentation::FileChange]) -> String {
    changes
        .iter()
        .map(|change| change.path.display().to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

fn format_operation_paths(operations: &[crate::presentation::FileOperation]) -> String {
    operations
        .iter()
        .map(|operation| match operation {
            crate::presentation::FileOperation::Create { path, .. }
            | crate::presentation::FileOperation::Update { path, .. }
            | crate::presentation::FileOperation::Delete { path, .. } => path.display().to_string(),
            crate::presentation::FileOperation::Move {
                from_path, to_path, ..
            } => format!("{} -> {}", from_path.display(), to_path.display()),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn format_artifacts(artifacts: &[crate::presentation::Artifact]) -> Vec<String> {
    artifacts
        .iter()
        .filter_map(|artifact| match artifact {
            crate::presentation::Artifact::Diff(_) => None,
            crate::presentation::Artifact::Text { text } if text.is_empty() => None,
            crate::presentation::Artifact::Text { text } if text.contains('\n') => {
                Some(format!("Output:\n{text}"))
            }
            crate::presentation::Artifact::Text { text } => Some(format!("Output: {text}")),
        })
        .collect()
}

fn format_tool_kind(kind: &crate::presentation::ToolKind) -> &str {
    match kind {
        crate::presentation::ToolKind::Read => "read",
        crate::presentation::ToolKind::Search => "search",
        crate::presentation::ToolKind::Execute => "execute",
        crate::presentation::ToolKind::Create => "create",
        crate::presentation::ToolKind::Edit => "edit",
        crate::presentation::ToolKind::Delete => "delete",
        crate::presentation::ToolKind::Move => "move",
        crate::presentation::ToolKind::Fetch => "fetch",
        crate::presentation::ToolKind::Think => "think",
        crate::presentation::ToolKind::Other(other) => other,
    }
}

fn format_tool_phase(phase: &crate::presentation::ToolPhase) -> &str {
    match phase {
        crate::presentation::ToolPhase::Pending => "pending",
        crate::presentation::ToolPhase::PendingApproval => "pending approval",
        crate::presentation::ToolPhase::InProgress => "in progress",
        crate::presentation::ToolPhase::Completed => "completed",
        crate::presentation::ToolPhase::Failed => "failed",
    }
}

fn format_timestamp(iso: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(iso)
        .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_else(|_| iso.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use nori_harness::transcript::TranscriptLoader;

    async fn transcript(events: Vec<nori_protocol::SessionEvent>) -> Transcript {
        let nori_home = tempfile::tempdir().expect("create Nori home");
        let session_dir = nori_home
            .path()
            .join("transcripts/by-project/project/sessions");
        tokio::fs::create_dir_all(&session_dir)
            .await
            .expect("create transcript directory");

        let mut lines = vec![serde_json::json!({
            "ts": "2025-01-27T12:00:00Z",
            "v": 3,
            "type": "session_meta",
            "session_id": "test-session",
            "project_id": "project",
            "started_at": "2025-01-27T12:00:00Z",
            "cwd": "/repo",
            "agent": "mock",
            "cli_version": "test",
            "acp_session_id": "acp-session"
        })];
        lines.extend(events.into_iter().map(|event| {
            serde_json::json!({
                "ts": "2025-01-27T12:00:01Z",
                "v": 3,
                "type": "session_event",
                "event": event
            })
        }));
        let body = lines
            .into_iter()
            .map(|line| serde_json::to_string(&line).expect("serialize transcript line"))
            .collect::<Vec<_>>()
            .join("\n");
        tokio::fs::write(session_dir.join("test-session.jsonl"), body)
            .await
            .expect("write transcript");

        TranscriptLoader::new(nori_home.path().to_path_buf())
            .load_transcript("project", "test-session")
            .await
            .expect("load transcript")
    }

    #[tokio::test]
    async fn renders_consecutive_raw_v3_assistant_chunks_as_one_message() {
        let raw_chunk = |text: &str| {
            nori_protocol::SessionEvent::Acp(nori_protocol::AcpEvent::Notification(
                nori_protocol::acp::v1::AgentNotification::SessionNotification(
                    nori_protocol::acp::v1::SessionNotification::new(
                        "acp-session",
                        nori_protocol::acp::v1::SessionUpdate::AgentMessageChunk(
                            nori_protocol::acp::v1::ContentChunk::new(
                                nori_protocol::acp::v1::ContentBlock::Text(
                                    nori_protocol::acp::v1::TextContent::new(text),
                                ),
                            ),
                        ),
                    ),
                ),
            ))
        };
        let transcript = transcript(vec![raw_chunk("hello "), raw_chunk("world")]).await;

        let entries = transcript_to_entries(&transcript);
        assert_eq!(
            entries
                .iter()
                .filter_map(|entry| match entry {
                    ViewonlyEntry::Assistant { content } => Some(content.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>(),
            vec!["hello world"]
        );
    }

    #[tokio::test]
    async fn renders_tool_updates_from_the_raw_v3_boundary() {
        let update = nori_protocol::acp::v1::SessionUpdate::ToolCall(
            nori_protocol::acp::v1::ToolCall::new(
                nori_protocol::acp::v1::ToolCallId::new("read-1"),
                "Read README.md",
            )
            .kind(nori_protocol::acp::v1::ToolKind::Read)
            .status(nori_protocol::acp::v1::ToolCallStatus::Completed)
            .locations(vec![nori_protocol::acp::v1::ToolCallLocation::new(
                "/repo/README.md",
            )]),
        );
        let event = nori_protocol::SessionEvent::Acp(nori_protocol::AcpEvent::Notification(
            nori_protocol::acp::v1::AgentNotification::SessionNotification(
                nori_protocol::acp::v1::SessionNotification::new("acp-session", update),
            ),
        ));
        let transcript = transcript(vec![event]).await;

        let entries = transcript_to_entries(&transcript);
        assert!(entries.iter().any(|entry| {
            matches!(entry, ViewonlyEntry::Info { content } if content.contains("Read README.md"))
        }));
    }

    #[tokio::test]
    async fn renders_structured_session_info_from_the_raw_v3_boundary() {
        let initialize = nori_protocol::SessionEvent::Acp(nori_protocol::AcpEvent::Response {
            request_id: nori_protocol::acp::v1::RequestId::Str("initialize".to_string()),
            response: Ok(nori_protocol::acp::v1::AgentResponse::InitializeResponse(
                nori_protocol::acp::v1::InitializeResponse::new(
                    nori_protocol::acp::ProtocolVersion::LATEST,
                )
                .agent_info(
                    nori_protocol::acp::v1::Implementation::new("codex-acp", "1.1.4")
                        .title("Codex ACP"),
                ),
            )),
        });
        let meta = serde_json::json!({
            "codex": {
                "threadStatus": {
                    "type": "active",
                    "activeFlags": ["waitingOnUserInput"]
                },
                "newDiagnostic": "private-value"
            },
            "other": {
                "counter": 9
            }
        })
        .as_object()
        .expect("metadata object")
        .clone();
        let update = nori_protocol::SessionEvent::Acp(nori_protocol::AcpEvent::Notification(
            nori_protocol::acp::v1::AgentNotification::SessionNotification(
                nori_protocol::acp::v1::SessionNotification::new(
                    "acp-session",
                    nori_protocol::acp::v1::SessionUpdate::SessionInfoUpdate(
                        nori_protocol::acp::v1::SessionInfoUpdate::new().meta(meta),
                    ),
                ),
            ),
        ));
        let transcript = transcript(vec![initialize, update]).await;

        let entries = transcript_to_entries(&transcript);
        let info = entries
            .iter()
            .filter_map(|entry| match entry {
                ViewonlyEntry::Info { content } => Some(content.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(info.contains("Codex ACP 1.1.4 session updated"), "{info}");
        assert!(info.contains("status=active"), "{info}");
        assert!(info.contains("waiting=user_input"), "{info}");
        assert!(info.contains("codex.newDiagnostic=<string>"), "{info}");
        assert!(info.contains("other.counter=<number>"), "{info}");
        assert!(!info.contains("private-value"), "{info}");
    }
}
