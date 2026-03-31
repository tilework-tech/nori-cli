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

/// Semantic header for Edit/Delete/Move tool snapshots.
/// Returns verb-based header like "Editing path", "Edit failed: path", "Deleted path".
pub(crate) fn format_edit_tool_header(snapshot: &nori_protocol::ToolSnapshot) -> String {
    let (verb_active, verb_past, verb_failed, prefix) = match &snapshot.kind {
        nori_protocol::ToolKind::Edit => ("Editing", "Edited", "Edit failed:", "Edit "),
        nori_protocol::ToolKind::Delete => ("Deleting", "Deleted", "Delete failed:", "Delete "),
        nori_protocol::ToolKind::Move => ("Moving", "Moved", "Move failed:", "Move "),
        _ => return format_tool_header(snapshot),
    };

    let path = snapshot
        .locations
        .first()
        .map(|loc| loc.path.display().to_string())
        .unwrap_or_else(|| {
            snapshot
                .title
                .strip_prefix(prefix)
                .unwrap_or(&snapshot.title)
                .to_string()
        });

    let verb = match &snapshot.phase {
        nori_protocol::ToolPhase::Failed => verb_failed,
        nori_protocol::ToolPhase::Completed => verb_past,
        _ => verb_active,
    };
    format!("{verb} {path}")
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

/// Sanitize a tool title for display: strip Gemini-style `[current working directory ...]`
/// bracket metadata and trailing `(description text)` parenthetical, then relativize paths.
pub(crate) fn sanitize_tool_title(title: &str, cwd: &Path) -> String {
    // Step 1: strip [current working directory ...] bracket pattern
    let mut result = title.to_string();
    if let Some(bracket_start) = result.find("[current working directory")
        && let Some(bracket_end) = result[bracket_start..].find(']')
    {
        result = format!(
            "{}{}",
            &result[..bracket_start],
            &result[bracket_start + bracket_end + 1..]
        );
    }

    // Step 2: strip trailing (description text) parenthetical
    let trimmed = result.trim_end();
    if trimmed.ends_with(')')
        && let Some(paren_start) = trimmed.rfind('(')
    {
        result = trimmed[..paren_start].to_string();
    }

    // Step 3: trim and relativize
    let result = result.trim().to_string();
    relativize_paths_in_text(&result, cwd)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn sanitize_gemini_title_strips_bracket_and_paren() {
        let title = "Read README.md [current working directory /home/user/project] (Read the contents of README.md)";
        let cwd = PathBuf::from("/home/user/project");
        let result = sanitize_tool_title(title, &cwd);
        assert_eq!(result, "Read README.md");
    }

    #[test]
    fn sanitize_title_with_only_brackets() {
        let title = "Search pattern [current working directory /home/user/project]";
        let cwd = PathBuf::from("/home/user/project");
        let result = sanitize_tool_title(title, &cwd);
        assert_eq!(result, "Search pattern");
    }

    #[test]
    fn sanitize_title_with_only_trailing_paren() {
        let title = "ListFiles src (List all files in src directory)";
        let cwd = PathBuf::from("/tmp");
        let result = sanitize_tool_title(title, &cwd);
        assert_eq!(result, "ListFiles src");
    }

    #[test]
    fn sanitize_clean_title_passes_through() {
        let title = "Read README.md";
        let cwd = PathBuf::from("/tmp");
        let result = sanitize_tool_title(title, &cwd);
        assert_eq!(result, "Read README.md");
    }

    #[test]
    fn sanitize_title_relativizes_absolute_paths() {
        let title = "Read /home/user/project/src/main.rs";
        let cwd = PathBuf::from("/home/user/project");
        let result = sanitize_tool_title(title, &cwd);
        assert_eq!(result, "Read src/main.rs");
    }

    #[test]
    fn sanitize_empty_title() {
        let result = sanitize_tool_title("", &PathBuf::from("/tmp"));
        assert_eq!(result, "");
    }
}
