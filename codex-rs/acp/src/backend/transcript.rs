use super::*;

/// Character threshold above which we log a warning about transcript summary
/// size. The summary is not truncated — the agent-side "prompt too long"
/// rejection is the real guard — but large summaries are worth logging.
const TRANSCRIPT_SUMMARY_WARN_CHARS: usize = 200_000;

pub(crate) fn should_record_client_event(event: &nori_protocol::ClientEvent) -> bool {
    !matches!(
        event,
        nori_protocol::ClientEvent::ToolSnapshot(snapshot)
            if snapshot.phase == nori_protocol::ToolPhase::InProgress
    )
}

/// Convert a loaded transcript into normalized replay events suitable for ACP
/// session resume. The replay stream is intentionally static: it reconstructs
/// user/assistant history and completed normalized artifacts without reviving
/// live approval or turn-lifecycle state.
pub fn transcript_to_replay_client_events(
    transcript: &crate::transcript::Transcript,
) -> Vec<nori_protocol::ClientEvent> {
    let mut replay = Vec::new();

    for line in &transcript.entries {
        match &line.entry {
            crate::transcript::TranscriptEntry::User(user) => {
                replay.push(nori_protocol::ClientEvent::ReplayEntry(
                    nori_protocol::ReplayEntry::UserMessage {
                        text: user.content.clone(),
                    },
                ));
            }
            crate::transcript::TranscriptEntry::Assistant(assistant) => {
                for block in &assistant.content {
                    match block {
                        ContentBlock::Text { text } if !text.is_empty() => {
                            replay.push(nori_protocol::ClientEvent::ReplayEntry(
                                nori_protocol::ReplayEntry::AssistantMessage { text: text.clone() },
                            ))
                        }
                        ContentBlock::Thinking { thinking } if !thinking.is_empty() => {
                            replay.push(nori_protocol::ClientEvent::ReplayEntry(
                                nori_protocol::ReplayEntry::ReasoningMessage {
                                    text: thinking.clone(),
                                },
                            ))
                        }
                        _ => {}
                    }
                }
            }
            crate::transcript::TranscriptEntry::ClientEvent(client_event) => {
                if let Some(replay_entry) = replay_entry_from_client_event(&client_event.event) {
                    replay.push(nori_protocol::ClientEvent::ReplayEntry(replay_entry));
                } else if should_pass_through_replay_client_event(&client_event.event) {
                    replay.push(client_event.event.clone());
                }
            }
            _ => {}
        }
    }

    replay
}

pub fn client_events_to_replay_client_events(
    client_events: Vec<nori_protocol::ClientEvent>,
) -> Vec<nori_protocol::ClientEvent> {
    let mut replay = Vec::new();
    let mut user = String::new();
    let mut assistant = String::new();
    let mut reasoning = String::new();

    let flush_buffers = |replay: &mut Vec<nori_protocol::ClientEvent>,
                         user: &mut String,
                         assistant: &mut String,
                         reasoning: &mut String| {
        if !user.is_empty() {
            replay.push(nori_protocol::ClientEvent::ReplayEntry(
                nori_protocol::ReplayEntry::UserMessage {
                    text: std::mem::take(user),
                },
            ));
        }
        if !reasoning.is_empty() {
            replay.push(nori_protocol::ClientEvent::ReplayEntry(
                nori_protocol::ReplayEntry::ReasoningMessage {
                    text: std::mem::take(reasoning),
                },
            ));
        }
        if !assistant.is_empty() {
            replay.push(nori_protocol::ClientEvent::ReplayEntry(
                nori_protocol::ReplayEntry::AssistantMessage {
                    text: std::mem::take(assistant),
                },
            ));
        }
    };

    for event in client_events {
        match event {
            nori_protocol::ClientEvent::MessageDelta(message_delta) => match message_delta.stream {
                nori_protocol::MessageStream::User => user.push_str(&message_delta.delta),
                nori_protocol::MessageStream::Answer => assistant.push_str(&message_delta.delta),
                nori_protocol::MessageStream::Reasoning => reasoning.push_str(&message_delta.delta),
            },
            other => {
                flush_buffers(&mut replay, &mut user, &mut assistant, &mut reasoning);
                if let Some(replay_entry) = replay_entry_from_client_event(&other) {
                    replay.push(nori_protocol::ClientEvent::ReplayEntry(replay_entry));
                } else if should_pass_through_replay_client_event(&other) {
                    replay.push(other);
                }
            }
        }
    }

    flush_buffers(&mut replay, &mut user, &mut assistant, &mut reasoning);
    replay
}

