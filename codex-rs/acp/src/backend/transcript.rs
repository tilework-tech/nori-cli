use super::*;

/// Character threshold above which we log a warning about transcript summary
/// size. The summary is not truncated — the agent-side "prompt too long"
/// rejection is the real guard — but large summaries are worth logging.
const TRANSCRIPT_SUMMARY_WARN_CHARS: usize = 200_000;

/// Convert a loaded transcript into a list of `EventMsg` suitable for
/// `SessionConfiguredEvent.initial_messages` (UI replay).
///
/// Only `User` and `Assistant` entries are converted; tool calls, results,
/// patches, and session metadata are skipped since the UI does not need to
/// replay the full tool lifecycle for display purposes.
pub fn transcript_to_replay_events(transcript: &crate::transcript::Transcript) -> Vec<EventMsg> {
    use codex_protocol::protocol::AgentMessageEvent;
    use codex_protocol::protocol::UserMessageEvent;

    transcript
        .entries
        .iter()
        .filter_map(|line| match &line.entry {
            crate::transcript::TranscriptEntry::User(user) => {
                Some(EventMsg::UserMessage(UserMessageEvent {
                    message: user.content.clone(),
                    images: None,
                }))
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
                if text.is_empty() {
                    None
                } else {
                    Some(EventMsg::AgentMessage(AgentMessageEvent { message: text }))
                }
            }
            _ => None,
        })
        .collect()
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
