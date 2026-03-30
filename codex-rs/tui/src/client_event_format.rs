use std::collections::HashMap;
use std::path::PathBuf;

pub(crate) fn format_plan_status(status: &nori_protocol::PlanStatus) -> &'static str {
    match status {
        nori_protocol::PlanStatus::Pending => "pending",
        nori_protocol::PlanStatus::InProgress => "in_progress",
        nori_protocol::PlanStatus::Completed => "completed",
    }
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

pub(crate) fn format_approval_header(approval: &nori_protocol::ApprovalRequest) -> String {
    format!(
        "Approval requested: {} ({})",
        approval.title,
        format_tool_kind(&approval.kind)
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

pub(crate) fn format_artifacts(artifacts: &[nori_protocol::Artifact]) -> Vec<String> {
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

pub(crate) fn snapshot_file_changes(
    snapshot: &nori_protocol::ToolSnapshot,
) -> Option<HashMap<PathBuf, codex_core::protocol::FileChange>> {
    match &snapshot.invocation {
        Some(nori_protocol::Invocation::FileChanges { changes }) => {
            let file_changes = changes
                .iter()
                .map(|change| (change.path.clone(), file_change_from_nori_change(change)))
                .collect::<HashMap<_, _>>();
            if file_changes.is_empty() {
                None
            } else {
                Some(file_changes)
            }
        }
        Some(nori_protocol::Invocation::FileOperations { operations }) => {
            let file_changes = operations
                .iter()
                .map(file_change_from_nori_operation)
                .collect::<HashMap<_, _>>();
            if file_changes.is_empty() {
                None
            } else {
                Some(file_changes)
            }
        }
        _ => None,
    }
}

fn file_change_from_nori_operation(
    operation: &nori_protocol::FileOperation,
) -> (PathBuf, codex_core::protocol::FileChange) {
    match operation {
        nori_protocol::FileOperation::Create { path, new_text } => (
            path.clone(),
            codex_core::protocol::FileChange::Add {
                content: new_text.clone(),
            },
        ),
        nori_protocol::FileOperation::Update {
            path,
            old_text,
            new_text,
        } => (
            path.clone(),
            codex_core::protocol::FileChange::Update {
                unified_diff: diffy::create_patch(old_text, new_text).to_string(),
                move_path: None,
            },
        ),
        nori_protocol::FileOperation::Delete { path, old_text } => (
            path.clone(),
            codex_core::protocol::FileChange::Delete {
                content: old_text.clone().unwrap_or_default(),
            },
        ),
        nori_protocol::FileOperation::Move {
            from_path,
            to_path,
            old_text,
            new_text,
        } => {
            let old_text = old_text.clone().unwrap_or_default();
            let new_text = new_text.clone().unwrap_or_else(|| old_text.clone());
            (
                from_path.clone(),
                codex_core::protocol::FileChange::Update {
                    unified_diff: diffy::create_patch(&old_text, &new_text).to_string(),
                    move_path: Some(to_path.clone()),
                },
            )
        }
    }
}

fn file_change_from_nori_change(
    change: &nori_protocol::FileChange,
) -> codex_core::protocol::FileChange {
    match &change.old_text {
        None => codex_core::protocol::FileChange::Add {
            content: change.new_text.clone(),
        },
        Some(old_text) => codex_core::protocol::FileChange::Update {
            unified_diff: diffy::create_patch(old_text, &change.new_text).to_string(),
            move_path: None,
        },
    }
}