/// Convert a loaded transcript into a human-readable summary string suitable
/// for injecting into the first prompt via `pending_compact_summary`.
///
/// The summary captures user messages, assistant responses, and tool call
/// names so the agent has context about the previous conversation without
/// needing the full tool lifecycle details.
///
/// No truncation is applied — the full transcript is preserved so the agent
/// retains as much context as possible on resume. If the resulting prompt
/// exceeds the model's context window, the agent will reject it with a
/// "prompt too long" error, which is handled gracefully by the caller.
pub fn transcript_to_summary(transcript: &crate::transcript::Transcript) -> String {
    let mut seen_tool_calls = std::collections::HashSet::new();
    let mut summary = String::new();

    for line in &transcript.entries {
        match &line.entry {
            crate::transcript::TranscriptEntry::User(user) => {
                summary.push_str(&format!("User: {}\n", user.content));
            }
            crate::transcript::TranscriptEntry::Assistant(assistant) => {
                let text: String = assistant
                    .content
                    .iter()
                    .filter_map(|block| match block {
                        ContentBlock::Text { text } => Some(text.as_str()),
                        ContentBlock::Thinking { .. } => None,
                    })
                    .collect::<Vec<_>>()
                    .join("");
                if !text.is_empty() {
                    summary.push_str(&format!("Assistant: {text}\n"));
                }
            }
            crate::transcript::TranscriptEntry::ToolCall(tool) => {
                summary.push_str(&format!("[Tool: {}]\n", tool.name));
            }
            crate::transcript::TranscriptEntry::ClientEvent(client_event) => {
                match &client_event.event {
                    nori_protocol::ClientEvent::ToolSnapshot(tool_snapshot)
                        if seen_tool_calls.insert(tool_snapshot.call_id.clone()) =>
                    {
                        summary.push_str(&format!("[Tool: {}]\n", tool_snapshot.title));
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    if summary.len() > TRANSCRIPT_SUMMARY_WARN_CHARS {
        warn!(
            "Transcript summary is very large ({} chars). \
             If the agent rejects it as too long, try /compact or start a new session.",
            summary.len()
        );
    }

    summary
}

fn replay_entry_from_client_event(
    event: &nori_protocol::ClientEvent,
) -> Option<nori_protocol::ReplayEntry> {
    match event {
        nori_protocol::ClientEvent::ToolSnapshot(snapshot)
            if matches!(
                snapshot.phase,
                nori_protocol::ToolPhase::Completed | nori_protocol::ToolPhase::Failed
            ) =>
        {
            Some(nori_protocol::ReplayEntry::ToolSnapshot {
                snapshot: Box::new(snapshot.clone()),
            })
        }
        nori_protocol::ClientEvent::ToolSnapshot(_) => None,
        nori_protocol::ClientEvent::PlanSnapshot(snapshot) => {
            Some(nori_protocol::ReplayEntry::PlanSnapshot {
                snapshot: snapshot.clone(),
            })
        }
        nori_protocol::ClientEvent::ApprovalRequest(_)
        | nori_protocol::ClientEvent::MessageDelta(_)
        | nori_protocol::ClientEvent::SessionPhaseChanged(_)
        | nori_protocol::ClientEvent::PromptCompleted(_)
        | nori_protocol::ClientEvent::LoadCompleted
        | nori_protocol::ClientEvent::QueueChanged(_)
        | nori_protocol::ClientEvent::ContextCompacted(_)
        | nori_protocol::ClientEvent::ReplayEntry(_)
        | nori_protocol::ClientEvent::AgentCommandsUpdate(_)
        | nori_protocol::ClientEvent::SessionUpdateInfo(_)
        | nori_protocol::ClientEvent::Warning(_) => None,
    }
}

fn should_pass_through_replay_client_event(event: &nori_protocol::ClientEvent) -> bool {
    matches!(event, nori_protocol::ClientEvent::SessionUpdateInfo(_))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use pretty_assertions::assert_eq;

    use super::*;
    use crate::transcript::AssistantEntry;
    use crate::transcript::ClientEventEntry;
    use crate::transcript::ContentBlock;
    use crate::transcript::SessionMetaEntry;
    use crate::transcript::Transcript;
    use crate::transcript::TranscriptEntry;
    use crate::transcript::TranscriptLine;
    use crate::transcript::UserEntry;

    fn make_transcript(entries: Vec<TranscriptEntry>) -> Transcript {
        let meta = SessionMetaEntry {
            session_id: "session-1".into(),
            project_id: "project-1".into(),
            started_at: "2025-01-01T00:00:00.000Z".into(),
            cwd: PathBuf::from("/repo"),
            agent: Some("claude-code".into()),
            cli_version: "0.1.0".into(),
            git: None,
            acp_session_id: None,
        };

        let mut lines = vec![TranscriptLine::new(TranscriptEntry::SessionMeta(
            meta.clone(),
        ))];
        lines.extend(entries.into_iter().map(TranscriptLine::new));

        Transcript {
            meta,
            entries: lines,
        }
    }

    #[test]
    fn transcript_replay_client_events_preserve_user_assistant_and_tool_snapshot() {
        let transcript = make_transcript(vec![
            TranscriptEntry::User(UserEntry {
                id: "user-1".into(),
                content: "Inspect the repo".into(),
                attachments: vec![],
            }),
            TranscriptEntry::Assistant(AssistantEntry {
                id: "assistant-1".into(),
                content: vec![
                    ContentBlock::Thinking {
                        thinking: "Need to inspect files".into(),
                    },
                    ContentBlock::Text {
                        text: "I found the ACP bridge.".into(),
                    },
                ],
                agent: Some("claude-code".into()),
            }),
            TranscriptEntry::ClientEvent(ClientEventEntry {
                event: nori_protocol::ClientEvent::ToolSnapshot(nori_protocol::ToolSnapshot {
                    call_id: "tool-1".into(),
                    title: "Read Cargo.toml".into(),
                    kind: nori_protocol::ToolKind::Read,
                    phase: nori_protocol::ToolPhase::Completed,
                    locations: vec![],
                    invocation: Some(nori_protocol::Invocation::Read {
                        path: PathBuf::from("Cargo.toml"),
                    }),
                    artifacts: vec![],
                    raw_input: None,
                    raw_output: None,
                    owner_request_id: None,
                }),
            }),
            TranscriptEntry::ClientEvent(ClientEventEntry {
                event: nori_protocol::ClientEvent::MessageDelta(nori_protocol::MessageDelta {
                    stream: nori_protocol::MessageStream::Answer,
                    delta: "duplicate streamed text".into(),
                }),
            }),
        ]);

        let replay = transcript_to_replay_client_events(&transcript);

        assert_eq!(
            replay,
            vec![
                nori_protocol::ClientEvent::ReplayEntry(nori_protocol::ReplayEntry::UserMessage {
                    text: "Inspect the repo".into(),
                }),
                nori_protocol::ClientEvent::ReplayEntry(
                    nori_protocol::ReplayEntry::ReasoningMessage {
                        text: "Need to inspect files".into(),
                    },
                ),
                nori_protocol::ClientEvent::ReplayEntry(
                    nori_protocol::ReplayEntry::AssistantMessage {
                        text: "I found the ACP bridge.".into(),
                    },
                ),
                nori_protocol::ClientEvent::ReplayEntry(nori_protocol::ReplayEntry::ToolSnapshot {
                    snapshot: Box::new(nori_protocol::ToolSnapshot {
                        call_id: "tool-1".into(),
                        title: "Read Cargo.toml".into(),
                        kind: nori_protocol::ToolKind::Read,
                        phase: nori_protocol::ToolPhase::Completed,
                        locations: vec![],
                        invocation: Some(nori_protocol::Invocation::Read {
                            path: PathBuf::from("Cargo.toml"),
                        }),
                        artifacts: vec![],
                        raw_input: None,
                        raw_output: None,
                        owner_request_id: None,
                    }),
                }),
            ]
        );
    }

    #[test]
    fn client_events_to_replay_client_events_buffers_user_deltas_and_preserves_info_updates() {
        let replay = client_events_to_replay_client_events(vec![
            nori_protocol::ClientEvent::MessageDelta(nori_protocol::MessageDelta {
                stream: nori_protocol::MessageStream::User,
                delta: "Resume".into(),
            }),
            nori_protocol::ClientEvent::MessageDelta(nori_protocol::MessageDelta {
                stream: nori_protocol::MessageStream::User,
                delta: " this session".into(),
            }),
            nori_protocol::ClientEvent::SessionUpdateInfo(nori_protocol::SessionUpdateInfo {
                kind: nori_protocol::SessionUpdateKind::SessionInfo,
                message: "Session info updated: title=\"Resume chat\"".into(),
                hint: None,
            }),
            nori_protocol::ClientEvent::MessageDelta(nori_protocol::MessageDelta {
                stream: nori_protocol::MessageStream::Answer,
                delta: "Loaded.".into(),
            }),
        ]);

        assert_eq!(
            replay,
            vec![
                nori_protocol::ClientEvent::ReplayEntry(nori_protocol::ReplayEntry::UserMessage {
                    text: "Resume this session".into(),
                }),
                nori_protocol::ClientEvent::SessionUpdateInfo(nori_protocol::SessionUpdateInfo {
                    kind: nori_protocol::SessionUpdateKind::SessionInfo,
                    message: "Session info updated: title=\"Resume chat\"".into(),
                    hint: None,
                }),
                nori_protocol::ClientEvent::ReplayEntry(
                    nori_protocol::ReplayEntry::AssistantMessage {
                        text: "Loaded.".into(),
                    },
                ),
            ]
        );
    }

    #[test]
    fn transcript_to_replay_client_events_preserves_session_update_info() {
        let transcript = make_transcript(vec![TranscriptEntry::ClientEvent(ClientEventEntry {
            event: nori_protocol::ClientEvent::SessionUpdateInfo(
                nori_protocol::SessionUpdateInfo {
                    kind: nori_protocol::SessionUpdateKind::Usage,
                    message: "Session usage: 128 / 4096 tokens".into(),
                    hint: None,
                },
            ),
        })]);

        let replay = transcript_to_replay_client_events(&transcript);

        assert_eq!(
            replay,
            vec![nori_protocol::ClientEvent::SessionUpdateInfo(
                nori_protocol::SessionUpdateInfo {
                    kind: nori_protocol::SessionUpdateKind::Usage,
                    message: "Session usage: 128 / 4096 tokens".into(),
                    hint: None,
                },
            )]
        );
    }

    #[test]
    fn transcript_recording_skips_in_progress_tool_snapshots() {
        let in_progress = nori_protocol::ClientEvent::ToolSnapshot(nori_protocol::ToolSnapshot {
            call_id: "tool-1".into(),
            title: "Search repo".into(),
            kind: nori_protocol::ToolKind::Search,
            phase: nori_protocol::ToolPhase::InProgress,
            locations: vec![],
            invocation: Some(nori_protocol::Invocation::Search {
                query: Some("needle".into()),
                path: Some(PathBuf::from("/repo")),
            }),
            artifacts: vec![nori_protocol::Artifact::Text {
                text: "streaming output".into(),
            }],
            raw_input: None,
            raw_output: None,
            owner_request_id: None,
        });
        let pending = nori_protocol::ClientEvent::ToolSnapshot(nori_protocol::ToolSnapshot {
            phase: nori_protocol::ToolPhase::Pending,
            ..match &in_progress {
                nori_protocol::ClientEvent::ToolSnapshot(snapshot) => snapshot.clone(),
                _ => unreachable!(),
            }
        });
        let completed = nori_protocol::ClientEvent::ToolSnapshot(nori_protocol::ToolSnapshot {
            phase: nori_protocol::ToolPhase::Completed,
            ..match &in_progress {
                nori_protocol::ClientEvent::ToolSnapshot(snapshot) => snapshot.clone(),
                _ => unreachable!(),
            }
        });

        assert!(!should_record_client_event(&in_progress));
        assert!(should_record_client_event(&pending));
        assert!(should_record_client_event(&completed));
    }
}
