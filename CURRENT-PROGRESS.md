Cuts:

1. Remove the old upstream onboarding stack in onboarding_screen.rs (/home/clifford/
   Documents/source/nori/cli/codex-rs/tui/src/onboarding/onboarding_screen.rs). It is
   explicitly “kept for reference,” and lib.rs (/home/clifford/Documents/source/nori/cli/
   codex-rs/tui/src/lib.rs) only runs Nori onboarding now.
2. Trim the compatibility-only login_status and auth_manager fields from nori/onboarding/
   onboarding_screen.rs (/home/clifford/Documents/source/nori/cli/codex-rs/tui/src/nori/
   onboarding/onboarding_screen.rs). The file itself says they are unused and only kept for
   API compatibility.
3. Delete the unused upstream update UI in update_prompt.rs (/home/clifford/Documents/
   source/nori/cli/codex-rs/tui/src/update_prompt.rs) and its paired update_action.rs (/
   home/clifford/Documents/source/nori/cli/codex-rs/tui/src/update_action.rs). lib.rs (/
   home/clifford/Documents/source/nori/cli/codex-rs/tui/src/lib.rs) re-exports the Nori
   versions instead.
4. Remove the model migration prompt subsystem in model_migration.rs (/home/clifford/
   Documents/source/nori/cli/codex-rs/tui/src/model_migration.rs) and its hooks in app/
   mod.rs (/home/clifford/Documents/source/nori/cli/codex-rs/tui/src/app/mod.rs). This is
   legacy Codex/GPT migration baggage, not ACP.
5. Remove legacy transcript parsers for Claude/Codex/Gemini in session_parser.rs (/home/
   clifford/Documents/source/nori/cli/codex-rs/acp/src/session_parser.rs). They are another
   non-ACP compatibility layer.
6. Drop legacy transcript entry variants ToolCall, ToolResult, and PatchApply from
   transcript/types.rs (/home/clifford/Documents/source/nori/cli/codex-rs/acp/src/
   transcript/types.rs). acp/docs.md (/home/clifford/Documents/source/nori/cli/codex-rs/acp/
   docs.md) says they remain only for legacy read compatibility.
7. Drop transcript attachments from transcript/types.rs (/home/clifford/Documents/source/
   nori/cli/codex-rs/acp/src/transcript/types.rs) and transcript/recorder.rs (/home/
   clifford/Documents/source/nori/cli/codex-rs/acp/src/transcript/recorder.rs). backend/
   user_input.rs (/home/clifford/Documents/source/nori/cli/codex-rs/acp/src/backend/
   user_input.rs) records every user message with vec![].
8. Remove the debug-only /rollout surface in slash_command.rs (/home/clifford/Documents/
   source/nori/cli/codex-rs/tui/src/slash_command.rs) and chatwidget/key_handling.rs (/
   home/clifford/Documents/source/nori/cli/codex-rs/tui/src/chatwidget/key_handling.rs). It
   exposes an old Codex concept, not ACP behavior.
9. Remove the debug-only /test-approval path in slash_command.rs (/home/clifford/Documents/
   source/nori/cli/codex-rs/tui/src/slash_command.rs) and chatwidget/key_handling.rs (/home/
   clifford/Documents/source/nori/cli/codex-rs/tui/src/chatwidget/key_handling.rs). Good
   cleanup, zero protocol cost.

Goal command progress:

10. Started `/goal` implementation from the current `feat/goal` branch, which was still at
    `main` with no committed goal work. Chosen architecture: keep the goal state in the
    ACP backend because it already owns the long-lived ACP session, and have the TUI send
    typed goal ops instead of forwarding `/goal` text to arbitrary agents.
11. Added the first shared protocol contract for thread goals: statuses, typed get/set/clear
    ops, and objective validation. Token budgets remain intentionally out of scope for the
    first Nori implementation, matching the goal-context instruction.
12. Added normalized ACP-client events for goal updated/cleared notifications in
    `nori-protocol`. This keeps the ACP backend as the goal-state owner while giving the
    TUI an agent-independent event shape to render.
13. Added an in-memory ACP-session goal state machine and wired `ThreadGoalGet`,
    `ThreadGoalSet`, and `ThreadGoalClear` through `AcpBackend::submit`. Goal time is
    accumulated only while the status is active; paused/blocked/limited/complete states do
    not accrue elapsed time until resumed.
14. Wired the TUI `/goal` command to typed ACP goal ops. Bare `/goal` requests the current
    goal, `/goal <objective>` creates/replaces the objective as active, `/goal pause`,
    `/goal resume`, and `/goal clear` map to direct mutations, and `/goal edit` preloads
    the current objective into the composer when the TUI has a goal snapshot.
15. Preserved goal update/clear client events through transcript replay conversion. This
    restores the latest TUI-visible goal snapshot on resumed sessions, but the ACP backend
    still needs a stronger state rehydration path before automatic continuation can rely on
    a replayed goal without a fresh mutation.
