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
24. Added backend-owned goal MCP tools for ACP agents that advertise HTTP MCP support. Nori now
    registers an in-process `nori-goal` MCP server over ACP MCP-over-ACP during new, resumed, and
    compaction-created sessions; agents can call `get_goal`, `create_goal`, and `update_goal`
    while the backend remains the goal-state authority and continues emitting transcript-backed
    `ThreadGoalUpdated` snapshots. Agents without HTTP MCP support still receive the existing
    prompt goal context and hidden continuation behavior without the structured tools.
25. Addressed review feedback on the goal MCP slice. Server-side ACP resume now rebuilds goal
    state from transcript-owned goal replay plus any agent load replay so non-goal ACP
    notifications cannot erase a restored goal. The local MCP bridge now has direct
    `_mcp/connect`/`_mcp/message` routing coverage and retains dynamic handler registrations only
    for the current advertised local MCP endpoint instead of leaking stale endpoints indefinitely.
26. Added the deeper autonomous continuation slice for ACP agents that can use goal tools. When
    an agent advertises HTTP MCP support, hidden `GoalContinuation` turns can now chain after
    prior continuation turns while the active goal remains open and the runtime is idle; agents
    without HTTP MCP support keep the previous single hidden continuation after a visible user
    turn so unsupported agents are not put into an unbounded loop they cannot stop.
27. Follow-up: disable or clearly mark `/goal` unavailable when the active ACP agent does not
    support HTTP MCP servers. The current slash popup has description overrides but no disabled
    row state, and pasted `/goal ...` is handled separately in `chatwidget/goal.rs`, so the
    correct small fix is probably both UI affordance and a backend/TUI command guard. This
    matters because prompt goal context can still work without MCP, but the main close-the-loop
    path depends on the agent having the `nori-goal` MCP tools so it can mark goals complete or
    blocked.
28. Fixed the quick goal visual/accounting issues from bugs 3, 4, and the scoped version of 5.
    The TUI now suppresses history cells for accounting-only `ThreadGoalUpdated` refreshes while
    still rendering explicit `/goal` status requests and objective/status changes. Goal summaries
    use compact SI token formatting and label the count as excluding subagents. Backend goal token
    usage now accumulates positive ACP usage deltas across context-window drops instead of
    mirroring the latest session usage value.
29. Verified the bug 3/4/5 slice end-to-end and pushed it to PR #491 as commit `846c27c1`.
    Local verification covered `cargo test -p nori-acp`, `cargo test -p nori-tui`,
    `cargo build --bin nori && cargo test -p tui-pty-e2e`, `just fmt`, scoped `just fix`,
    snapshot acceptance, and an isolated ElizACP TUI smoke test. GitHub checks for the PR passed
    afterward: `Linux checks` and `cargo-deny`.

Follow-up bug investigations - 2026-05-28:

### Bug 1: `nori-goal` MCP startup failure

- Finding: The existing `nori-rs/closing-loop/nori-goal-mcp-over-acp-bug-report.md`
  report is substantially correct. Nori advertises backend-owned goal tools as an
  ACP local MCP server by sending an HTTP MCP server entry whose URL is `acp:<uuid>`.
  Codex ACP treats that entry as a normal streamable HTTP MCP server and Codex's
  HTTP client rejects the `acp:` scheme before Nori's `_mcp/connect` bridge can run.
- Evidence: Goal MCP registration is guarded only by `mcp_capabilities.http` in
  `nori-rs/acp/src/backend/thread_goal_mcp.rs`; registration appends
  `McpServer::Http { url: "acp:<uuid>" }` in
  `nori-rs/acp/src/connection/sacp_connection.rs`; the intended local receiver is
  `_mcp/connect` in `nori-rs/acp/src/connection/local_mcp.rs`.
- Correction to the prior report: the built-in Nori Codex agent still launches
  `@zed-industries/codex-acp`, not `@agentclientprotocol/codex-acp`. Both appear to
  have the same shape mismatch: they advertise HTTP MCP but forward/load the
  `acp:` URL as a normal HTTP MCP config instead of using ACP `_mcp/connect`.
