use std::path::Path;

/// Replace occurrences of the given `cwd` prefix (with trailing `/`) in `text`
/// with an empty string, effectively turning absolute paths under `cwd` into
/// relative ones. Only replaces when the prefix is followed by a non-`/`
/// character (to avoid stripping a sibling directory that shares a prefix).
pub(crate) fn relativize_paths_in_text(text: &str, cwd: &Path) -> String {
    let cwd_str = format!("{}/", cwd.display());
    text.replace(&cwd_str, "")
}

pub(crate) fn format_tool_kind(kind: &crate::presentation::ToolKind) -> &str {
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

pub(crate) fn format_tool_phase(phase: &crate::presentation::ToolPhase) -> &str {
    match phase {
        crate::presentation::ToolPhase::Pending => "pending",
        crate::presentation::ToolPhase::PendingApproval => "pending approval",
        crate::presentation::ToolPhase::InProgress => "in progress",
        crate::presentation::ToolPhase::Completed => "completed",
        crate::presentation::ToolPhase::Failed => "failed",
    }
}

pub(crate) fn format_tool_header(snapshot: &crate::presentation::ToolSnapshot) -> String {
    format!(
        "Tool [{}]: {} ({})",
        format_tool_phase(&snapshot.phase),
        snapshot.title,
        format_tool_kind(&snapshot.kind)
    )
}

/// Semantic header for Create/Edit/Delete/Move tool snapshots.
/// Returns verb-based header like "Adding path", "Edited path", "Deleted path".
pub(crate) fn format_edit_tool_header(snapshot: &crate::presentation::ToolSnapshot) -> String {
    let (verb_active, verb_past, verb_failed, prefix) = match &snapshot.kind {
        crate::presentation::ToolKind::Create => ("Adding", "Added", "Add failed:", "Add "),
        crate::presentation::ToolKind::Edit => ("Editing", "Edited", "Edit failed:", "Edit "),
        crate::presentation::ToolKind::Delete => {
            ("Deleting", "Deleted", "Delete failed:", "Delete ")
        }
        crate::presentation::ToolKind::Move => ("Moving", "Moved", "Move failed:", "Move "),
        _ => return format_tool_header(snapshot),
    };

    let path = snapshot
        .locations
        .first()
        .map(|loc| loc.path.display().to_string())
        .unwrap_or_else(|| {
            // Try the kind-specific prefix first, then fall back to "Edit " since
            // some agents (Codex) always send title "Edit /path" regardless of the
            // actual operation type (which we refine via rawInput.changes).
            snapshot
                .title
                .strip_prefix(prefix)
                .or_else(|| snapshot.title.strip_prefix("Edit "))
                .unwrap_or(&snapshot.title)
                .to_string()
        });

    let verb = match &snapshot.phase {
        crate::presentation::ToolPhase::Failed => verb_failed,
        crate::presentation::ToolPhase::Completed => verb_past,
        _ => verb_active,
    };
    format!("{verb} {path}")
}

pub(crate) fn is_exploring_snapshot(snapshot: &crate::presentation::ToolSnapshot) -> bool {
    matches!(
        snapshot.kind,
        crate::presentation::ToolKind::Read | crate::presentation::ToolKind::Search
    ) || matches!(
        snapshot.invocation,
        Some(crate::presentation::Invocation::ListFiles { .. })
    )
}

pub(crate) fn format_invocation(
    invocation: &Option<crate::presentation::Invocation>,
) -> Option<String> {
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

/// Returns true when the formatted invocation string is redundant given the
/// snapshot title. For example, `Read: /repo/README.md` is redundant when the
/// title is `Read /repo/README.md`.
pub(crate) fn is_invocation_redundant(invocation_text: &str, title: &str) -> bool {
    // "Read: /repo/README.md" vs title "Read /repo/README.md"
    // "Command: ls -la" vs title "ls -la"
    // Strip the label prefix (everything before and including ": ")
    let payload = invocation_text
        .find(": ")
        .map(|idx| &invocation_text[idx + 2..])
        .unwrap_or(invocation_text);
    title.contains(payload)
}

pub(crate) fn format_artifacts(artifacts: &[crate::presentation::Artifact]) -> Vec<String> {
    artifacts
        .iter()
        .filter_map(|artifact| match artifact {
            crate::presentation::Artifact::Diff(_) => None,
            crate::presentation::Artifact::Text { text } if text.is_empty() => None,
            crate::presentation::Artifact::Text { text } => {
                let cleaned = strip_code_fences(text);
                if cleaned.is_empty() {
                    return None;
                }
                Some(cleaned)
            }
        })
        .collect()
}

pub(crate) fn strip_code_fences(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() >= 2 && lines[0].starts_with("```") && lines[lines.len() - 1].trim() == "```" {
        lines[1..lines.len() - 1].join("\n")
    } else {
        text.to_string()
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
