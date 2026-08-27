use super::*;

/// Character threshold above which we log a warning about transcript summary
/// size. The summary is not truncated — the agent-side "prompt too long"
/// rejection is the real guard — but large summaries are worth logging.
const TRANSCRIPT_SUMMARY_WARN_CHARS: usize = 200_000;

/// Recover the agent-to-client ACP notification stream stored in a v3
/// transcript. For v1/v2 transcripts, synthesize message notifications from
/// the stable user/assistant entries so old sessions remain replayable.
pub fn transcript_to_replay_session_events(
    transcript: &crate::transcript::Transcript,
) -> Vec<nori_protocol::SessionEvent> {
    let has_raw_notifications = transcript.entries.iter().any(|line| {
        matches!(
            &line.entry,
            crate::transcript::TranscriptEntry::SessionEvent(entry)
                if matches!(
                    entry.event,
                    nori_protocol::SessionEvent::Acp(
                        nori_protocol::AcpEvent::Notification(
                            nori_protocol::acp::v1::AgentNotification::SessionNotification(_)
                        )
                    )
                )
        )
    });
    let canonical_user_message_ids = transcript
        .entries
        .iter()
        .filter_map(|line| {
            let crate::transcript::TranscriptEntry::SessionEvent(entry) = &line.entry else {
                return None;
            };
            let nori_protocol::SessionEvent::Acp(nori_protocol::AcpEvent::Notification(
                nori_protocol::acp::v1::AgentNotification::SessionNotification(notification),
            )) = &entry.event
            else {
                return None;
            };
            let nori_protocol::acp::v1::SessionUpdate::UserMessageChunk(chunk) =
                &notification.update
            else {
                return None;
            };
            chunk.message_id.as_ref().map(ToString::to_string)
        })
        .collect::<std::collections::HashSet<_>>();

    let session_id = transcript
        .meta
        .acp_session_id
        .clone()
        .unwrap_or_else(|| format!("transcript-{}", transcript.meta.session_id));
    transcript
        .entries
        .iter()
        .flat_map(|line| {
            if has_raw_notifications
                && let crate::transcript::TranscriptEntry::SessionEvent(entry) = &line.entry
                && matches!(
                    entry.event,
                    nori_protocol::SessionEvent::Acp(nori_protocol::AcpEvent::Notification(
                        nori_protocol::acp::v1::AgentNotification::SessionNotification(_)
                    ))
                )
            {
                return vec![entry.event.clone()];
            }

            let updates: Vec<_> = match &line.entry {
                crate::transcript::TranscriptEntry::User(user)
                    if !canonical_user_message_ids.contains(&user.id) =>
                {
                    vec![nori_protocol::acp::v1::SessionUpdate::UserMessageChunk(
                        nori_protocol::acp::v1::ContentChunk::new(
                            nori_protocol::acp::v1::ContentBlock::Text(
                                nori_protocol::acp::v1::TextContent::new(user.content.clone()),
                            ),
                        ),
                    )]
                }
                crate::transcript::TranscriptEntry::User(_) => Vec::new(),
                crate::transcript::TranscriptEntry::Assistant(assistant)
                    if !has_raw_notifications =>
                {
                    assistant
                        .content
                        .iter()
                        .map(|block| match block {
                            ContentBlock::Text { text } => {
                                nori_protocol::acp::v1::SessionUpdate::AgentMessageChunk(
                                    nori_protocol::acp::v1::ContentChunk::new(
                                        nori_protocol::acp::v1::ContentBlock::Text(
                                            nori_protocol::acp::v1::TextContent::new(text.clone()),
                                        ),
                                    ),
                                )
                            }
                            ContentBlock::Thinking { thinking } => {
                                nori_protocol::acp::v1::SessionUpdate::AgentThoughtChunk(
                                    nori_protocol::acp::v1::ContentChunk::new(
                                        nori_protocol::acp::v1::ContentBlock::Text(
                                            nori_protocol::acp::v1::TextContent::new(
                                                thinking.clone(),
                                            ),
                                        ),
                                    ),
                                )
                            }
                        })
                        .collect()
                }
                _ => Vec::new(),
            };
            updates
                .into_iter()
                .map(|update| {
                    nori_protocol::SessionEvent::Acp(nori_protocol::AcpEvent::Notification(
                        nori_protocol::acp::v1::AgentNotification::SessionNotification(
                            nori_protocol::acp::v1::SessionNotification::new(
                                session_id.clone(),
                                update,
                            ),
                        ),
                    ))
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Convert a loaded transcript into normalized replay events suitable for ACP
/// session resume. The replay stream is intentionally static: it reconstructs
/// user/assistant history and completed normalized artifacts without reviving
/// live approval or turn-lifecycle state.
pub fn transcript_to_replay_client_events(
    transcript: &crate::transcript::Transcript,
) -> Vec<crate::normalized::ClientEvent> {
    let mut replay = Vec::new();

    for line in &transcript.entries {
        match &line.entry {
            crate::transcript::TranscriptEntry::User(user) => {
                replay.push(crate::normalized::ClientEvent::ReplayEntry(
                    crate::normalized::ReplayEntry::UserMessage {
                        text: user.content.clone(),
                    },
                ));
            }
            crate::transcript::TranscriptEntry::Assistant(assistant) => {
                for block in &assistant.content {
                    match block {
                        ContentBlock::Text { text } if !text.is_empty() => {
                            replay.push(crate::normalized::ClientEvent::ReplayEntry(
                                crate::normalized::ReplayEntry::AssistantMessage {
                                    text: text.clone(),
                                },
                            ))
                        }
                        ContentBlock::Thinking { thinking } if !thinking.is_empty() => {
                            replay.push(crate::normalized::ClientEvent::ReplayEntry(
                                crate::normalized::ReplayEntry::ReasoningMessage {
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
                    replay.push(crate::normalized::ClientEvent::ReplayEntry(replay_entry));
                } else if should_pass_through_replay_client_event(&client_event.event) {
                    replay.push(client_event.event.clone());
                }
            }
            crate::transcript::TranscriptEntry::SessionEvent(session_event) => match &session_event
                .event
            {
                nori_protocol::SessionEvent::Nori(nori_protocol::NoriEvent::GoalChanged(Some(
                    goal,
                ))) => {
                    replay.push(crate::normalized::ClientEvent::ThreadGoalUpdated(
                        crate::normalized::ThreadGoalUpdated { goal: goal.clone() },
                    ));
                }
                nori_protocol::SessionEvent::Nori(nori_protocol::NoriEvent::GoalChanged(None)) => {
                    replay.push(crate::normalized::ClientEvent::ThreadGoalCleared);
                }
                _ => {}
            },
            _ => {}
        }
    }

    replay
}

pub fn client_events_to_replay_client_events(
    client_events: Vec<crate::normalized::ClientEvent>,
) -> Vec<crate::normalized::ClientEvent> {
    let mut replay = Vec::new();
    let mut current_stream: Option<crate::normalized::MessageStream> = None;
    let mut current_text = String::new();

    let flush_message = |replay: &mut Vec<crate::normalized::ClientEvent>,
                         stream: &mut Option<crate::normalized::MessageStream>,
                         text: &mut String| {
        let Some(stream) = stream.take() else {
            return;
        };
        if text.is_empty() {
            return;
        }

        let text = std::mem::take(text);
        let entry = match stream {
            crate::normalized::MessageStream::User => {
                crate::normalized::ReplayEntry::UserMessage { text }
            }
            crate::normalized::MessageStream::Answer => {
                crate::normalized::ReplayEntry::AssistantMessage { text }
            }
            crate::normalized::MessageStream::Reasoning => {
                crate::normalized::ReplayEntry::ReasoningMessage { text }
            }
        };
        replay.push(crate::normalized::ClientEvent::ReplayEntry(entry));
    };

    for event in client_events {
        match event {
            crate::normalized::ClientEvent::MessageDelta(message_delta) => {
                if current_stream.as_ref() != Some(&message_delta.stream) {
                    flush_message(&mut replay, &mut current_stream, &mut current_text);
                    current_stream = Some(message_delta.stream);
                }
                current_text.push_str(&message_delta.delta);
            }
            other => {
                flush_message(&mut replay, &mut current_stream, &mut current_text);
                if let Some(replay_entry) = replay_entry_from_client_event(&other) {
                    replay.push(crate::normalized::ClientEvent::ReplayEntry(replay_entry));
                } else if should_pass_through_replay_client_event(&other) {
                    replay.push(other);
                }
            }
        }
    }

    flush_message(&mut replay, &mut current_stream, &mut current_text);
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
    let mut pending_raw_assistant = String::new();

    for line in &transcript.entries {
        if let crate::transcript::TranscriptEntry::SessionEvent(session_event) = &line.entry
            && let nori_protocol::SessionEvent::Acp(nori_protocol::AcpEvent::Notification(
                nori_protocol::acp::v1::AgentNotification::SessionNotification(notification),
            )) = &session_event.event
            && let nori_protocol::acp::v1::SessionUpdate::AgentMessageChunk(chunk) =
                &notification.update
            && let nori_protocol::acp::v1::ContentBlock::Text(text) = &chunk.content
        {
            pending_raw_assistant.push_str(&text.text);
            continue;
        }

        if !pending_raw_assistant.is_empty() {
            summary.push_str(&format!("Assistant: {pending_raw_assistant}\n"));
            pending_raw_assistant.clear();
        }

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
                    crate::normalized::ClientEvent::ToolSnapshot(tool_snapshot)
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

    if !pending_raw_assistant.is_empty() {
        summary.push_str(&format!("Assistant: {pending_raw_assistant}\n"));
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
    event: &crate::normalized::ClientEvent,
) -> Option<crate::normalized::ReplayEntry> {
    match event {
        crate::normalized::ClientEvent::ToolSnapshot(snapshot)
            if matches!(
                snapshot.phase,
                crate::normalized::ToolPhase::Completed | crate::normalized::ToolPhase::Failed
            ) =>
        {
            Some(crate::normalized::ReplayEntry::ToolSnapshot {
                snapshot: Box::new(snapshot.clone()),
            })
        }
        crate::normalized::ClientEvent::ToolSnapshot(_) => None,
        crate::normalized::ClientEvent::PlanSnapshot(snapshot) => {
            Some(crate::normalized::ReplayEntry::PlanSnapshot {
                snapshot: snapshot.clone(),
            })
        }
        crate::normalized::ClientEvent::ApprovalRequest(_)
        | crate::normalized::ClientEvent::MessageDelta(_)
        | crate::normalized::ClientEvent::SessionPhaseChanged(_)
        | crate::normalized::ClientEvent::PromptCompleted(_)
        | crate::normalized::ClientEvent::LoadCompleted
        | crate::normalized::ClientEvent::QueueChanged(_)
        | crate::normalized::ClientEvent::ContextCompacted(_)
        | crate::normalized::ClientEvent::ReplayEntry(_)
        | crate::normalized::ClientEvent::AgentCommandsUpdate(_)
        | crate::normalized::ClientEvent::SessionCapabilitiesChanged(_)
        | crate::normalized::ClientEvent::SessionUpdateInfo(_)
        | crate::normalized::ClientEvent::SessionConfigUpdate(_)
        | crate::normalized::ClientEvent::SessionModeChanged(_)
        | crate::normalized::ClientEvent::ThreadGoalUpdated(_)
        | crate::normalized::ClientEvent::ThreadGoalCleared
        | crate::normalized::ClientEvent::Warning(_) => None,
    }
}

fn should_pass_through_replay_client_event(event: &crate::normalized::ClientEvent) -> bool {
    matches!(
        event,
        crate::normalized::ClientEvent::SessionUpdateInfo(_)
            | crate::normalized::ClientEvent::ThreadGoalUpdated(_)
            | crate::normalized::ClientEvent::ThreadGoalCleared
    )
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use pretty_assertions::assert_eq;

    use super::*;
    use crate::transcript::AssistantEntry;
    use crate::transcript::ClientEventEntry;
    use crate::transcript::ContentBlock;
    use crate::transcript::SessionEventEntry;
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
            forked_from: None,
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
    fn v3_replay_preserves_user_turns_between_raw_acp_notifications() {
        let raw_assistant_event =
            nori_protocol::SessionEvent::Acp(nori_protocol::AcpEvent::Notification(
                nori_protocol::acp::v1::AgentNotification::SessionNotification(
                    nori_protocol::acp::v1::SessionNotification::new(
                        "recorded-session",
                        nori_protocol::acp::v1::SessionUpdate::AgentMessageChunk(
                            nori_protocol::acp::v1::ContentChunk::new(
                                nori_protocol::acp::v1::ContentBlock::Text(
                                    nori_protocol::acp::v1::TextContent::new("Stored answer"),
                                ),
                            ),
                        ),
                    ),
                ),
            ));
        let transcript = make_transcript(vec![
            TranscriptEntry::User(UserEntry {
                id: "user-1".into(),
                content: "Stored question".into(),
                attachments: vec![],
            }),
            TranscriptEntry::SessionEvent(SessionEventEntry {
                event: raw_assistant_event.clone(),
            }),
        ]);

        let replay = transcript_to_replay_session_events(&transcript);

        assert_eq!(replay.len(), 2);
        assert!(matches!(
            &replay[0],
            nori_protocol::SessionEvent::Acp(nori_protocol::AcpEvent::Notification(
                nori_protocol::acp::v1::AgentNotification::SessionNotification(notification)
            )) if matches!(
                &notification.update,
                nori_protocol::acp::v1::SessionUpdate::UserMessageChunk(chunk)
                    if matches!(
                        &chunk.content,
                        nori_protocol::acp::v1::ContentBlock::Text(text)
                            if text.text == "Stored question"
                    )
            )
        ));
        assert_eq!(
            serde_json::to_value(&replay[1]).unwrap(),
            serde_json::to_value(raw_assistant_event).unwrap()
        );
    }

    #[test]
    fn v3_replay_uses_canonical_raw_user_chunks_without_synthesizing_a_duplicate() {
        let message_id = nori_protocol::acp::v1::MessageId::new("user-1");
        let raw_user_events = vec![
            nori_protocol::SessionEvent::Acp(nori_protocol::AcpEvent::Notification(
                nori_protocol::acp::v1::AgentNotification::SessionNotification(
                    nori_protocol::acp::v1::SessionNotification::new(
                        "recorded-session",
                        nori_protocol::acp::v1::SessionUpdate::UserMessageChunk(
                            nori_protocol::acp::v1::ContentChunk::new(
                                nori_protocol::acp::v1::ContentBlock::Text(
                                    nori_protocol::acp::v1::TextContent::new("Stored question"),
                                ),
                            )
                            .message_id(message_id.clone()),
                        ),
                    ),
                ),
            )),
            nori_protocol::SessionEvent::Acp(nori_protocol::AcpEvent::Notification(
                nori_protocol::acp::v1::AgentNotification::SessionNotification(
                    nori_protocol::acp::v1::SessionNotification::new(
                        "recorded-session",
                        nori_protocol::acp::v1::SessionUpdate::UserMessageChunk(
                            nori_protocol::acp::v1::ContentChunk::new(
                                nori_protocol::acp::v1::ContentBlock::Image(
                                    nori_protocol::acp::v1::ImageContent::new(
                                        "aW1hZ2U=",
                                        "image/png",
                                    ),
                                ),
                            )
                            .message_id(message_id),
                        ),
                    ),
                ),
            )),
        ];
        let transcript = make_transcript(vec![
            TranscriptEntry::User(UserEntry {
                id: "user-1".into(),
                content: "Stored question".into(),
                attachments: vec![],
            }),
            TranscriptEntry::SessionEvent(SessionEventEntry {
                event: raw_user_events[0].clone(),
            }),
            TranscriptEntry::SessionEvent(SessionEventEntry {
                event: raw_user_events[1].clone(),
            }),
        ]);

        assert_eq!(
            transcript_to_replay_session_events(&transcript)
                .into_iter()
                .map(|event| serde_json::to_value(event).unwrap())
                .collect::<Vec<_>>(),
            raw_user_events
                .into_iter()
                .map(|event| serde_json::to_value(event).unwrap())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn v3_summary_includes_raw_acp_assistant_text() {
        let transcript = make_transcript(vec![
            TranscriptEntry::User(UserEntry {
                id: "user-1".into(),
                content: "Stored question".into(),
                attachments: vec![],
            }),
            TranscriptEntry::SessionEvent(SessionEventEntry {
                event: nori_protocol::SessionEvent::Acp(nori_protocol::AcpEvent::Notification(
                    nori_protocol::acp::v1::AgentNotification::SessionNotification(
                        nori_protocol::acp::v1::SessionNotification::new(
                            "recorded-session",
                            nori_protocol::acp::v1::SessionUpdate::AgentMessageChunk(
                                nori_protocol::acp::v1::ContentChunk::new(
                                    nori_protocol::acp::v1::ContentBlock::Text(
                                        nori_protocol::acp::v1::TextContent::new("Stored answer"),
                                    ),
                                ),
                            ),
                        ),
                    ),
                )),
            }),
        ]);

        assert_eq!(
            transcript_to_summary(&transcript),
            "User: Stored question\nAssistant: Stored answer\n"
        );
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
                event: crate::normalized::ClientEvent::ToolSnapshot(
                    crate::normalized::ToolSnapshot {
                        call_id: "tool-1".into(),
                        title: "Read Cargo.toml".into(),
                        kind: crate::normalized::ToolKind::Read,
                        phase: crate::normalized::ToolPhase::Completed,
                        locations: vec![],
                        invocation: Some(crate::normalized::Invocation::Read {
                            path: PathBuf::from("Cargo.toml"),
                        }),
                        artifacts: vec![],
                        raw_input: None,
                        raw_output: None,
                        owner_request_id: None,
                    },
                ),
            }),
            TranscriptEntry::ClientEvent(ClientEventEntry {
                event: crate::normalized::ClientEvent::MessageDelta(
                    crate::normalized::MessageDelta {
                        stream: crate::normalized::MessageStream::Answer,
                        delta: "duplicate streamed text".into(),
                    },
                ),
            }),
        ]);

        let replay = transcript_to_replay_client_events(&transcript);

        assert_eq!(
            replay,
            vec![
                crate::normalized::ClientEvent::ReplayEntry(
                    crate::normalized::ReplayEntry::UserMessage {
                        text: "Inspect the repo".into(),
                    }
                ),
                crate::normalized::ClientEvent::ReplayEntry(
                    crate::normalized::ReplayEntry::ReasoningMessage {
                        text: "Need to inspect files".into(),
                    },
                ),
                crate::normalized::ClientEvent::ReplayEntry(
                    crate::normalized::ReplayEntry::AssistantMessage {
                        text: "I found the ACP bridge.".into(),
                    },
                ),
                crate::normalized::ClientEvent::ReplayEntry(
                    crate::normalized::ReplayEntry::ToolSnapshot {
                        snapshot: Box::new(crate::normalized::ToolSnapshot {
                            call_id: "tool-1".into(),
                            title: "Read Cargo.toml".into(),
                            kind: crate::normalized::ToolKind::Read,
                            phase: crate::normalized::ToolPhase::Completed,
                            locations: vec![],
                            invocation: Some(crate::normalized::Invocation::Read {
                                path: PathBuf::from("Cargo.toml"),
                            }),
                            artifacts: vec![],
                            raw_input: None,
                            raw_output: None,
                            owner_request_id: None,
                        }),
                    }
                ),
            ]
        );
    }

    #[test]
    fn client_events_to_replay_client_events_buffers_user_deltas_and_preserves_info_updates() {
        let replay = client_events_to_replay_client_events(vec![
            crate::normalized::ClientEvent::MessageDelta(crate::normalized::MessageDelta {
                stream: crate::normalized::MessageStream::User,
                delta: "Resume".into(),
            }),
            crate::normalized::ClientEvent::MessageDelta(crate::normalized::MessageDelta {
                stream: crate::normalized::MessageStream::User,
                delta: " this session".into(),
            }),
            crate::normalized::ClientEvent::SessionUpdateInfo(
                crate::normalized::SessionUpdateInfo {
                    kind: crate::normalized::SessionUpdateKind::SessionInfo,
                    message: "Session info updated: title=\"Resume chat\"".into(),
                    hint: None,
                    usage: None,
                },
            ),
            crate::normalized::ClientEvent::MessageDelta(crate::normalized::MessageDelta {
                stream: crate::normalized::MessageStream::Answer,
                delta: "Loaded.".into(),
            }),
        ]);

        assert_eq!(
            replay,
            vec![
                crate::normalized::ClientEvent::ReplayEntry(
                    crate::normalized::ReplayEntry::UserMessage {
                        text: "Resume this session".into(),
                    }
                ),
                crate::normalized::ClientEvent::SessionUpdateInfo(
                    crate::normalized::SessionUpdateInfo {
                        kind: crate::normalized::SessionUpdateKind::SessionInfo,
                        message: "Session info updated: title=\"Resume chat\"".into(),
                        hint: None,
                        usage: None,
                    }
                ),
                crate::normalized::ClientEvent::ReplayEntry(
                    crate::normalized::ReplayEntry::AssistantMessage {
                        text: "Loaded.".into(),
                    },
                ),
            ]
        );
    }

    #[test]
    fn client_events_to_replay_client_events_preserves_goal_updates() {
        let goal_event = crate::normalized::ClientEvent::ThreadGoalUpdated(
            crate::normalized::ThreadGoalUpdated {
                goal: nori_protocol::ThreadGoal {
                    objective: "Keep the north star".to_string(),
                    status: nori_protocol::ThreadGoalStatus::Active,
                    tokens_used: 42,
                    time_used_seconds: 7,
                    created_at: 100,
                    updated_at: 107,
                },
            },
        );
        let replay = client_events_to_replay_client_events(vec![goal_event.clone()]);

        assert_eq!(replay, vec![goal_event]);
    }

    #[test]
    fn client_events_to_replay_client_events_preserves_goal_clears() {
        let replay = client_events_to_replay_client_events(vec![
            crate::normalized::ClientEvent::ThreadGoalCleared,
        ]);

        assert_eq!(
            replay,
            vec![crate::normalized::ClientEvent::ThreadGoalCleared]
        );
    }

    #[test]
    fn client_events_to_replay_client_events_preserves_mixed_message_delta_order() {
        let replay = client_events_to_replay_client_events(vec![
            crate::normalized::ClientEvent::MessageDelta(crate::normalized::MessageDelta {
                stream: crate::normalized::MessageStream::Answer,
                delta: "CI is green.".into(),
            }),
            crate::normalized::ClientEvent::MessageDelta(crate::normalized::MessageDelta {
                stream: crate::normalized::MessageStream::Reasoning,
                delta: "Preparing PR.".into(),
            }),
            crate::normalized::ClientEvent::MessageDelta(crate::normalized::MessageDelta {
                stream: crate::normalized::MessageStream::Answer,
                delta: "The PR is up.".into(),
            }),
        ]);

        assert_eq!(
            replay,
            vec![
                crate::normalized::ClientEvent::ReplayEntry(
                    crate::normalized::ReplayEntry::AssistantMessage {
                        text: "CI is green.".into(),
                    },
                ),
                crate::normalized::ClientEvent::ReplayEntry(
                    crate::normalized::ReplayEntry::ReasoningMessage {
                        text: "Preparing PR.".into(),
                    },
                ),
                crate::normalized::ClientEvent::ReplayEntry(
                    crate::normalized::ReplayEntry::AssistantMessage {
                        text: "The PR is up.".into(),
                    },
                ),
            ]
        );
    }

    #[test]
    fn transcript_to_replay_client_events_preserves_session_update_info() {
        let transcript = make_transcript(vec![TranscriptEntry::ClientEvent(ClientEventEntry {
            event: crate::normalized::ClientEvent::SessionUpdateInfo(
                crate::normalized::SessionUpdateInfo {
                    kind: crate::normalized::SessionUpdateKind::Usage,
                    message: "Session usage: 128 / 4096 tokens".into(),
                    hint: None,
                    usage: Some(crate::normalized::session_runtime::SessionUsageState {
                        used_tokens: 128,
                        total_tokens: 4096,
                        cost_display: None,
                    }),
                },
            ),
        })]);

        let replay = transcript_to_replay_client_events(&transcript);

        assert_eq!(
            replay,
            vec![crate::normalized::ClientEvent::SessionUpdateInfo(
                crate::normalized::SessionUpdateInfo {
                    kind: crate::normalized::SessionUpdateKind::Usage,
                    message: "Session usage: 128 / 4096 tokens".into(),
                    hint: None,
                    usage: Some(crate::normalized::session_runtime::SessionUsageState {
                        used_tokens: 128,
                        total_tokens: 4096,
                        cost_display: None,
                    }),
                },
            )]
        );
    }
}
