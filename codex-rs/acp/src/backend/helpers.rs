use super::*;

/// Get a human-readable name for an Op variant.
pub(crate) fn get_op_name(op: &Op) -> &'static str {
    match op {
        Op::Interrupt => "Interrupt",
        Op::UserInput { .. } => "UserInput",
        Op::UserTurn { .. } => "UserTurn",
        Op::OverrideTurnContext { .. } => "OverrideTurnContext",
        Op::ExecApproval { .. } => "ExecApproval",
        Op::PatchApproval { .. } => "PatchApproval",
        Op::ResolveElicitation { .. } => "ResolveElicitation",
        Op::AddToHistory { .. } => "AddToHistory",
        Op::GetHistoryEntryRequest { .. } => "GetHistoryEntryRequest",
        Op::SearchHistoryRequest { .. } => "SearchHistoryRequest",
        Op::ListCustomPrompts => "ListCustomPrompts",
        Op::Compact => "Compact",
        Op::Undo => "Undo",
        Op::UndoList => "UndoList",
        Op::UndoTo { .. } => "UndoTo",
        Op::Shutdown => "Shutdown",
        Op::RunUserShellCommand { .. } => "RunUserShellCommand",
        _ => "Unknown",
    }
}

/// Accumulated tool metadata captured from ACP permission requests before the
/// eventual `ToolCallUpdate(completed)` arrives.
#[derive(Default)]
pub(crate) struct AccumulatedToolCall {
    pub title: Option<String>,
    pub kind: Option<acp::ToolKind>,
    pub raw_input: Option<serde_json::Value>,
}
