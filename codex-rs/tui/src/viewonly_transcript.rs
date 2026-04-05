//! View-only transcript display.
//!
//! This module converts transcript entries into displayable history cells
//! for the view-only transcript viewer.

use codex_acp::transcript::ContentBlock;
use codex_acp::transcript::Transcript;
use codex_acp::transcript::TranscriptEntry;

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

/// Convert a loaded transcript into displayable entries.
pub fn transcript_to_entries(transcript: &Transcript) -> Vec<ViewonlyEntry> {
    let mut entries = Vec::new();

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

    // Convert each entry
    for line in &transcript.entries {
        match &line.entry {
            TranscriptEntry::SessionMeta(_) => {
                // Skip - already shown in header
            }
            TranscriptEntry::User(user) => {
                entries.push(ViewonlyEntry::User {
                    content: user.content.clone(),
                });
            }
            TranscriptEntry::Assistant(assistant) => {
                // Process each content block separately to handle thinking blocks
                for block in &assistant.content {
                    match block {
                        ContentBlock::Thinking { thinking } => {
                            entries.push(ViewonlyEntry::Thinking {
                                content: thinking.clone(),
                            });
                        }
                        ContentBlock::Text { text } => {
                            entries.push(ViewonlyEntry::Assistant {
                                content: text.clone(),
                            });
                        }
                    }
                }
            }
            TranscriptEntry::ClientEvent(client_event) => {
                entries.extend(viewonly_entries_from_client_event(&client_event.event));
            }
            // Skip legacy tool calls, tool results, and patch operations.
            // Normalized client_event entries are the preferred transcript form.
            TranscriptEntry::ToolCall(_)
            | TranscriptEntry::ToolResult(_)
            | TranscriptEntry::PatchApply(_) => {}
        }
    }

    entries
}

fn viewonly_entries_from_client_event(event: &nori_protocol::ClientEvent) -> Vec<ViewonlyEntry> {
    match event {
        // Transcript files already store finalized assistant/thinking blocks.
        // Rendering live deltas here duplicates content in view-only history.
        nori_protocol::ClientEvent::MessageDelta(_) => vec![],
        nori_protocol::ClientEvent::ReplayEntry(replay_entry) => {
            viewonly_entries_from_replay_entry(replay_entry)
        }
        _ => format_client_event(event)
            .map(|content| vec![ViewonlyEntry::Info { content }])
            .unwrap_or_default(),
    }
}

fn viewonly_entries_from_replay_entry(
    replay_entry: &nori_protocol::ReplayEntry,
) -> Vec<ViewonlyEntry> {
    match replay_entry {
        nori_protocol::ReplayEntry::UserMessage { text } => vec![ViewonlyEntry::User {
            content: text.clone(),
        }],
        nori_protocol::ReplayEntry::AssistantMessage { text } => vec![ViewonlyEntry::Assistant {
            content: text.clone(),
        }],
        nori_protocol::ReplayEntry::ReasoningMessage { text } => vec![ViewonlyEntry::Thinking {
            content: text.clone(),
        }],
        nori_protocol::ReplayEntry::PlanSnapshot { snapshot } => vec![ViewonlyEntry::Info {
            content: format_client_event(&nori_protocol::ClientEvent::PlanSnapshot(
                snapshot.clone(),
            ))
            .unwrap_or_default(),
        }],
        nori_protocol::ReplayEntry::ToolSnapshot { snapshot } => vec![ViewonlyEntry::Info {
            content: format_client_event(&nori_protocol::ClientEvent::ToolSnapshot(
                snapshot.as_ref().clone(),
            ))
            .unwrap_or_default(),
        }],
    }
}

