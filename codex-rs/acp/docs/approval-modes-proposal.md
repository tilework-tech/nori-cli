# Proposal: Breaking Out ACP Approval Modes

**Issue:** [#367](https://github.com/tilework-tech/nori-cli/issues/367)
**Branch:** `claude/acp-approval-modes-research-XlXSw`
**Date:** 2026-03-18

## Problem Statement

The current `/approvals` popup offers three presets that conflate two independent axes (approval policy + sandbox policy) into opaque bundles:

| Current Preset | AskForApproval | SandboxPolicy | User Perception |
|---|---|---|---|
| Read Only | OnRequest | ReadOnly | "Asks for everything" |
| Agent | OnRequest | WorkspaceWrite | "Agent decides" (but *what* does it decide?) |
| Full Access | Never | DangerFullAccess | "Approve everything" |

The "Agent" mode is unclear. Users don't know that:
- The Claude agent auto-approves whitelisted commands in settings.json and asks for other shell commands
- File edits are initially prompted but eventually auto-approved after several approvals
- This behavior is specific to the Claude agent SDK -- other ACP agents (Codex, Gemini) behave differently

Per issue #367, users want a mode between "Agent decides" and "Full Access" -- specifically, one that pre-approves file edits in the current directory tree but still prompts for shell commands.

## Research Findings

### Layer 1: ACP Protocol (the spec)

The ACP protocol is deliberately unopinionated about approval policies:

- `session/request_permission` is called by the **Agent** on the **Client**
- The protocol says: *"Clients MAY automatically allow or reject permission requests according to user settings"*
- Permission options have 4 kinds: `AllowOnce`, `AllowAlways`, `RejectOnce`, `RejectAlways`
- There is **no** protocol-level concept of approval modes or sandbox policies
- Session Modes (agent-defined) can affect whether the agent requests permission, but semantics are agent-specific

**Implication:** Nori's approval modes are purely a client-side concept. The client can auto-respond to any `request_permission` call however it wants.

### Layer 2: Claude Agent ACP Adapter

The Claude agent has internal permission modes in the SDK:

| SDK Mode | Behavior |
|---|---|
| `default` | Prompts for dangerous ops; auto-approves safe reads |
| `acceptEdits` | Auto-approves file edits; still prompts for shell commands |
| `dontAsk` | Denies anything not pre-approved |
| `plan` | No tool execution at all |
| `bypassPermissions` | Auto-approves everything |

The adapter itself is a passthrough -- when the SDK decides something needs approval, it calls `requestPermission` with three options (Allow Always / Allow / Reject). The SDK decides *when* to ask based on its mode.

Key insight: The `acceptEdits` mode is exactly what users in #367 are requesting, but it's buried inside the Claude SDK with no way for Nori's client to activate it.

### Layer 3: Codex ACP Adapter

The Codex adapter delegates approval decisions to `codex-core`:

- `AskForApproval::Never` or `OnFailure` → no approval needed
- `AskForApproval::OnRequest` → needs approval unless sandbox is `DangerFullAccess`
- `AskForApproval::UnlessTrusted` → always needs approval

codex-core has richer permission options including exec-policy amendments ("don't ask again for commands starting with X") and network policy amendments.

### Layer 4: Nori ACP Backend (Client Side)

The `run_approval_handler` in `spawn_and_relay.rs` is the single decision point:

- If `AskForApproval::Never` → auto-approve via `ReviewDecision::Approved`
- Otherwise → forward to TUI for user decision

This is a binary gate: either auto-approve everything, or show everything to the user. There is no logic to selectively auto-approve based on operation type (exec vs patch).

## Proposed New Approval Modes

Replace the current 3 presets with 4, adding "Allow Edits" between "Agent" and "Full Access":

| Preset ID | Label | Description | Behavior |
|---|---|---|---|
| `read-only` | **Read Only** | Requires approval to edit files and run commands | Agent can read; all writes and commands need user approval |
| `agent` | **Agent** | Agent decides what needs approval (default) | Agent's internal policy decides; Nori forwards all `request_permission` calls to the user |
| `allow-edits` | **Allow Edits** | Pre-approves file edits in the workspace | Auto-approve patch operations in cwd tree; prompt for shell commands |
| `full-access` | **Full Access** | Approve everything (exercise caution) | Auto-approve all operations; no sandbox restrictions |

### Detailed Behavior of "Allow Edits"

When a `request_permission` arrives:
1. Classify it as **Exec** or **Patch** (this already happens in `client_delegate.rs`)
2. If **Patch** and all changed files are within the cwd tree → **auto-approve**
3. If **Patch** but files are outside cwd → **prompt user** (safety boundary)
4. If **Exec** → **prompt user** (shell commands still require approval)

This gives the user the "I trust the agent to edit my project files, but I want to see what commands it runs" experience.

## Implementation Plan

### Changes Required

#### 1. New `AskForApproval` variant (or separate axis)

**Option A: Add variant to `AskForApproval`**
Add `AllowEdits` variant to `AskForApproval` in `protocol/src/protocol/mod.rs`. This is the simplest approach -- the approval handler already switches on this enum.

```rust
pub enum AskForApproval {
    UnlessTrusted,
    OnFailure,
    OnRequest,     // "Agent" mode
    AllowEdits,    // NEW: auto-approve patches in workspace
    Never,         // "Full Access"
}
```

**Option B: Separate the two axes explicitly**
Instead of overloading `AskForApproval`, introduce a new `PatchApprovalPolicy` axis:
```rust
pub enum PatchApprovalPolicy {
    AlwaysAsk,           // same as today
    AutoApproveWorkspace, // auto-approve patches in cwd
    AutoApproveAll,       // auto-approve all patches
}
```

**Recommendation:** Option A is simpler and matches the existing pattern. The approval handler already inspects `AskForApproval` as a single value. A separate axis adds complexity for a single feature.

#### 2. Update `run_approval_handler` in `acp/src/backend/spawn_and_relay.rs`

The core logic change. Currently the handler only checks for `Never`:

```rust
// BEFORE
if current_policy == AskForApproval::Never {
    let _ = request.response_tx.send(ReviewDecision::Approved);
    continue;
}

// AFTER
if current_policy == AskForApproval::Never {
    let _ = request.response_tx.send(ReviewDecision::Approved);
    continue;
}

if current_policy == AskForApproval::AllowEdits {
    if let ApprovalEventType::Patch(ref patch_event) = request.event {
        if all_changes_in_workspace(&patch_event.changes, &cwd) {
            let _ = request.response_tx.send(ReviewDecision::Approved);
            continue;
        }
    }
    // Exec requests and out-of-workspace patches fall through to TUI
}
```

The `all_changes_in_workspace` helper checks whether every file path in the patch's `changes` HashMap starts with `cwd`.

#### 3. New `ApprovalPreset` in `common/src/approval_presets.rs`

Add the fourth preset:

```rust
ApprovalPreset {
    id: "allow-edits",
    label: "Allow Edits",
    description: "Pre-approves file edits in the workspace. Commands still require approval.",
    approval: AskForApproval::AllowEdits,
    sandbox: SandboxPolicy::new_workspace_write_policy(),
},
```

Insert between `"auto"` and `"full-access"` in the `builtin_approval_presets()` vec.

#### 4. Update `approval_mode_label` in `common/src/approval_presets.rs`

The existing label matching will work automatically since it matches against presets. No code change needed beyond adding the preset.

#### 5. Update `ApprovalPolicy` config enum in `acp/src/config/types/mod.rs`

Add `AllowEdits` variant for TOML config support:

```rust
pub enum ApprovalPolicy {
    Always,
    OnRequest,
    AllowEdits,  // NEW
    Never,
}
```

#### 6. Update `ApprovalModeCliArg` in `common/src/approval_mode_cli_arg.rs`

Add CLI flag support:

```rust
pub enum ApprovalModeCliArg {
    Untrusted,
    OnFailure,
    OnRequest,
    AllowEdits,  // NEW
    FullAuto,
}
```

With the mapping: `ApprovalModeCliArg::AllowEdits => AskForApproval::AllowEdits`

#### 7. Update serialization

Ensure the new variant serializes correctly:
- serde: `"allow-edits"` (kebab-case, matches existing pattern)
- strum: `"allow-edits"`
- clap ValueEnum: `--approval-mode allow-edits`

#### 8. Agent-side integration (optional, future)

For the Claude agent specifically, the ideal integration would be:
- When client selects "Allow Edits", send a session config option or mode hint to the agent
- The Claude SDK's `acceptEdits` mode would then avoid even *requesting* permission for edits
- This reduces round-trips but is not required for correctness -- the client-side auto-approve in step 2 handles it regardless of agent behavior

This is an optimization and can be done in a follow-up.

### Files to Modify

| File | Change |
|---|---|
| `protocol/src/protocol/mod.rs` | Add `AllowEdits` to `AskForApproval` |
| `common/src/approval_presets.rs` | Add "Allow Edits" preset |
| `common/src/approval_mode_cli_arg.rs` | Add `AllowEdits` variant + mapping |
| `acp/src/config/types/mod.rs` | Add `AllowEdits` to `ApprovalPolicy` |
| `acp/src/backend/spawn_and_relay.rs` | Add `AllowEdits` logic to `run_approval_handler` |
| `acp/src/config/loader.rs` | Map `ApprovalPolicy::AllowEdits` to `AskForApproval::AllowEdits` |
| `tui/src/chatwidget/approvals.rs` | No change needed (reads from `builtin_approval_presets()`) |
| `tui/src/bottom_pane/approval_overlay.rs` | No change needed |
| TUI snapshot tests | Update to reflect 4th option in popup |
| `acp/src/backend/tests/` | Add test for AllowEdits auto-approve behavior |

### Tests to Add

1. **Unit test:** `all_changes_in_workspace` correctly identifies in-workspace vs out-of-workspace patches
2. **Integration test:** `run_approval_handler` with `AllowEdits` policy auto-approves workspace patches
3. **Integration test:** `run_approval_handler` with `AllowEdits` policy escalates out-of-workspace patches
4. **Integration test:** `run_approval_handler` with `AllowEdits` policy escalates exec requests
5. **Snapshot test:** Updated `/approvals` popup showing 4 presets
6. **Config test:** `AllowEdits` deserializes from TOML correctly

## Alternatives Considered

### Alternative 1: Two-axis selector (Approval + Sandbox independently)

Instead of presets, let users independently choose:
- Approval: Always Ask / Agent Decides / Allow Edits / Never Ask
- Sandbox: Read Only / Workspace Write / Full Access

**Rejected because:** The combinatorial complexity (12 combinations) is confusing. Many combinations don't make sense (e.g., "Never Ask" + "Read Only"). Presets are simpler and cover the real use cases.

### Alternative 2: Only "Agent" and "Full Access" with a toggle for edits

Keep 3 presets but add a checkbox "Auto-approve file edits" to the Agent preset.

**Rejected because:** This makes the UI more complex (popup with nested options) and doesn't generalize well to future approval refinements.

### Alternative 3: Tell the agent to use `acceptEdits` mode via ACP session config

Send a config option to the Claude agent telling it to switch to `acceptEdits` mode, so it never even requests permission for edits.

**Rejected as primary approach because:** This only works with the Claude agent. Codex and Gemini agents don't have this concept. The client-side approach works with any agent. However, this is worth doing as a follow-up optimization for Claude specifically.

## Migration / Backwards Compatibility

- No breaking changes. The new variant is additive.
- Existing configs with `approval_policy = "on-request"` continue to work unchanged.
- The default remains "Agent" (`OnRequest`).
- Old TUI versions that don't know about `AllowEdits` will fail to deserialize it, but this only matters for protocol wire format -- and we control both sides.
