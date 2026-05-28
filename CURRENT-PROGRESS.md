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
16. Rehydrated ACP backend goal state from replayed goal events during resume setup. Active
    goals resume elapsed-time accounting from their last `updated_at` timestamp; a later
    replayed clear removes the backend goal state.
17. Added ACP prompt-context injection for active session goals. Before user input is sent to
    the ACP agent, the backend now prepends a structured `<goal_context>` block with the goal
    status, objective, elapsed active time, and token count, while preserving compact-summary
    ordering when a summary is pending.
18. Added goal token accounting from ACP usage updates. The backend keeps a usage baseline
    when a goal is created, updates goal token totals as `UsageUpdate` client events arrive,
    emits refreshed goal snapshots, and rebuilds the baseline from replayed usage/goal events
    when resuming a session.
19. Compared the current ACP goal implementation against upstream Codex goal behavior after
    verification. Remaining parity gaps are intentional blockers for the next design slice:
    Codex confirms before replacing an unfinished goal, prompts on resume for paused/blocked
    goals, exposes model-facing `create_goal`/`update_goal` tools, and can automatically
    continue active goals when the runtime goes idle. The current Nori slice stores,
    rehydrates, renders, and injects goal context, but does not yet auto-submit continuation
    turns or expose structured goal tools to ACP agents.
20. Added the first ACP-native automatic continuation slice. After a visible user prompt
    completes with an active goal and the ACP runtime is idle with no queued user work, the
    backend now submits one hidden `GoalContinuation` prompt to the same ACP session. The
    hidden prompt is omitted from visible queue text and user transcript entries, while the
    agent response still renders and records like normal assistant work. This intentionally
    does not recurse after continuation turns; deeper Codex-style autonomous loops and
    structured agent goal tools remain follow-on parity work from note 19.
21. Matched another Codex `/goal` UX affordance in the Nori TUI: submitting a new objective
    while an unfinished goal is cached now opens a replacement confirmation picker instead
    of immediately overwriting the active thread goal. Completed goals remain terminal and
    can be replaced directly without confirmation.
22. Fixed a stale pending-edit edge case in `/goal edit`. When the TUI requests a backend goal
    snapshot for editing and the backend replies that no goal exists, the pending edit request
    is now cleared, so a later unrelated `ThreadGoalUpdated` event does not unexpectedly
    replace the user's composer contents with `/goal <later objective>`.
23. Added resume notices for replayed paused, blocked, and usage-limited goals. After ACP resume sends deferred
    replay events, the backend now appends a non-persisted session info notice when the restored
    goal is stopped but resumable, pointing the user at `/goal resume`, `/goal edit`, and `/goal
    clear` without recording duplicate resume-only messages into future transcripts.