fn format_client_event(event: &nori_protocol::ClientEvent) -> Option<String> {
    match event {
        nori_protocol::ClientEvent::PlanSnapshot(plan_snapshot) => Some(
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
        nori_protocol::ClientEvent::ToolSnapshot(tool_snapshot) => Some(format_tool_event(
            format!(
                "Tool [{}]: {} ({})",
                format_tool_phase(&tool_snapshot.phase),
                tool_snapshot.title,
                format_tool_kind(&tool_snapshot.kind)
            ),
            &tool_snapshot.invocation,
            &tool_snapshot.artifacts,
        )),
        nori_protocol::ClientEvent::ApprovalRequest(approval) => {
            let nori_protocol::ApprovalSubject::ToolSnapshot(snapshot) = &approval.subject;
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
        nori_protocol::ClientEvent::MessageDelta(_)
        | nori_protocol::ClientEvent::ReplayEntry(_)
        | nori_protocol::ClientEvent::TurnLifecycle(_)
        | nori_protocol::ClientEvent::AgentCommandsUpdate(_) => None,
    }
}

fn format_plan_status(status: &nori_protocol::PlanStatus) -> &'static str {
    match status {
        nori_protocol::PlanStatus::Pending => "pending",
        nori_protocol::PlanStatus::InProgress => "in_progress",
        nori_protocol::PlanStatus::Completed => "completed",
    }
}

fn format_tool_event(
    header: String,
    invocation: &Option<nori_protocol::Invocation>,
    artifacts: &[nori_protocol::Artifact],
) -> String {
    let mut lines = vec![header];

    if let Some(line) = format_invocation(invocation) {
        lines.push(line);
    }

    lines.extend(format_artifacts(artifacts));

    lines.join("\n")
}

fn format_invocation(invocation: &Option<nori_protocol::Invocation>) -> Option<String> {
    match invocation.as_ref()? {
        nori_protocol::Invocation::FileChanges { changes } => {
            Some(format!("Files changed: {}", format_change_paths(changes)))
        }
        nori_protocol::Invocation::FileOperations { operations } => Some(format!(
            "Files changed: {}",
            format_operation_paths(operations)
        )),
        nori_protocol::Invocation::Command { command } => Some(format!("Command: {command}")),
        nori_protocol::Invocation::Read { path } => Some(format!("Read: {}", path.display())),
        nori_protocol::Invocation::Search { query, path } => match (query, path) {
            (Some(query), Some(path)) => Some(format!("Search: {query} in {}", path.display())),
            (Some(query), None) => Some(format!("Search: {query}")),
            (None, Some(path)) => Some(format!("Search in {}", path.display())),
            (None, None) => None,
        },
        nori_protocol::Invocation::ListFiles { path } => path
            .as_ref()
            .map(|path| format!("List files: {}", path.display()))
            .or_else(|| Some("List files".to_string())),
        nori_protocol::Invocation::Tool { tool_name, input } => match input {
            Some(input) => Some(format!("Tool: {tool_name} {input}")),
            None => Some(format!("Tool: {tool_name}")),
        },
        nori_protocol::Invocation::RawJson(value) => Some(format!("Input: {value}")),
    }
}

fn format_change_paths(changes: &[nori_protocol::FileChange]) -> String {
    changes
        .iter()
        .map(|change| change.path.display().to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

fn format_operation_paths(operations: &[nori_protocol::FileOperation]) -> String {
    operations
        .iter()
        .map(|operation| match operation {
            nori_protocol::FileOperation::Create { path, .. }
            | nori_protocol::FileOperation::Update { path, .. }
            | nori_protocol::FileOperation::Delete { path, .. } => path.display().to_string(),
            nori_protocol::FileOperation::Move {
                from_path, to_path, ..
            } => format!("{} -> {}", from_path.display(), to_path.display()),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn format_artifacts(artifacts: &[nori_protocol::Artifact]) -> Vec<String> {
    artifacts
        .iter()
        .filter_map(|artifact| match artifact {
            nori_protocol::Artifact::Diff(_) => None,
            nori_protocol::Artifact::Text { text } if text.is_empty() => None,
            nori_protocol::Artifact::Text { text } if text.contains('\n') => {
                Some(format!("Output:\n{text}"))
            }
            nori_protocol::Artifact::Text { text } => Some(format!("Output: {text}")),
        })
        .collect()
}

fn format_tool_kind(kind: &nori_protocol::ToolKind) -> &str {
    match kind {
        nori_protocol::ToolKind::Read => "read",
        nori_protocol::ToolKind::Search => "search",
        nori_protocol::ToolKind::Execute => "execute",
        nori_protocol::ToolKind::Create => "create",
        nori_protocol::ToolKind::Edit => "edit",
        nori_protocol::ToolKind::Delete => "delete",
        nori_protocol::ToolKind::Move => "move",
        nori_protocol::ToolKind::Fetch => "fetch",
        nori_protocol::ToolKind::Think => "think",
        nori_protocol::ToolKind::Other(other) => other,
    }
}

fn format_tool_phase(phase: &nori_protocol::ToolPhase) -> &str {
    match phase {
        nori_protocol::ToolPhase::Pending => "pending",
        nori_protocol::ToolPhase::PendingApproval => "pending approval",
        nori_protocol::ToolPhase::InProgress => "in progress",
        nori_protocol::ToolPhase::Completed => "completed",
        nori_protocol::ToolPhase::Failed => "failed",
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
    use codex_acp::transcript::AssistantEntry;
    use codex_acp::transcript::ClientEventEntry;
    use codex_acp::transcript::PatchApplyEntry;
    use codex_acp::transcript::PatchOperationType;
    use codex_acp::transcript::SessionMetaEntry;
    use codex_acp::transcript::ToolCallEntry;
    use codex_acp::transcript::ToolResultEntry;
    use codex_acp::transcript::Transcript;
    use codex_acp::transcript::TranscriptLine;
    use codex_acp::transcript::UserEntry;
    use pretty_assertions::assert_eq;
    use std::path::PathBuf;

    fn make_session_meta() -> SessionMetaEntry {
        SessionMetaEntry {
            session_id: "test-session-123".to_string(),
            project_id: "test-project".to_string(),
            started_at: "2025-01-27T12:00:00.000Z".to_string(),
            cwd: PathBuf::from("/tmp/test"),
            agent: Some("claude".to_string()),
            cli_version: "0.1.0".to_string(),
            git: None,
            acp_session_id: None,
        }
    }

    fn make_transcript(entries: Vec<TranscriptEntry>) -> Transcript {
        let meta = make_session_meta();
        let mut lines = vec![TranscriptLine {
            ts: "2025-01-27T12:00:00.000Z".to_string(),
            v: 1,
            entry: TranscriptEntry::SessionMeta(meta.clone()),
        }];
        for entry in entries {
            lines.push(TranscriptLine {
                ts: "2025-01-27T12:00:01.000Z".to_string(),
                v: 1,
                entry,
            });
        }
        Transcript {
            meta,
            entries: lines,
        }
    }

    #[test]
    fn test_transcript_to_entries_skips_tool_calls() {
        let transcript = make_transcript(vec![
            TranscriptEntry::User(UserEntry {
                id: "msg-001".to_string(),
                content: "Hello".to_string(),
                attachments: vec![],
            }),
            TranscriptEntry::ToolCall(ToolCallEntry {
                call_id: "call-001".to_string(),
                name: "shell".to_string(),
                input: serde_json::json!({"command": "ls"}),
            }),
            TranscriptEntry::Assistant(AssistantEntry {
                id: "msg-002".to_string(),
                content: vec![ContentBlock::Text {
                    text: "Here are the files".to_string(),
                }],
                agent: None,
            }),
        ]);

        let entries = transcript_to_entries(&transcript);

        // Should have: session info, user message, assistant message
        // Should NOT have: tool call info
        assert_eq!(entries.len(), 3);
        assert!(matches!(&entries[0], ViewonlyEntry::Info { .. })); // session header
        assert!(matches!(&entries[1], ViewonlyEntry::User { content } if content == "Hello"));
        assert!(
            matches!(&entries[2], ViewonlyEntry::Assistant { content } if content == "Here are the files")
        );
    }

    #[test]
    fn test_transcript_to_entries_skips_tool_results() {
        let transcript = make_transcript(vec![
            TranscriptEntry::User(UserEntry {
                id: "msg-001".to_string(),
                content: "Run a command".to_string(),
                attachments: vec![],
            }),
            TranscriptEntry::ToolResult(ToolResultEntry {
                call_id: "call-001".to_string(),
                output: "file1.txt\nfile2.txt".to_string(),
                truncated: false,
                exit_code: Some(0),
            }),
            TranscriptEntry::Assistant(AssistantEntry {
                id: "msg-002".to_string(),
                content: vec![ContentBlock::Text {
                    text: "Command executed".to_string(),
                }],
                agent: None,
            }),
        ]);

        let entries = transcript_to_entries(&transcript);

        // Should have: session info, user message, assistant message
        // Should NOT have: tool result info
        assert_eq!(entries.len(), 3);
        assert!(matches!(&entries[0], ViewonlyEntry::Info { .. }));
        assert!(matches!(&entries[1], ViewonlyEntry::User { .. }));
        assert!(matches!(&entries[2], ViewonlyEntry::Assistant { .. }));
    }

    #[test]
    fn test_transcript_to_entries_skips_patch_operations() {
        let transcript = make_transcript(vec![
            TranscriptEntry::User(UserEntry {
                id: "msg-001".to_string(),
                content: "Edit the file".to_string(),
                attachments: vec![],
            }),
            TranscriptEntry::PatchApply(PatchApplyEntry {
                call_id: "call-001".to_string(),
                operation: PatchOperationType::Edit,
                path: PathBuf::from("/src/main.rs"),
                success: true,
                error: None,
            }),
            TranscriptEntry::Assistant(AssistantEntry {
                id: "msg-002".to_string(),
                content: vec![ContentBlock::Text {
                    text: "File edited".to_string(),
                }],
                agent: None,
            }),
        ]);

        let entries = transcript_to_entries(&transcript);

        // Should have: session info, user message, assistant message
        // Should NOT have: patch info
        assert_eq!(entries.len(), 3);
        assert!(matches!(&entries[0], ViewonlyEntry::Info { .. }));
        assert!(matches!(&entries[1], ViewonlyEntry::User { .. }));
        assert!(matches!(&entries[2], ViewonlyEntry::Assistant { .. }));
    }

    #[test]
    fn test_transcript_to_entries_displays_thinking_blocks() {
        let transcript = make_transcript(vec![
            TranscriptEntry::User(UserEntry {
                id: "msg-001".to_string(),
                content: "Think about this".to_string(),
                attachments: vec![],
            }),
            TranscriptEntry::Assistant(AssistantEntry {
                id: "msg-002".to_string(),
                content: vec![
                    ContentBlock::Thinking {
                        thinking: "Let me think about this carefully...".to_string(),
                    },
                    ContentBlock::Text {
                        text: "Here is my response".to_string(),
                    },
                ],
                agent: None,
            }),
        ]);

        let entries = transcript_to_entries(&transcript);

        // Should have: session info, user message, thinking block, assistant message
        assert_eq!(entries.len(), 4);
        assert!(matches!(&entries[0], ViewonlyEntry::Info { .. }));
        assert!(
            matches!(&entries[1], ViewonlyEntry::User { content } if content == "Think about this")
        );
        assert!(
            matches!(&entries[2], ViewonlyEntry::Thinking { content } if content == "Let me think about this carefully...")
        );
        assert!(
            matches!(&entries[3], ViewonlyEntry::Assistant { content } if content == "Here is my response")
        );
    }

    #[test]
    fn test_transcript_to_entries_skips_normalized_message_deltas() {
        let transcript = make_transcript(vec![
            TranscriptEntry::User(UserEntry {
                id: "msg-001".to_string(),
                content: "Hello".to_string(),
                attachments: vec![],
            }),
            TranscriptEntry::ClientEvent(ClientEventEntry {
                event: nori_protocol::ClientEvent::MessageDelta(nori_protocol::MessageDelta {
                    stream: nori_protocol::MessageStream::Answer,
                    delta: "partial answer".to_string(),
                }),
            }),
            TranscriptEntry::Assistant(AssistantEntry {
                id: "msg-002".to_string(),
                content: vec![ContentBlock::Text {
                    text: "final answer".to_string(),
                }],
                agent: None,
            }),
        ]);

        let entries = transcript_to_entries(&transcript);

        assert_eq!(
            entries,
            vec![
                ViewonlyEntry::Info {
                    content: "Session from 2025-01-27 12:00 (test-ses)".to_string(),
                },
                ViewonlyEntry::User {
                    content: "Hello".to_string(),
                },
                ViewonlyEntry::Assistant {
                    content: "final answer".to_string(),
                },
            ]
        );
    }

    #[test]
    fn test_transcript_to_entries_keeps_session_header() {
        let transcript = make_transcript(vec![]);

        let entries = transcript_to_entries(&transcript);

        // Should have session header even with no messages
        assert_eq!(entries.len(), 1);
        assert!(
            matches!(&entries[0], ViewonlyEntry::Info { content } if content.contains("Session from"))
        );
    }

    #[test]
    fn test_transcript_to_entries_renders_normalized_tool_snapshot() {
        let transcript = make_transcript(vec![TranscriptEntry::ClientEvent(ClientEventEntry {
            event: nori_protocol::ClientEvent::ToolSnapshot(nori_protocol::ToolSnapshot {
                call_id: "call-001".to_string(),
                title: "Edit /tmp/main.rs".to_string(),
                kind: nori_protocol::ToolKind::Edit,
                phase: nori_protocol::ToolPhase::Completed,
                locations: vec![],
                invocation: None,
                artifacts: vec![],
                raw_input: None,
                raw_output: None,
            }),
        })]);

        let entries = transcript_to_entries(&transcript);

        assert_eq!(entries.len(), 2);
        assert!(matches!(&entries[0], ViewonlyEntry::Info { .. }));
        assert!(
            matches!(&entries[1], ViewonlyEntry::Info { content } if content == "Tool [completed]: Edit /tmp/main.rs (edit)")
        );
    }

    #[test]
    fn test_transcript_to_entries_renders_command_snapshot_details() {
        let transcript = make_transcript(vec![TranscriptEntry::ClientEvent(ClientEventEntry {
            event: nori_protocol::ClientEvent::ToolSnapshot(nori_protocol::ToolSnapshot {
                call_id: "call-exec-001".to_string(),
                title: "Run date".to_string(),
                kind: nori_protocol::ToolKind::Execute,
                phase: nori_protocol::ToolPhase::Completed,
                locations: vec![],
                invocation: Some(nori_protocol::Invocation::Command {
                    command: "date".to_string(),
                }),
                artifacts: vec![nori_protocol::Artifact::Text {
                    text: "Thu Mar 27 12:00:00 UTC 2025".to_string(),
                }],
                raw_input: None,
                raw_output: None,
            }),
        })]);

        let entries = transcript_to_entries(&transcript);

        assert_eq!(
            entries,
            vec![
                ViewonlyEntry::Info {
                    content: "Session from 2025-01-27 12:00 (test-ses)".to_string(),
                },
                ViewonlyEntry::Info {
                    content: "Tool [completed]: Run date (execute)\nCommand: date\nOutput: Thu Mar 27 12:00:00 UTC 2025".to_string(),
                },
            ]
        );
    }

    #[test]
    fn test_transcript_to_entries_renders_structured_read_search_and_list_details() {
        let transcript = make_transcript(vec![
            TranscriptEntry::ClientEvent(ClientEventEntry {
                event: nori_protocol::ClientEvent::ToolSnapshot(nori_protocol::ToolSnapshot {
                    call_id: "call-read-001".to_string(),
                    title: "Read Cargo.toml".to_string(),
                    kind: nori_protocol::ToolKind::Read,
                    phase: nori_protocol::ToolPhase::Completed,
                    locations: vec![],
                    invocation: Some(nori_protocol::Invocation::Read {
                        path: PathBuf::from("/repo/Cargo.toml"),
                    }),
                    artifacts: vec![nori_protocol::Artifact::Text {
                        text: "Read 42 lines".to_string(),
                    }],
                    raw_input: None,
                    raw_output: None,
                }),
            }),
            TranscriptEntry::ClientEvent(ClientEventEntry {
                event: nori_protocol::ClientEvent::ToolSnapshot(nori_protocol::ToolSnapshot {
                    call_id: "call-search-001".to_string(),
                    title: "Search src".to_string(),
                    kind: nori_protocol::ToolKind::Search,
                    phase: nori_protocol::ToolPhase::Completed,
                    locations: vec![],
                    invocation: Some(nori_protocol::Invocation::Search {
                        query: Some("TODO".to_string()),
                        path: Some(PathBuf::from("/repo/src")),
                    }),
                    artifacts: vec![nori_protocol::Artifact::Text {
                        text: "3 matches".to_string(),
                    }],
                    raw_input: None,
                    raw_output: None,
                }),
            }),
            TranscriptEntry::ClientEvent(ClientEventEntry {
                event: nori_protocol::ClientEvent::ToolSnapshot(nori_protocol::ToolSnapshot {
                    call_id: "call-list-001".to_string(),
                    title: "List src".to_string(),
                    kind: nori_protocol::ToolKind::Search,
                    phase: nori_protocol::ToolPhase::Completed,
                    locations: vec![],
                    invocation: Some(nori_protocol::Invocation::ListFiles {
                        path: Some(PathBuf::from("/repo/src")),
                    }),
                    artifacts: vec![],
                    raw_input: None,
                    raw_output: None,
                }),
            }),
        ]);

        let entries = transcript_to_entries(&transcript);

        assert_eq!(
            entries,
            vec![
                ViewonlyEntry::Info {
                    content: "Session from 2025-01-27 12:00 (test-ses)".to_string(),
                },
                ViewonlyEntry::Info {
                    content:
                        "Tool [completed]: Read Cargo.toml (read)\nRead: /repo/Cargo.toml\nOutput: Read 42 lines"
                            .to_string(),
                },
                ViewonlyEntry::Info {
                    content:
                        "Tool [completed]: Search src (search)\nSearch: TODO in /repo/src\nOutput: 3 matches"
                            .to_string(),
                },
                ViewonlyEntry::Info {
                    content:
                        "Tool [completed]: List src (search)\nList files: /repo/src".to_string(),
                },
            ]
        );
    }

    #[test]
    fn test_transcript_to_entries_renders_normalized_approval_request() {
        let transcript = make_transcript(vec![TranscriptEntry::ClientEvent(ClientEventEntry {
            event: nori_protocol::ClientEvent::ApprovalRequest(nori_protocol::ApprovalRequest {
                call_id: "call-approve-001".to_string(),
                title: "Write /tmp/main.rs".to_string(),
                kind: nori_protocol::ToolKind::Edit,
                options: vec![],
                subject: nori_protocol::ApprovalSubject::ToolSnapshot(
                    nori_protocol::ToolSnapshot {
                        call_id: "call-approve-001".to_string(),
                        title: "Write /tmp/main.rs".to_string(),
                        kind: nori_protocol::ToolKind::Edit,
                        phase: nori_protocol::ToolPhase::PendingApproval,
                        locations: vec![],
                        invocation: None,
                        artifacts: vec![],
                        raw_input: None,
                        raw_output: None,
                    },
                ),
            }),
        })]);

        let entries = transcript_to_entries(&transcript);

        assert_eq!(entries.len(), 2);
        assert!(
            matches!(&entries[1], ViewonlyEntry::Info { content } if content == "Approval requested: Write /tmp/main.rs (edit)")
        );
    }
}
