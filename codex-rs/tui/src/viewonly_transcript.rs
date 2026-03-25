//! View-only transcript display.
//!
//! This module converts transcript entries into displayable history cells
//! for the view-only transcript viewer.

use codex_acp::transcript::ContentBlock;
use codex_acp::transcript::Transcript;
use codex_acp::transcript::TranscriptEntry;

/// A simplified entry for display in the view-only transcript viewer.
#[derive(Debug, Clone)]
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
                if let Some(content) = format_client_event(&client_event.event) {
                    entries.push(ViewonlyEntry::Info { content });
                }
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

fn format_client_event(event: &nori_protocol::ClientEvent) -> Option<String> {
    match event {
        nori_protocol::ClientEvent::ToolSnapshot(tool_snapshot) => Some(format!(
            "Tool [{}]: {} ({})",
            format_tool_phase(&tool_snapshot.phase),
            tool_snapshot.title,
            format_tool_kind(&tool_snapshot.kind)
        )),
        nori_protocol::ClientEvent::ApprovalRequest(approval) => Some(format!(
            "Approval requested: {} ({})",
            approval.title,
            format_tool_kind(&approval.kind)
        )),
    }
}

fn format_tool_kind(kind: &nori_protocol::ToolKind) -> &str {
    match kind {
        nori_protocol::ToolKind::Read => "read",
        nori_protocol::ToolKind::Search => "search",
        nori_protocol::ToolKind::Execute => "execute",
        nori_protocol::ToolKind::Edit => "edit",
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