- Quickest estimate: 2-4 hours for a focused mitigation that disables local goal
  MCP advertisement for Codex ACP and gates continuation chaining on goal MCP
  being actually supported/registered, not raw HTTP MCP capability. A real
  loopback HTTP MCP server is a more complete but larger 1-2 day fix. Upstream
  Codex ACP support for ACP-local MCP is the cleanest architecture, but timing is
  outside this repo.
- Risks/unknowns: Need confirm which non-Codex ACP agents actually support `acp:`
  local MCP before changing behavior broadly. A Codex-specific deny path is safer
  than treating all HTTP MCP agents as broken.

### Bug 2: New `/goal <objective>` does not begin work immediately

- Finding: Confirmed. `/goal <objective>` is a state mutation only. The TUI
  intercepts the slash command, sends `Op::ThreadGoalSet`, and returns before any
  normal ACP `session/prompt` is submitted.
- Root cause: Automatic hidden goal continuation is currently triggered only after
  a completed visible user turn or prior continuation turn. Setting the goal does
  not enqueue a hidden `GoalContinuation` prompt, so work starts only after the
  user submits a second prompt.
- Evidence: `/goal` sends only `ThreadGoalSet` from
  `nori-rs/tui/src/chatwidget/goal.rs`; `handle_thread_goal_set` stores state and
  emits `ThreadGoalUpdated`; `maybe_submit_goal_continuation` is called from
  completed-turn handling in `nori-rs/acp/src/backend/session_runtime_driver.rs`.
- Quickest estimate: small, 1-2 hours with tests. More like 2-3 hours if the same
  change covers `/goal resume`, active-goal replacement while a request is in
  flight, and HTTP-MCP chain semantics.
- Likely fix: after a successful active `ThreadGoalSet`, enqueue the existing
  hidden `GoalContinuation` prompt when the runtime is idle and there is no queued
  user work. Extract the enqueue portion of `maybe_submit_goal_continuation` so it
  can be reused without requiring a `CompletedTurn`.

### Bug 3: Goal status prints constantly into history

- Finding: Confirmed. Nori emits a `ThreadGoalUpdated` event for every ACP usage
  update, and the TUI renders every `ThreadGoalUpdated` as a full history summary.
  That makes token/time refreshes look like repeated goal status messages.
- What Codex does: upstream Codex stores goal state and updates a compact footer
  status indicator such as `Pursuing goal (...)`. It does not append every backend
  goal update into chat history. History/info messages are mostly tied to explicit
  `/goal` actions such as set, status, pause, resume, or clear.
- Evidence: ACP usage updates are converted into `ThreadGoalUpdated` in
  `nori-rs/acp/src/backend/thread_goal.rs`; the TUI always calls
  `show_goal_summary` from `handle_thread_goal_updated` in
  `nori-rs/tui/src/chatwidget/goal.rs`. Codex's footer/status indicator path is in
  `other-repos/codex/codex-rs/tui/src/chatwidget/goal_status.rs` and
  `other-repos/codex/codex-rs/tui/src/bottom_pane/footer.rs`.
- Quickest estimate: small, 1-2 hours with focused TUI tests/snapshot updates.
  Matching the upstream footer/status model more fully is closer to 0.5-1 day.
- Likely fix: keep updating cached `current_goal` on every event, but only append
  a history summary when user-visible goal meaning changes or when the user
  explicitly asks for `/goal` status. Minimal safe heuristic: suppress summaries
  when previous and new goal have the same objective/status/created timestamp and
  only accounting fields changed.
- Risks/unknowns: The protocol currently does not carry provenance that says
  "this update is an explicit user-requested status response" versus "this update
  is backend sync." Without adding provenance, the first fix will be heuristic.

### Bug 4: `Tokens used` should pretty print

- Finding: Display-only bug. `nori-rs/tui/src/chatwidget/goal.rs` renders
  `Tokens used` with `goal.tokens_used.to_string()`.
