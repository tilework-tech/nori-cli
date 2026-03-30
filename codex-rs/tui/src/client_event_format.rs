use std::path::Path;

/// Replace occurrences of the given `cwd` prefix (with trailing `/`) in `text`
/// with an empty string, effectively turning absolute paths under `cwd` into
/// relative ones. Only replaces when the prefix is followed by a non-`/`
/// character (to avoid stripping a sibling directory that shares a prefix).
pub(crate) fn relativize_paths_in_text(text: &str, cwd: &Path) -> String {
    let cwd_str = format!("{}/", cwd.display());
    text.replace(&cwd_str, "")
}

pub(crate) fn format_tool_kind(kind: &nori_protocol::ToolKind) -> &str {
    match kind {
        nori_protocol::ToolKind::Read => "read",
        nori_protocol::ToolKind::Search => "search",
        nori_protocol::ToolKind::Execute => "execute",
        nori_protocol::ToolKind::Edit => "edit",
        nori_protocol::ToolKind::Delete => "delete",
        nori_protocol::ToolKind::Move => "move",
        nori_protocol::ToolKind::Fetch => "fetch",
        nori_protocol::ToolKind::Think => "think",
        nori_protocol::ToolKind::Other(other) => other,
    }
}

pub(crate) fn format_tool_phase(phase: &nori_protocol::ToolPhase) -> &str {
    match phase {
        nori_protocol::ToolPhase::Pending => "pending",
        nori_protocol::ToolPhase::PendingApproval => "pending approval",
        nori_protocol::ToolPhase::InProgress => "in progress",
        nori_protocol::ToolPhase::Completed => "completed",
        nori_protocol::ToolPhase::Failed => "failed",
    }
}

pub(crate) fn format_tool_header(snapshot: &nori_protocol::ToolSnapshot) -> String {
    format!(
        "Tool [{}]: {} ({})",
        format_tool_phase(&snapshot.phase),
        snapshot.title,
        format_tool_kind(&snapshot.kind)
    )
}

pub(crate) fn is_exploring_snapshot(snapshot: &nori_protocol::ToolSnapshot) -> bool {
    matches!(
        snapshot.kind,
        nori_protocol::ToolKind::Read | nori_protocol::ToolKind::Search
    ) || matches!(
        snapshot.invocation,
        Some(nori_protocol::Invocation::ListFiles { .. })
    )
}

pub(crate) fn format_invocation(invocation: &Option<nori_protocol::Invocation>) -> Option<String> {
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

pub(crate) fn format_artifacts(artifacts: &[nori_protocol::Artifact]) -> Vec<String> {
    artifacts
        .iter()
        .filter_map(|artifact| match artifact {
            nori_protocol::Artifact::Diff(_) => None,
            nori_protocol::Artifact::Text { text } if text.is_empty() => None,
            nori_protocol::Artifact::Text { text } => {
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
