use super::*;

/// Maximum character length for the transcript summary text.
const TRANSCRIPT_SUMMARY_MAX_CHARS: usize = 20_000;

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
pub fn transcript_to_summary(transcript: &crate::transcript::Transcript) -> String {
    let mut summary = String::new();

    for line in &transcript.entries {
        if summary.len() >= TRANSCRIPT_SUMMARY_MAX_CHARS {
            summary.push_str("\n[...transcript truncated...]");
            break;
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
            _ => {}
        }
    }

    // Final truncation guard: find the nearest char boundary at or before
    // the limit to avoid panicking on multi-byte UTF-8 (CJK, emoji, etc.).
    if summary.len() > TRANSCRIPT_SUMMARY_MAX_CHARS {
        let mut boundary = TRANSCRIPT_SUMMARY_MAX_CHARS;
        while !summary.is_char_boundary(boundary) {
            boundary -= 1;
        }
        summary.truncate(boundary);
        summary.push_str("\n[...truncated...]");
    }

    summary
}
