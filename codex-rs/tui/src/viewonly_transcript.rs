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
                let content = assistant
                    .content
                    .iter()
                    .map(|block| {
                        let ContentBlock::Text { text } = block;
                        text.clone()
                    })
                    .collect::<Vec<_>>()
                    .join("");
                entries.push(ViewonlyEntry::Assistant { content });
            }
            TranscriptEntry::ToolCall(tool) => {
                entries.push(ViewonlyEntry::Info {
                    content: format!("Tool: {} ({})", tool.name, tool.call_id),
                });
            }
            TranscriptEntry::ToolResult(result) => {
                let output = if result.truncated {
                    format!("{} [truncated]", &result.output)
                } else {
                    result.output.clone()
                };
                entries.push(ViewonlyEntry::Info {
                    content: format!("Result: {}", truncate_str(&output, 200)),
                });
            }
            TranscriptEntry::PatchApply(patch) => {
                let status = if patch.success { "applied" } else { "failed" };
                entries.push(ViewonlyEntry::Info {
                    content: format!(
                        "Patch {}: {} ({:?})",
                        status,
                        patch.path.display(),
                        patch.operation
                    ),
                });
            }
        }
    }

    entries
}

fn format_timestamp(iso: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(iso)
        .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_else(|_| iso.to_string())
}

fn truncate_str(s: &str, max_chars: usize) -> String {
    if s.chars().count() > max_chars {
        let truncated: String = s.chars().take(max_chars).collect();
        format!("{truncated}...")
    } else {
        s.to_string()
    }
}