- Existing helper: `codex_protocol::num_format::format_si_suffix` already exists
  and is used elsewhere for compact token counts, so the cheapest fix does not
  need a new formatter or dependency.
- Format ambiguity: `195043 -> 195K` matches the existing Nori helper and upstream
  Codex compact formatting. The example `32492004 -> 32,492M` is ambiguous because
  it reads as 32,492 million, while existing Nori/upstream compact formatting
  would be around `32.5M`.
- Quickest estimate: tiny, 15-30 minutes including one TUI snapshot update.
- Likely fix: replace raw `to_string()` with `format_si_suffix(goal.tokens_used)`,
  update the existing goal summary snapshot to cover a nonzero count, and decide
  whether `32.5M` is acceptable or whether this needs a new project-specific
  convention.

### Bug 5: Goal token count mirrors latest usage instead of cumulative capacity

- Finding: Confirmed. Goal token accounting treats ACP `UsageUpdate.used` as if it
  were cumulative goal spend, but ACP usage is "tokens currently in context". Nori
  subtracts a single baseline from a point-in-time context-window measurement.
- Evidence: `StoredThreadGoal` stores `tokens_used`, `token_usage_baseline`, and
  `last_session_used_tokens`. On each usage update,
  `nori-rs/acp/src/backend/thread_goal.rs` sets `goal.tokens_used =
  used_tokens.saturating_sub(baseline)`. That exactly explains why the goal count
  mirrors usage updates and can reset or drift across compaction, resume, subagent
  sidechains, and new ACP session IDs.
- Likely correct model: keep goal-owned cumulative usage and per-source/segment
  last-seen context values. For a same segment, add only positive deltas; when
  context usage drops because of compaction/resume/session changes, start a new
  segment instead of subtracting from an old baseline. Provider transcript totals
  may be needed to include subagent usage accurately, with deduping by transcript
  message/session identity.
- Quickest estimate: medium, 1-2 days for a focused segment-based approximation
  with tests. More if robust provider transcript aggregation across Claude, Codex,
  Gemini, and sidechain/subagent messages is required.
- Risks/unknowns: ACP does not appear to expose cumulative token spend directly.
  Provider transcript schemas differ and may be stale during live sessions. Replay
  and live `session/load` usage must be deduped to avoid double-counting.

### Bug 6: Agent repeats "Verified again..." and cannot stop the goal loop

- Finding: High-confidence root cause is continuation chaining based on advertised
  HTTP MCP capability rather than observed working `nori-goal` MCP tools. For
  Codex ACP, the tools fail to start because of Bug 1, but Nori still believes the
  agent can stop itself via goal MCP and therefore keeps chaining hidden
  continuations while the backend goal remains active.
- Evidence: `maybe_submit_goal_continuation` chains after a `GoalContinuation`
  when `connection.capabilities().mcp_capabilities.http` is true. The actual stop
  path requires the agent to call MCP `update_goal({"status":"complete"})` in
  `nori-rs/acp/src/backend/thread_goal_mcp.rs`. Since Codex never successfully
  initializes the `acp:` goal MCP server, the active goal never transitions to
  complete through that path.
- Why those history cells appear: hidden `GoalContinuation` prompts are not shown
  as user prompts, but assistant output is still recorded and finalized like normal
  assistant history. The repeated "Verified again..." messages are normal
  assistant completions from each chained hidden continuation, not duplicate TUI
  rendering of the same cell.
- Quickest estimate: small for mitigation, roughly 1-2 hours to disable chained
  continuation unless goal MCP is known usable. Medium, 0.5-1 day, to track an
  observed `_mcp/connect` success from the local MCP bridge and gate chaining on
  that runtime state.
- Likely fix: allow one post-user hidden continuation as the unsupported-agent
  fallback, but do not allow `GoalContinuation -> GoalContinuation` chaining until
  Nori has observed a working `nori-goal` MCP connection. A faster temporary
  mitigation is to disable chained continuations entirely.
