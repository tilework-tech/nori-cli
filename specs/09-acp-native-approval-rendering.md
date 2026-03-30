# Spec 09: ACP-Native Approval Rendering

## Summary

Replace the legacy exec/patch approval bridge with an ACP-native approval model. Currently, all ACP approval requests are force-fit into either `ApprovalRequest::Exec` (with a synthesized command `Vec<String>`) or `ApprovalRequest::ApplyPatch` (with converted `FileChange` maps). This produces wrong approval overlay text, wrong fullscreen preview content, and wrong history cells — especially for non-execute, non-edit ACP tool approvals.

## Current Behavior

### Approval overlay

`approval_request_from_client_event` (event_handlers.rs:1638-1665) converts every `nori_protocol::ApprovalRequest` into one of two legacy variants:

- **Edit/Delete/Move with parseable file changes** → `ApprovalRequest::ApplyPatch` with `file_changes_from_snapshot()` bridge
- **Everything else** → `ApprovalRequest::Exec` with `approval_command_from_snapshot()` synthesizing a fake `Vec<String>` command

`approval_command_from_snapshot` (event_handlers.rs:1697-1717) produces:
- `Invocation::Command` → `["bash", "-lc", command]` — reasonable for execute tools
- `Invocation::Tool` → `["{tool_name} {compact_json(input)}"]` — raw JSON dumped as a command string
- Everything else → `[generic_execute_command_text(snapshot)]` — title with raw_input appended

The approval overlay (approval_overlay.rs:338-412) then renders these as:
- `Exec` → "Would you like to run the following command?" + bash-highlighted command — wrong for non-execute ACP tools
- `ApplyPatch` → "Would you like to make the following edits?" + DiffSummary — correct for edits, but bypasses ACP title/kind

### Approval decision history

`handle_exec_decision` (approval_overlay.rs:194-201) calls `history_cell::new_approval_decision_cell(command, decision)` which renders:
- `"✔ You approved Nori to run {exec_snippet} this time"`
- `"✔ You approved Nori to run {exec_snippet} every time this session"`
- `"✗ You did not approve Nori to run {exec_snippet}"`

For ACP approvals that are not shell commands, this produces nonsense like:
- `"✔ You approved Nori to run tool_name {"input": "..."} this time"`

Patch decisions (approval_overlay.rs:203-208) emit `Op::PatchApproval` but produce no history cell at all.

### Fullscreen preview

`FullScreenApprovalRequest` (app/event_handling.rs:490-525) renders:
- `Exec` → bash-highlighted command text in "E X E C" overlay
- `ApplyPatch` → DiffSummary in "P A T C H" overlay
- No ACP-tool-native presentation

### What the wire data actually provides

`nori_protocol::ApprovalRequest` (nori-protocol/src/lib.rs:97-103):
```rust
pub struct ApprovalRequest {
    pub call_id: String,
    pub title: String,         // Human-readable tool title
    pub kind: ToolKind,        // edit, execute, read, etc.
    pub options: Vec<ApprovalOption>,  // AllowAlways, AllowOnce, RejectOnce
    pub subject: ApprovalSubject,     // Contains the full ToolSnapshot
}
```

`ApprovalOption` (nori-protocol/src/lib.rs:113-126):
```rust
pub struct ApprovalOption {
    pub option_id: String,
    pub name: String,          // Human-readable option label
    pub kind: ApprovalOptionKind,  // AllowAlways, AllowOnce, RejectOnce, Other
}
```

The protocol already provides the exact approval title, typed options, and the full tool snapshot. None of this is used by the current rendering path.

## Required Changes

### 1. New `ApprovalRequest::AcpTool` variant

Add a third variant to the TUI's `ApprovalRequest` enum (approval_overlay.rs:40-58):

```rust
AcpTool {
    call_id: String,
    title: String,
    kind: nori_protocol::ToolKind,
    options: Vec<nori_protocol::ApprovalOption>,
    snapshot: nori_protocol::ToolSnapshot,
}
```

### 2. New conversion in `approval_request_from_client_event`

Replace the current two-path conversion (event_handlers.rs:1638-1665) with:

- **Edit/Delete/Move with file changes** → still produce `ApplyPatch` (the DiffSummary overlay is good)
- **Execute with `Invocation::Command`** → still produce `Exec` (the bash-highlighted overlay is good)
- **Everything else** → produce `AcpTool` carrying the native protocol fields

### 3. New overlay renderer for `AcpTool`

In `ApprovalRequestState::from` (approval_overlay.rs:338-412), add a branch for `AcpTool`:

- Title question: `"Would you like to allow {kind} tool: {title}?"` (not "run the following command")
- Header: render the tool snapshot's invocation detail and diff artifacts (reuse `ClientToolCell`'s generic renderer or format_invocation + format_artifacts + diff rendering)
- Options: map `ApprovalOption` entries to `ApprovalOption` UI items, using `option.name` as the label and `option.kind` to determine the `ApprovalDecision`

### 4. New `ApprovalVariant::AcpTool` and decision handler

Add a matching `ApprovalVariant::AcpTool` (approval_overlay.rs:436-449) and a `handle_acp_tool_decision` method that:

- Maps `ApprovalOptionKind::AllowOnce` → `ReviewDecision::Approved`
- Maps `ApprovalOptionKind::AllowAlways` → `ReviewDecision::ApprovedForSession`
- Maps `ApprovalOptionKind::RejectOnce` → `ReviewDecision::Denied`
- Sends the appropriate `Op::ExecApproval` or `Op::PatchApproval` (these backend ops haven't changed)
- Inserts a new ACP-native history cell (see below)

### 5. New approval decision history cell

Replace `new_approval_decision_cell(command, decision)` (history_cell/mod.rs:386-450) usage with a new function for ACP approvals:

```
new_acp_approval_decision_cell(title, kind, option_name, decision)
```

Rendering:
- Approved once: `"✔ You approved {kind}: {title} this time"`
- Approved always: `"✔ You approved {kind}: {title} for this session"`
- Denied: `"✗ You denied {kind}: {title}"`
- Aborted: `"✗ You canceled {kind}: {title}"`

### 6. Fullscreen preview for `AcpTool`

In `FullScreenApprovalRequest` handling (app/event_handling.rs:490-525), add a branch that renders the tool snapshot using `ClientToolCell`'s rendering in a "T O O L" overlay, showing the full invocation detail and any diff artifacts.

### 7. Notification for AcpTool approvals

In `handle_client_approval_request` (event_handlers.rs:1230-1252), add a notification path for the new `AcpTool` variant that includes the tool title and kind.

## Scope

- Add `ApprovalRequest::AcpTool` variant and corresponding `ApprovalVariant`
- Update `approval_request_from_client_event` to produce `AcpTool` for non-exec, non-patch-edit approvals
- Add overlay rendering, decision handling, history cell, and fullscreen preview for the new variant
- Update `on_ctrl_c` and queue handling to cover the new variant
- Preserve existing `Exec` and `ApplyPatch` paths unchanged (they work well for their cases)
- Add tests: overlay render with AcpTool, decision history cell text, Ctrl+C abort for AcpTool

## Non-Goals

- This spec does not change the backend approval protocol (`Op::ExecApproval`, `Op::PatchApproval`)
- This spec does not remove the existing Exec or ApplyPatch approval paths
- This spec does not change MCP elicitation rendering
