# Noridoc: nori-tui

Path: @/nori-rs/tui

### Overview

The `nori-tui` crate provides the interactive terminal user interface for Nori, built with the Ratatui framework. It handles the fullscreen TUI experience including chat display, input composition, onboarding flows, and real-time streaming of agent responses with markdown rendering.

### How it fits into the larger codebase

```
User Input --> nori-tui --> nori-harness (ACP backend)
                       \--> nori-config (Nori config, ~/.nori/cli/config.toml)
                       \--> codex-core (config, auth)
                       \--> codex-rmcp-client (MCP OAuth login)
                       \--> nori-protocol (ACP session events)
                       \--> codex-protocol (shared control-plane events)
```

The TUI acts as the frontend layer. It:

- Uses `nori-harness` for ACP agent communication: sessions are launched through the harness session runtime (`nori_harness::runtime::launch_session`, see `@/nori-rs/harness/src/runtime.rs`), and the TUI maps its `SessionEvent` stream onto `AppEvent`s (see `@/nori-rs/harness/`)
- Imports `NoriConfig` and the other Nori config types directly from `nori-config` (see `@/nori-rs/nori-config/`); they are not re-exported through `nori-harness`
- Uses `codex-core` for configuration loading and authentication (see `@/nori-rs/core/`)
- Uses `codex-sandbox` for platform sandbox availability checks (`get_platform_sandbox`) in approval flows (see `@/nori-rs/sandbox/`)
- Consumes `nori-protocol` for ACP session-domain rendering (messages, plans, tool snapshots, approvals, replay, lifecycle)
- Maps user-facing session controls such as `/goal` into typed `codex-protocol` operations, leaving ACP backend state ownership in `@/nori-rs/harness`
- Displays approval requests from the ACP layer and forwards user decisions back
- Renders streaming AI responses with markdown and syntax highlighting

The `cli/` crate's `main.rs` dispatches to `nori_tui::run_main()` for interactive mode. Feature flags propagate from CLI to TUI for coordinated modular builds.

Key dependencies: `ratatui` for rendering, `crossterm` for terminal events, `pulldown-cmark` for markdown parsing, `tree-sitter-highlight` for syntax highlighting.

### Core Implementation

Entry point is `main.rs` which delegates to `run_app()` in `lib.rs`. The `run_main()` function loads `NoriConfig` once early and reuses it for both the auto-worktree setup and the `vertical_footer` setting (passed as a parameter to `run_ratatui_app()`). After loading config, `run_main()` initializes the agent registry via `nori_harness::initialize_registry()` with any custom `[[agents]]` defined in `config.toml` (see `@/nori-rs/harness/docs.md` for registry details). Initialization failure is non-fatal (logged as a warning).

`NoriConfig` is also the source of truth for ACP backend diagnostics. The harness session runtime (`@/nori-rs/harness/src/runtime.rs`) loads `NoriConfig` itself when launching or resuming sessions and passes the resolved ACP proxy configuration into `AcpBackendConfig`, so enabling `[acp_proxy]` in config wraps every backend ACP subprocess in the wire logger without requiring the live backend to be reconfigured in place.

The auto-worktree startup flow first checks eligibility via `can_create_worktree()` (see `@/nori-rs/harness/docs.md`), then branches on the `AutoWorktree` enum:

| State                                                  | Timing                                 | Behavior                                                                                                                                    |
| ------------------------------------------------------ | -------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------- |
| Blocked (not a git repo, or already inside a worktree) | Before TUI init, in `run_main()`       | Sets `worktree_blocked_reason`; a `WorktreeBlockedScreen` popup is shown after onboarding explaining why, then continues without a worktree |
| `Automatic` (eligible)                                 | Before TUI init, in `run_main()`       | Calls `setup_auto_worktree()` immediately and overrides cwd                                                                                 |
| `Ask` (eligible)                                       | After TUI init, in `run_ratatui_app()` | Sets `pending_worktree_ask = true`, deferred to a TUI popup shown after onboarding but before `App::run()`                                  |
| `Off`                                                  | N/A                                    | Skips worktree creation entirely                                                                                                            |

The `Ask` popup is implemented by `nori::worktree_ask::run_worktree_ask_popup()`, a standalone mini-app screen using the same pre-`App` event-loop pattern as `nori::update_prompt` in release builds. It presents two options ("Yes, create a worktree" / "No, continue without a worktree") and returns a boolean. If the user confirms, `setup_auto_worktree()` is called and config is reloaded with the new cwd via `load_config_or_exit()`. Ctrl-C, Escape, and the "No" option all skip worktree creation. On failure, the TUI continues with the original cwd.

The `WorktreeBlockedScreen` popup (also in `nori::worktree_ask`) shows the blocked reason and a single "Continue without a worktree" option. It accepts Enter, Escape, or Ctrl-C/Ctrl-D to dismiss. The `worktree_blocked_reason` parameter is threaded from `run_main()` through to `run_ratatui_app()` as `Option<String>` and takes priority over `pending_worktree_ask` in the popup dispatch.

The main event loop in `app/mod.rs` processes:

1. **Terminal events** (keyboard input, resize) via `tui.rs`
2. **Backend events** from ACP: `BackendEvent::Client` carries normalized `nori_protocol::ClientEvent` session data, while `BackendEvent::Control` carries shared control-plane events
3. **App events** for state changes (agent selection, config updates)

The client-event stream now also includes lightweight ACP session metadata summaries. Most `ClientEvent::SessionUpdateInfo` values still render as ordinary info/history cells, but usage updates are handled specially: they update the footer context segment and are omitted from both live history cells and the view-only transcript.

The chat interface is managed by the `chatwidget/` module (`chatwidget/mod.rs` + submodules), which handles:

- User input composition with multi-line editing
- Message history display with markdown rendering
- File search integration (`file_search.rs`)
- Pager overlay for reviewing long content (`pager_overlay.rs`)

For replayed ACP conversations, user-authored message chunks are reconstructed upstream into `ReplayEntry::UserMessage` before they reach the widget. Live `MessageStream::User` deltas are therefore ignored by `ChatWidget` itself; the widget only needs to render the replay entry path, not duplicate the local composer state.

**Thread Goal UI** (`chatwidget/goal.rs`, `chatwidget/event_handlers.rs`, `slash_command.rs`):

The `/goal` command is a TUI command surface for ACP backend-owned goal state. `@/nori-rs/tui/src/slash_command.rs` advertises the command, while `@/nori-rs/tui/src/chatwidget/goal.rs` maps the command family (viewing, setting, status changes, clearing, and editing) into typed `codex_protocol::protocol::Op::ThreadGoal*` operations. Those operations are handled by `@/nori-rs/harness/src/backend/thread_goal.rs`; the TUI does not persist or derive goal state from prompt text. Typed `/goal ...` invocations are still persisted through the normal prompt-history path so users can recall or search the command text later without making prompt history the source of truth for goal state.

`ClientEvent::ThreadGoalUpdated` is treated as the source of truth for the visible current goal. `ChatWidget` stores that snapshot in `current_goal`, renders a compact history summary for new goals and objective/status changes, and uses it to seed `/goal edit` back into the composer. Accounting-only updates from backend usage refresh the cached snapshot without adding history cells. The summary formats elapsed time and token counts with the shared compact formatters from `@/nori-rs/protocol/src/num_format.rs`. `ClientEvent::ThreadGoalCleared` clears the cached snapshot and writes a short info message. Goal updates are omitted from view-only transcript rendering in `@/nori-rs/tui/src/viewonly_transcript.rs` because they are state synchronization events rather than conversation messages.

The TUI validates goal objective text through `@/nori-rs/protocol/src/protocol/mod.rs` before submitting a set operation, matching the backend's validation path. This keeps the UI responsive while preserving the backend as the authority for state transitions, resume rehydration, token accounting, and prompt `<goal_context>` injection.

`ClientEvent::SessionCapabilitiesChanged` carries the backend's current session capability projection, including derived availability for built-in commands. When the active ACP agent cannot receive the backend-owned `nori-client` MCP server, the TUI keeps `/goal` visible but disabled in the slash popup and blocks typed `/goal ...` submissions with the backend-provided reason.

For agents without MCP, the desired fallback is backend-owned context rather than
UI-specific hacks: the first prompt should carry a concise `<context>` block
explaining that the session is running in Nori CLI, linking to
`https://github.com/tilework-tech/nori-cli`, and noting which MCP-backed Nori
affordances are unavailable. MCP-capable agents receive Nori operating context
through the backend-owned `nori-client` resources/prompts described in
`@/nori-rs/harness/docs.md`.

`/goal edit` uses the cached goal immediately when available. If no snapshot is cached, it requests one from the ACP backend and marks the edit as pending until the backend replies. A no-goal response clears that pending flag before rendering the usage hint, preventing a later unrelated goal update from unexpectedly replacing the user's composer contents.

When `/goal <objective>` is used while `current_goal` contains an unfinished ACP goal, the TUI opens a `SelectionView` confirmation instead of immediately sending the mutation. Choosing "Replace current goal" forwards `AppEvent::CodexOp(Op::ThreadGoalSet)` with the replacement objective and `Active` status; choosing "Keep current goal" dismisses the popup without changing backend state. Completed goals are replaced directly because they no longer protect an in-progress objective. This mirrors the Codex goal replacement flow while preserving the invariant that only explicit user confirmation can overwrite an unfinished goal snapshot cached from `ClientEvent::ThreadGoalUpdated`; the ACP backend owns the follow-up behavior that starts active goal work immediately when it can.

The transcript pager overlay uses each history cell's transcript view rather than the live summary view. To keep reopened transcripts readable, the overlay caps non-patch cells at 20 lines and appends an omission marker, while patch cells keep their full diff output for review. In ACP sessions, `ClientToolCell` provides differentiated `transcript_lines()` for Execute tools (shell-style `$ command` format via `render_execute_transcript_lines()`) while exploring and edit cells reuse their `display_lines()` rendering for transcripts.

**Approval Request Routing** (`chatwidget/event_handlers.rs`, `bottom_pane/approval_overlay.rs`): ACP approval requests arrive as `ClientEvent::ApprovalRequest` containing a `nori_protocol::ToolSnapshot`. The `approval_request_from_client_event()` function performs two-way routing: Execute tools with `Invocation::Command` map to `ApprovalRequest::Exec` (bash-highlighted overlay), and everything else (including Edit/Delete/Move) maps to `ApprovalRequest::AcpTool`. The `AcpTool` variant carries a boxed `ToolSnapshot`, a `cwd: PathBuf` (threaded from `self.config.cwd` in the chat widget), and dispatches decisions via `Op::ExecApproval`, which gives users the "always approve" option that `ApplyPatch` did not have. The `From<ApprovalRequest>` impl in `approval_overlay.rs` applies `relativize_paths_in_text` to the title before building the overlay prompt and `DiffSummary`, so users see relative paths instead of absolute ones. The fullscreen approval preview in `app/event_handling.rs` also uses the real `cwd` from the request for `DiffSummary` construction. `ApprovalRequest::ApplyPatch` is now only used by the legacy non-ACP codex backend. History cells for AcpTool decisions are produced by `history_cell::new_acp_approval_decision_cell()`, using `format_tool_kind()` for the kind label.

For edit-like tools (Edit/Delete/Move), both the approval overlay and the fullscreen preview extract diff data from the `ToolSnapshot` and render a `DiffSummary`. The diff extraction reuses two `pub(crate)` helpers from `client_tool_cell.rs`: `diff_changes_from_artifacts()` (checks `Artifact::Diff` entries) with fallback to `changes_from_invocation()` (handles `Invocation::FileChanges` and `Invocation::FileOperations`). When diff data is available, the overlay renders a `DiffSummary` via `ColumnRenderable` and the fullscreen preview renders a `DiffSummary` overlay titled "P A T C H". When no diff data is available, both paths fall back to text-only rendering of title, invocation, and artifacts.

**ClientToolCell Rendering** (`client_tool_cell.rs`):

`ClientToolCell` wraps a `nori_protocol::ToolSnapshot` (and a `cwd` path for path normalization) and implements `HistoryCell`. All ACP tool kinds route through `ClientToolCell` via `handle_client_tool_snapshot`. The cell selects between four rendering paths based on cell state: exploring cells (Read/Search, auto-detected via `is_exploring_snapshot()` or merged via `exploring_snapshots`) use `render_exploring_lines(width)`, `ToolKind::Execute` uses `render_execute_lines(width)` for display and `render_execute_transcript_lines(width)` for shell-style transcripts, Edit/Delete/Move kinds use `render_edit_lines()` for semantic verb headers with diff content, and all remaining tool kinds use `render_generic_lines()` for the generic `"Tool [phase]: title (kind)"` format with invocation/artifact details.

**Exploring cell grouping**: When consecutive Read/Search/ListFiles snapshots arrive, they are merged into a single `ClientToolCell` with a grouped exploring rendering. The exploring display shows a compact `Explored`/`Exploring` header with tree-prefixed sub-items that group consecutive reads by basename (e.g., `Read file1.rs, file2.rs`) and show `Search`/`List` labels with compact arguments. Read output content is omitted from exploring cells since it is noise in history. The merge logic in `handle_client_tool_snapshot` checks whether the active cell is an exploring `ClientToolCell` and the new snapshot is also exploring; if so, it merges the snapshot via `merge_exploring()` rather than creating a new cell. `merge_exploring()` deduplicates by `call_id` — if a snapshot with the same call_id already exists in the group, it is updated in place rather than appended. Merged call_ids are tracked in `completed_client_tool_calls` so completions arriving after the cell is flushed to history don't get re-merged into a later exploring cell. A standalone Read/Search snapshot (not merged with others) still uses `render_exploring_lines` — the auto-detection via `is_exploring_snapshot()` in `display_lines`/`transcript_lines` routes it there without requiring explicit `mark_exploring()`. The generic fallback sub-item renderer avoids duplicating the kind label when the title already starts with it (case-insensitive prefix check), e.g., `List /path` instead of `List List /path`.

**Tool title sanitization** (`client_event_format.rs`): The `sanitize_tool_title()` function cleans up noisy tool titles produced by some agents (notably Gemini). It strips `[current working directory ...]` bracket patterns and trailing `(description text)` parenthetical metadata, then trims whitespace. This is applied in the approval request path and helper functions in `event_handlers.rs`, ensuring that tool kinds display clean titles in the TUI.

**Execute rendering**: The execute rendering path reuses shared utilities from `exec_cell/render.rs` (`truncate_lines_middle`, `limit_lines_from_start`, `output_lines`, `spinner`) and layout constants that match the `ExecCell` display layout. Output text is sourced preferentially from `raw_output["stdout"]`, falling back to `Artifact::Text` with code fence stripping only for completed/failed snapshots. During pending/in-progress phases, artifact text for execute tools contains the agent's description (e.g., "Print current UTC date/time"), not stdout, so the fallback is suppressed via `is_active_phase` gating in `execute_output_text()`. Exit code success is determined from `raw_output["exit_code"]` when present, otherwise inferred from `ToolPhase`.

For Codex-backed ACP sessions, this rendering path depends on `nori-protocol` normalizing shell-wrapper `rawInput.command` arrays and `rawInput.parsed_cmd` metadata into structured `Invocation::Command` / `Invocation::Read` / `Invocation::Search` / `Invocation::ListFiles` values. Without that normalization, `ClientToolCell` falls back to rendering raw protocol JSON instead of the compact command and exploration details the TUI expects.

**Edit/Delete/Move rendering** (`render_edit_lines()`): Edit, Delete, and Move tool kinds use a dedicated rendering path with semantic verb-based headers from `format_edit_tool_header()` (in `client_event_format.rs`):

| Kind   | In-Progress       | Completed        | Failed                  |
| ------ | ----------------- | ---------------- | ----------------------- |
| Edit   | `Editing {path}`  | `Edited {path}`  | `Edit failed: {path}`   |
| Delete | `Deleting {path}` | `Deleted {path}` | `Delete failed: {path}` |
| Move   | `Moving {path}`   | `Moved {path}`   | `Move failed: {path}`   |

The path is extracted from `locations[0].path` when available, falling back to parsing the title (stripping the kind prefix, e.g., `"Edit README.md"` -> `"README.md"`). Bullet styling: green bold for completed, red bold for failed, spinner for active. For failed edits, error text is extracted via `extract_error_text()` (checks `raw_output` for `"error"`, `"stderr"`, `"output"`, or bare string), with a `"(failed)"` fallback.

Diff content is rendered from two sources in priority order: (1) `Artifact::Diff` entries via `diff_changes_from_artifacts()`, (2) invocation data via `changes_from_invocation()` which handles both `Invocation::FileChanges` and `Invocation::FileOperations` (Create, Update, Delete, Move). Both helpers convert `nori_protocol` types to `codex_protocol::protocol::FileChange` for `create_diff_summary` from `diff_render.rs`. Update and move diffs use the real `cwd` to preserve file-context line numbers when the edited text can be found on disk, so completed edits show inline diffs whether the diff data arrives as artifacts or as invocation-level file changes.

The diff renderer preserves syntax-highlighter state across each update hunk before applying add/delete/context styling, then wraps styled spans by terminal display width rather than byte or character count. Move/update diffs use the destination path for syntax detection, so renamed files highlight as the language they become instead of the language implied by the old path.

**Header promotion**: For all Edit/Delete/Move tools (both single-file and multi-file), the `DiffSummary`'s first header line is promoted to the outer header position. For a single-file edit this is the verb+path+line counts (e.g., "Edited README.md (+1 -1)"); for a multi-file edit this is the aggregate header (e.g., "Edited 2 files (+2 -2)"). The promoted line's "• " bullet prefix is stripped and replaced with the phase-aware bullet styling (green bold for completed, red bold for failed). For Move tools, the "Edited" verb span is swapped to "Moved" during header construction. This produces exactly one header line per edit cell. Diff content lines below the header come directly from `create_diff_summary`, which applies a single 4-space `prefix_lines()` indent — matching the indentation used by `PatchHistoryCell` in the non-ACP path. The `prefix_lines()` helper (from `@/nori-rs/tui/src/render/line_utils.rs`) propagates `Line.style.bg` onto the indent prefix span so that diff background tints (add/delete colors) extend edge-to-edge across the full terminal width.

**Generic rendering**: The generic rendering path (`render_generic_lines()`) applies several cleanup passes to produce compact output: code fences are stripped from text artifacts via `strip_code_fences()` (shared with the execute path), the `Output:` prefix is omitted so artifact text renders directly as detail lines, invocation detail lines that are redundant with the title are suppressed (e.g., `Read: /path` when the title already says `Read /path`), and absolute paths under `cwd` are relativized in both the header and invocation lines.

Bullet styling is phase-aware: active tools show a spinner, failed tools (`ToolPhase::Failed`) show a red bold bullet (`"•".red().bold()`), and all other completed tools show a dim bullet.

For failed tools, error detail is extracted via a cascade: (1) text artifacts (via `format_artifacts`), (2) `extract_error_text()` which checks `raw_output` for `"error"`, `"output"`, or bare string values, (3) a `"(failed)"` fallback when no detail is available at all. For non-failed tools, the location fallback still applies: when both invocation formatting and artifact formatting produce zero detail lines, it displays the `locations` paths from the `ToolSnapshot` as dim sub-items. This prevents completed tool cells from rendering as bare headers with no context, which occurs when agents (e.g., Gemini) send tool calls with empty `content` arrays and no `rawInput`/`rawOutput`.

**Edit/Delete/Move routing**: All Edit/Delete/Move snapshots (all phases including Completed) are routed to `handle_client_tool_snapshot`, the same handler used by Execute tools. In-progress snapshots create a spinner cell in `active_cell`. When the completed snapshot arrives with the same `call_id`, `apply_snapshot()` updates the cell in place, transitioning it from the spinner state to the completed state with diff content. The completed cell is then flushed to history. For completed Edit/Delete/Move snapshots, `handle_client_tool_snapshot` also calls `observe_directories_from_paths()` (using the snapshot's `locations`) and records tool call stats. `PatchHistoryCell` is no longer used in the ACP rendering path -- it remains only for the non-ACP codex backend path (via `on_patch_apply_begin`). Edit/Delete/Move approval requests route through `ApprovalRequest::AcpTool` (not `ApplyPatch`), so there are no bridge functions converting `nori_protocol` types to `codex_protocol::protocol::FileChange` for the approval path -- the diff extraction for approval overlays reuses the same `pub(crate)` helpers in `client_tool_cell.rs` that the completed-cell rendering uses.

**Execute Cell Completion Buffering** (`chatwidget/event_handlers.rs`, `chatwidget/mod.rs`):

When the ACP backend sends parallel execute tool calls (e.g., `date --utc`, `uptime -p`, `df -h` simultaneously), the TUI's single `active_cell` slot can only hold one cell at a time. Without buffering, when a new tool snapshot displaces the current active Execute cell, the displaced cell would be flushed to history with incomplete content -- showing the agent's description text (e.g., "Print current UTC date/time") as command output instead of actual stdout.

The `pending_client_tool_cells: HashMap<String, ClientToolCell>` buffer holds incomplete Execute cells that were displaced from `active_cell`. The flow in `handle_client_tool_snapshot()`:

1. **Buffer lookup first**: Before creating a new cell, the handler checks if the incoming snapshot's `call_id` matches a buffered cell. If found, the buffered cell is updated via `apply_snapshot()`. If the cell is now complete, it is inserted directly into history via `AppEvent::InsertHistoryCell` (bypassing `add_boxed_history` to avoid flushing the current active cell). If still incomplete, it goes back into the buffer.

2. **Conditional displacement**: When a new snapshot arrives and the current `active_cell` is an incomplete Execute `ClientToolCell`, instead of calling `flush_active_cell()` (which would send it to history with wrong content), the cell is moved to the buffer keyed by its `call_id`. Non-Execute cells and completed cells still go through the normal `flush_active_cell()` path.

3. **Turn-boundary drain**: The buffer is cleared (orphans discarded) at all turn boundaries: `on_agent_message()`, `on_task_complete()`, `finalize_turn()`, and `on_context_compacted()`. Discarding orphans is preferred over flushing them with description text.

The displacement check uses `into_any()` on `dyn HistoryCell` (added in `history_cell/mod.rs`) for owned downcasting from `Box<dyn HistoryCell>` to the concrete `ClientToolCell` type, and `snapshot_kind()` to confirm the cell is `ToolKind::Execute`.

**Chronological Ordering Invariant** (`chatwidget/event_handlers.rs`, `chatwidget/user_input.rs`):

Tool cells always appear in scrollback history before the agent text that follows them, matching the chronological order of execution. This is enforced by two mechanisms:

- `handle_streaming_delta()` always calls `flush_active_cell()` before streaming text, even when the active cell contains an incomplete (still-running) ExecCell. The incomplete cell is sent to history immediately rather than held in `active_cell` until completion.
- `flush_active_cell()` marks pending call_ids of incomplete ExecCells as completed (via `completed_client_tool_calls`) so that later completion events for the same call_ids do not create duplicate cells. The `pending_exec_cells` tracker is bypassed for this path -- cells go directly to history.
- `add_boxed_history()` also always flushes the active cell first, applying the same ordering guarantee when non-streaming history cells are inserted.
- Assistant message cells do not re-arm the final-message separator; a single tool-to-answer boundary should not become repeated dividers when one assistant turn arrives as multiple message cells.
- The inverse also holds: no-op `ToolSnapshot` updates do not finalize the answer stream. `handle_client_tool_snapshot()` calls `flush_answer_stream_with_separator()` only on the paths that can insert a new history cell while a stream is open — a buffered Execute cell completing, or a genuinely new tool call creating/displacing an active cell. In-place updates of the active cell (including completions that flush it to history), snapshots for call_ids already in `completed_client_tool_calls`, re-buffering of still-incomplete pending cells, and exploring-cell merges all leave the stream open, so one streamed assistant message stays one `•` cell even while a long-running tool emits periodic `tool_call_update` progress notifications. This is safe because any path that sets `active_cell` flushes the stream first and the next answer delta clears `active_cell` via `flush_active_cell()`, so the stream cannot be open while a tool cell is active (`debug_assert`ed in the handler). Regression coverage: `noop_tool_updates_do_not_fragment_streaming_answer` in `@/nori-rs/tui/src/chatwidget/tests/part8.rs`.

The trade-off: incomplete cells may appear in scrollback showing "Running"/"Exploring" status rather than their final "Ran"/"Explored" state, because they are flushed before completion events arrive.

**Interrupt Queue & Tool Event Deferral** (`chatwidget/event_handlers.rs`):

When the agent streams text, ACP `ClientEvent::ToolSnapshot` updates can arrive concurrently with answer or reasoning deltas. All ACP tool kinds route directly through `ClientToolCell` via `handle_client_tool_snapshot`, and the handler calls `flush_answer_stream_with_separator()` whenever a snapshot will insert a new history cell, so tool cells appear in their correct interleaved position relative to text rather than being grouped after all text. The flush is deliberately not unconditional: snapshot updates that change nothing visible (see the Chronological Ordering Invariant above) leave the answer stream open, preventing one streamed message from fragmenting into many cells. Reasoning deltas also flush any open answer stream before updating the status header, preserving the visible boundary for answer -> reasoning -> answer sequences. The `InterruptManager` queues events via `defer_or_handle()` when the queue is already non-empty, preserving FIFO ordering for events that arrive while earlier deferred events are pending.

One operation consumes the queue:

| Method                          | Called From                                | Behavior                                                                                                                                    |
| ------------------------------- | ------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------- |
| `flush_completions_and_clear()` | `on_agent_message()`, `on_task_complete()` | Processes completion events whose Begin was already handled, discards Begin events and any End events whose Begin was discarded. See below. |

The selective flush ensures tool cells that are already visible transition from "Running" to "Ran", while preventing new "Explored" / "Ran" cells from appearing below the agent's final message.

**Begin/Completion Pairing in `flush_completions_and_clear`**: Tool begin and completion updates for the same `call_id` are still paired in the FIFO queue. When `flush_completions_and_clear` discards a deferred begin update, it records the `call_id` in a `HashSet`. Any later completion for the same `call_id` is discarded too. Without this pairing, a deferred completion can synthesize an orphan `ExecCell` from a normalized ACP tool snapshot after its begin state was already dropped.

**Reducer-Owned ACP Phase Wiring** (`chatwidget/event_handlers.rs`, `chatwidget/user_input.rs`):

ACP prompt ownership is now rendered from normalized reducer projections instead of the old lifecycle/interrupt timing path. `ChatWidget` consumes:

- `ClientEvent::SessionPhaseChanged(Idle|Loading|Prompt|Cancelling)` to drive input locking, status visibility, and the interrupt hint
- `ClientEvent::PromptCompleted { .. }` to finalize the turn when the real ACP prompt response arrives
- `ClientEvent::QueueChanged { prompts }` to render queued ACP prompts without owning a second prompt queue in the TUI

For ACP sessions, pressing Enter while the phase is `Prompt` or `Cancelling` still sends `Op::UserInput`; the backend reducer decides whether to send immediately or enqueue. Interrupt no longer restores queued ACP prompts into the composer, and `ChatWidget` no longer owns a second ACP submission queue.

**Stale Event Suppression:**

ACP cancel no longer makes the TUI idle on its own. The UI stays in `Cancelling` until the backend reduces the matching prompt response and emits `PromptCompleted`. See `@/nori-rs/harness/docs.md` for the backend-side reducer rules.

For ACP tool rendering, phase is no longer used as a visibility gate. Once the backend emits a normalized `ClientEvent::ToolSnapshot`, the chat widget renders it even if the ACP phase is already `Idle`, so late or update-only provider events remain visible instead of disappearing.

**Turn-Boundary Cleanup of Incomplete Tool Cells** (`chatwidget/event_handlers.rs`):

At ACP turn boundaries, `on_agent_message()` and `on_task_complete()` still explicitly finalize incomplete cells so the viewport is freed for the agent text and completed tool output can settle cleanly:

```
on_agent_message():
  1. flush_answer_stream_with_separator()    -- finalize any in-progress text stream
  2. finalize_active_cell_as_failed()        -- mark stuck active_cell as failed, flush to history
  3. pending_exec_cells.drain_failed()       -- drain any queued incomplete cells
  4. flush_completions_and_clear()           -- process deferred End events, discard orphan Begins

on_task_complete():
  1. flush_answer_stream_with_separator()
  2. flush_completions_and_clear()
  3. pending_exec_cells.drain_failed()
  4. finalize_active_cell_as_failed()        -- safety net for incomplete cells
  5. set_task_running(false)
```

`finalize_active_cell_as_failed()` (in `user_input.rs`) takes the cell from `active_cell`, calls `mark_failed()` on the underlying `ExecCell` or `McpToolCallCell`, and flushes it to history. This frees the viewport so subsequent content (the agent's response text) can be inserted via `insert_history_lines()`.

**Pinned Plan Drawer** (`pinned_plan_drawer.rs`, `chatwidget/mod.rs`, `chatwidget/event_handlers.rs`, `chatwidget/helpers.rs`):

Plan updates from the ACP agent (`ClientEvent::PlanSnapshot`) can be rendered in one of two ways, controlled by the `PlanDrawerMode` enum on `ChatWidget`:

| Mode             | `PlanDrawerMode` | Behavior                                                                  |
| ---------------- | ---------------- | ------------------------------------------------------------------------- |
| History cells    | `Off` (default)  | Each plan update creates a `PlanUpdateCell` in scrollback history         |
| Collapsed drawer | `Collapsed`      | One-line progress summary: `Plan: X/Y completed  *  > Current: step_name` |
| Expanded drawer  | `Expanded`       | Full plan checklist (same as the previous boolean `true` behavior)        |

The toggle cycle (bound to `Ctrl+O` via `HotkeyAction::TogglePlanDrawer`) is: `Off -> Collapsed -> Expanded -> Collapsed -> ...`. Once the drawer enters a visible mode, it cycles between Collapsed and Expanded without returning to Off. The `toggle_plan_drawer()` method on `ChatWidget` implements this state machine. The `App` layer intercepts the hotkey binding in `handle_key_event()` and updates both the widget and its own `plan_drawer_mode` field.

The `pinned_plan` field on `ChatWidget` always tracks the latest plan update, regardless of the current mode. In the ACP path, `handle_client_plan_snapshot()` converts the normalized snapshot into `UpdatePlanArgs`, stores it in `pinned_plan`, and when the mode is `Off`, clones it into scrollback as a `PlanUpdateCell`. This "always-store" invariant means toggling the drawer on mid-conversation immediately shows the most recent plan without waiting for the next update.

The drawer is inserted into the `FlexRenderable` layout in `ChatWidget::as_renderable()` as a flex=0 child between the active cell (flex=1) and the bottom pane (flex=0):

- `Collapsed` renders `PinnedPlanDrawerCollapsed` (1 line, shows progress count and current/next step with truncation)
- `Expanded` renders `PinnedPlanDrawer` (full checklist via `render_plan_lines()`)
- `Off` contributes zero height

The config persists a boolean `pinned_plan_drawer` in `[tui]` of `config.toml`. At startup, `true` maps to `Expanded` and `false` maps to `Off`. Runtime toggling via Ctrl+O does not persist -- only the `/settings` toggle persists.

The Nori-specific agent picker UI lives in `nori/agent_picker.rs`, allowing users to select between available ACP agents. It also exposes the ACP wire JSONL recorder as a same-line footer hint: `Shift-Tab` toggles `[acp_proxy].enabled` through the app config persistence path, updates the open picker and slash-command status text, and applies to future ACP child subprocesses. Existing running ACP subprocesses keep the proxy setting they were spawned with.

**ACP Session Config Mode Shortcut** (`nori/session_config_mode.rs`, `chatwidget/session_config_mode.rs`, `bottom_pane/footer.rs`):

When an ACP agent exposes a select-style session config option categorized as `Mode` (or using id `mode`), the TUI derives a compact mode snapshot from the same live `config_options` data used by `/config`. The current mode label flows into the normal footer segment pipeline as the `mode_indicator` segment, rendered as compact bracketed text such as `[ Plan ]`; labels longer than 20 characters are truncated with an ellipsis. By default, the segment appears in the right side of the footer line and is skipped entirely when the selected agent does not expose mode options. While the composer has focus and no popup is active, `Shift-Tab` fetches the current ACP session config snapshot from the agent handle, chooses the next mode value in the agent-provided option order (including grouped options), and applies it through `session/set_config_option`. Successful changes from either `Shift-Tab` or the ACP `/config` menu refresh the footer segment through `AppEvent::AcpModeConfigSnapshot`. This remains live-session only and does not persist mode selections to `config.toml`.

**Agent Options History** (`nori/session_config_history.rs`, `chatwidget/event_handlers.rs`, `chatwidget/pickers.rs`):

Live `ClientEvent::SessionConfigUpdate` events carry the full ACP session config snapshot into the TUI. `ChatWidget` stores the first supported select-option snapshot as a silent baseline, then compares later snapshots against it and writes a user-facing history line only for values that actually changed, such as `Claude Code option updated: Effort=High`. User-triggered `/config` changes skip the old intermediate "updating" history line and render a single final message, such as `Claude Code option set: Model=Opus 4.6`; the returned snapshot is synced immediately so a later backend echo does not duplicate the same update. When the set option is the agent's Model-category option, the selection is also persisted as the agent's default model and the final message carries a dim `(saved as default)` suffix (see the live config picker section below). Unsupported config kinds, newly added options, and removed options are ignored for history noise while still allowing the mode footer snapshot to refresh from supported values.

**System Info Collection** (`system_info.rs`):

The `SystemInfo` struct collects environment data in a background thread to avoid blocking TUI startup:

| Field                                   | Source                                                                                                                                                                            |
| --------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `git_branch`                            | Git repository branch name                                                                                                                                                        |
| `active_skillsets`                      | Active skillsets from `nori-skillsets list-active` (one name per line; returns all skillsets active for the current directory). Empty vec if the command is unavailable or fails. |
| `git_lines_added` / `git_lines_removed` | Git working tree statistics relative to `HEAD` for tracked files                                                                                                                  |
| `git_has_untracked`                     | Whether untracked, non-ignored files are present                                                                                                                                  |
| `is_worktree`                           | Whether CWD is a git worktree                                                                                                                                                     |
| `worktree_name`                         | Last path component of CWD when parent directory is `.worktrees`; used to display the immutable worktree directory identifier in the footer                                       |
| `transcript_location`                   | Discovered transcript path and token usage when running within an agent environment                                                                                               |
| `worktree_cleanup_warning`              | Warning when git worktrees exist and disk space is below 10% free (unix only)                                                                                                     |

The `transcript_location` field includes `token_breakdown` (detailed input/output/cached breakdown), which is displayed in the TUI footer when Nori runs as a nested agent inside Claude Code, Codex, or Gemini. It can also include `subagents_used`, which is merged into goodbye-card session stats when visible ACP events do not expose delegated subagent launches.

**Goodbye Card Session Stats**:

The goodbye card renders from `SessionStats` and does not parse transcripts directly. ACP sessions update those stats from normalized `ClientEvent` values:

- `ToolSnapshot` increments one tool group for each completed or failed `call_id`.
- Tool snapshots are scanned for `*/SKILL.md` paths in locations, invocations, artifacts, raw input, and raw output so skills are listed once by directory name.
- Agent-style snapshots with generic `Other("Other")` kinds fall back to the snapshot title for display, allowing `Agent` to appear as a tool group.
- ACP answer streams are counted as one assistant message at `PromptCompleted`, whether the final message arrives in `last_agent_message` or only as prior `MessageDelta { stream: Answer, .. }` chunks.
- `TranscriptLocation.subagents_used` is merged during system-info refresh as a narrow fallback for subagent launches that do not appear as visible ACP tool snapshots.

The footer git stats are intentionally scoped to uncommitted tracked-file
changes so the statusline stays compact in long-lived branches or repositories
with large histories. Untracked, non-ignored files render as a compact red `!`
alert instead of contributing line counts. The `/diff` command still produces a
PR-like diff when users ask for the full change context.

Two collection methods are provided:

- `collect_for_directory()` - Basic collection without first-message matching (test-only)
- `collect_for_directory_with_message()` - Preferred method that passes the first user message to the transcript discovery layer for accurate transcript identification across all agents

The first-message is obtained from `ChatWidget::first_prompt_text()`, which stores the text of the first submitted prompt. This flows through `SystemInfoRefreshRequest` to the background worker, enabling accurate transcript matching when multiple sessions exist in the same project directory.

**Refresh model:**

`spawn_system_info_worker` runs a background thread that blocks on its request channel: a refresh happens only when a `SystemInfoRefreshRequest` is sent via `request_system_info_refresh()`. There is no periodic polling. Refreshes are triggered on:

1. Startup (explicit initial refresh in `App::run()`)
2. User message submit (`chatwidget/user_input.rs`)
3. Task completion (`chatwidget/event_handlers.rs`)
4. Effective cwd change observed from tool-call directories or file-change paths (debounced 500ms by `EffectiveCwdTracker`)
5. Successful skillset install or switch (`app/event_handling.rs`)

This means an external change (e.g., the user runs `nori-skillsets switch` in another terminal) will not be reflected in the footer until the next event-driven refresh. Footer staleness is bounded by user activity, not by wall-clock time.

When a file-change path needs to be lifted to a repository root for refresh, `@/nori-rs/tui/src/effective_cwd_tracker.rs` uses `@/nori-rs/tui/src/git_marker.rs::is_git_marker()` so only worktree `.git` files or repository `.git` directories containing `HEAD` count as git roots. Empty marker-shaped directories are ignored and fall back to the nearest existing parent directory.

**Version caching:**

`get_nori_version()` shells out to `nori-skillsets --version` (or `nori-ai --version` as a legacy fallback) and caches the result in a process-wide `OnceLock` (`NORI_VERSION_CACHE`). The installed CLI version is stable for the lifetime of a TUI process, so the subprocess runs at most once per session. Only `nori-skillsets list-active` is re-invoked on every refresh.

**`/diff` Slash Command** (`get_git_diff.rs`):

The `/diff` handler in `key_handling.rs` resolves the effective CWD from the `effective_cwd_tracker` (falling back to `config.cwd`) and passes it to `get_git_diff()`. This ensures `/diff` works correctly in git worktrees and directories different from the process launch directory. All git commands in `get_git_diff.rs` use `.current_dir()` when a directory is provided.

`get_git_diff.rs` uses the same diff base resolution strategy as `system_info.rs` (`origin/HEAD` -> `main` -> `master` -> `HEAD` fallback), but implemented as async functions rather than the sync versions in `system_info.rs`. This duplication exists because the sync/async boundary makes sharing impractical. The result is that `/diff` output and the statusline diff stats are consistent -- both show PR-like diffs against the merge-base with the default branch.

**Worktree Cleanup Warning:**

During background system info collection on unix, `check_worktree_cleanup()` runs three checks in sequence: confirms the directory is a git repo via `git rev-parse --show-toplevel`, lists extra worktrees via `codex_git::list_worktrees()` (see `@/nori-rs/utils/git/`), and checks disk space via `df -Pk`. If worktrees exist and free disk space is below the `DISK_SPACE_LOW_PERCENT` threshold (10%), a `WorktreeCleanupWarning` is attached to the `SystemInfo` result. When the `App` event loop handles `SystemInfoRefreshed`, it checks for this warning and calls `chat_widget.add_warning_message()` to display a yellow warning cell in the chat history suggesting the user clean up unused worktrees. Non-unix platforms skip this check entirely.

**Slash Commands:**

| Command                   | Description                                                                                                                                                                                                                                                                                                |
| ------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `/agent`                  | Switch between available ACP agents (dynamically shows current agent name)                                                                                                                                                                                                                                 |
| `/model`                  | Choose model -- convenience shortcut that opens the Model-category config option value picker when the agent advertises one, otherwise shows a "not supported" fallback (dynamically shows current agent/model name)                                                                                                        |
| `/config`                 | Configure live ACP session settings exposed by the current agent                                                                                                                                                                                                                                           |
| `/approvals`              | Choose what Nori can do without approval (dynamically shows current approval mode)                                                                                                                                                                                                                         |
| `/settings`               | Configure Nori CLI settings (pinned plan drawer, custom working messages, vertical footer, terminal notifications, OS notifications, vim mode with enter behavior sub-picker, auto worktree, per session skillsets, notify after idle, hotkeys, script timeout, loop count, footer segments, file manager) |
| `/browse`                 | Open a terminal file manager to browse and edit files                                                                                                                                                                                                                                                      |
| `/new`                    | Start a new chat during a conversation                                                                                                                                                                                                                                                                     |
| `/resume`                 | Resume a previous ACP session                                                                                                                                                                                                                                                                              |
| `/close`                  | Close (release) the current agent-side session and return to the session picker -- capability-gated on ACP `session/close`                                                                                                                                                                                 |
| `/init`                   | Create an AGENTS.md file with instructions                                                                                                                                                                                                                                                                 |
| `/resume-viewonly`        | View a previous session transcript (read-only)                                                                                                                                                                                                                                                             |
| `/compact`                | Summarize conversation to prevent context limit                                                                                                                                                                                                                                                            |
| `/undo`                   | Open undo snapshot picker to select a restore point                                                                                                                                                                                                                                                        |
| `/diff`                   | Show PR-like git diff (changes since merge-base with default branch, plus untracked files)                                                                                                                                                                                                                 |
| `/mention`                | Mention a file                                                                                                                                                                                                                                                                                             |
| `/status`                 | Show session configuration and context window usage                                                                                                                                                                                                                                                        |
| `/memory`                 | Show the contents of all active instruction files (CLAUDE.md / AGENTS.md / GEMINI.md)                                                                                                                                                                                                                      |
| `/first-prompt`           | Show the first prompt from this session                                                                                                                                                                                                                                                                    |
| `/mcp`                    | Manage MCP server connections (add, toggle, delete) via interactive wizard                                                                                                                                                                                                                                 |
| `/login`                  | Log in to the current agent                                                                                                                                                                                                                                                                                |
| `/logout`                 | Show logout instructions                                                                                                                                                                                                                                                                                   |
| `/switch-skillset [name]` | Switch between available skillsets (with optional direct name)                                                                                                                                                                                                                                             |
| `/fork`                   | Rewind conversation to a previous message                                                                                                                                                                                                                                                                  |
| `/browser`                | Launch a headed Chrome browser the agent can control via CDP (Chrome DevTools Protocol)                                                                                                                                                                                                                    |
| `/quit`                   | Exit Nori -- on cloud-lifecycle agents this is a detach; the session keeps running server-side (see "Exit Is Detach")                                                                                                                                                                                      |
| `/exit`                   | Exit Nori (alias for /quit)                                                                                                                                                                                                                                                                                |

**`/mcp` Picker** (`nori/mcp_server_picker.rs`):

The `/mcp` command opens an interactive `BottomPaneView` for managing MCP server connections (same pattern as `HotkeyPickerView`). It is not available during a task. The picker operates as a state machine with these modes:

| Mode                      | Purpose                                                                                   | Transitions                                                                                                                                   |
| ------------------------- | ----------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------- |
| `List`                    | Browse servers; "Add new..." row at index 0, servers below                                | Enter on "Add new..." -> `TransportSelect`; Enter on server -> toggle enabled; `d` on server -> `ConfirmDelete`; `l` on server -> OAuth login |
| `ConfirmDelete`           | Confirm server deletion                                                                   | `d` -> delete + save + `List`; Esc -> `List`                                                                                                  |
| `TransportSelect`         | Choose Stdio or HTTP transport                                                            | Enter -> `NameInput`                                                                                                                          |
| `NameInput`               | Type server name                                                                          | Enter -> `CommandInput` (stdio) or `UrlInput` (http)                                                                                          |
| `CommandInput`            | Type command for stdio transport                                                          | Enter -> `ArgsInput`                                                                                                                          |
| `ArgsInput`               | Type space-separated args                                                                 | Enter -> `EnvInput`                                                                                                                           |
| `UrlInput`                | Type URL for HTTP transport                                                               | Enter -> `HeaderInput`                                                                                                                        |
| `EnvInput`                | Type env vars as `KEY=VAL`                                                                | Enter with empty -> finalize (stdio only); Enter with value -> adds to list, stays in `EnvInput`                                              |
| `HeaderInput`             | Type headers as `Key: Value` (HTTP only)                                                  | Enter with empty -> `SecretInput`; Enter with value -> adds to list, stays in `HeaderInput`                                                   |
| `SecretInput`             | Type bearer token env var name (HTTP only)                                                | Enter with value -> finalize (bearer token and client credentials are mutually exclusive); Enter with empty -> `ClientIdInput`                |
| `ClientIdInput`           | Type pre-registered OAuth client ID (HTTP only, for servers without dynamic registration) | Enter with value -> `ClientSecretEnvVarInput`; Enter with empty -> finalize (skip client credentials); Esc -> `SecretInput`                   |
| `ClientSecretEnvVarInput` | Type env var name for OAuth client secret (HTTP only)                                     | Enter -> finalize; Esc -> `ClientIdInput` (restores typed client ID)                                                                          |
| `OAuthInProgress`         | Inline OAuth status display                                                               | Esc -> emits `McpOAuthLoginCancel`, returns to `List`                                                                                         |

The wizard field set matches Claude Code's `claude mcp add` command: transport type, name, command/url, args, env vars, headers, bearer token env var, plus optional OAuth client credentials for servers that do not support dynamic client registration.

On finalize, the wizard builds an `McpServerConfig` with the appropriate `McpServerTransportConfig` variant (stdio or HTTP, with `bearer_token_env_var`, `client_id`, and `client_secret_env_var` populated from the wizard fields for HTTP), inserts it into the servers list, and calls `save_servers()`. When a bearer token is provided, client credential fields are left as `None` since the two auth methods are mutually exclusive. All mutations (toggle, delete, add) send `AppEvent::SaveMcpServers` with the full `BTreeMap<String, McpServerConfig>`. The `App` handles this via `persist_mcp_servers()` in `config_persistence.rs`, which uses `ConfigEditsBuilder::replace_mcp_servers()` for atomic config file writes. On success, an info message tells the user to restart since MCP connections are established at session startup.

**Auto-OAuth Probe**: When an HTTP server is added without a bearer token (`wizard_bearer_token_env_var` is empty), `finish_wizard()` sets `pending_oauth_server` to the new server name and fires `AppEvent::ComputeMcpAuthStatuses`. This applies even when client credentials are provided -- the auto-probe checks server auth capability and triggers OAuth login if the server reports `NotLoggedIn`. When auth statuses arrive, `update_mcp_auth_statuses()` checks if the pending server reports `NotLoggedIn` -- if so, it emits `AppEvent::McpOAuthLogin` and transitions to `Mode::OAuthInProgress`. If the server reports `Unsupported` or any other status, the pending server is cleared and the picker stays in `List` mode. This provides a seamless setup flow where users add an HTTP server and are automatically prompted for OAuth if the server requires it.

The picker is opened by `ChatWidget::open_mcp_servers_popup()` in `chatwidget/pickers.rs`, which converts `config.mcp_servers` to a `BTreeMap` and creates the view via `McpServerPickerView::new()`. After creating the picker, it fires `AppEvent::ComputeMcpAuthStatuses` to asynchronously populate auth statuses.

**MCP OAuth Login** (`nori/mcp_server_picker.rs`, `app/config_persistence.rs`):

OAuth login can be triggered two ways: (1) pressing `l` in the `/mcp` list on a server with `NotLoggedIn` status, or (2) automatically via the auto-probe mechanism after adding an HTTP server without a bearer token. Both paths emit `AppEvent::McpOAuthLogin`.

Auth statuses are computed asynchronously when the picker opens:

```
open_mcp_servers_popup()
    -> sends AppEvent::ComputeMcpAuthStatuses
    -> App spawns tokio task calling codex_core::mcp::auth::compute_auth_statuses()
    -> results delivered via AppEvent::McpAuthStatusesReady(HashMap)
    -> ChatWidget.update_mcp_auth_statuses() -> BottomPane -> active BottomPaneView
    -> McpServerPickerView.update_mcp_auth_statuses() stores statuses
        (also auto-triggers OAuth for pending_oauth_server if NotLoggedIn)
    -> handle_list_login() checks status before emitting AppEvent::McpOAuthLogin
```

The `BottomPaneView` trait has default no-op `update_mcp_auth_statuses()` and `handle_mcp_oauth_complete()` methods; only `McpServerPickerView` implements them. This pattern pushes data INTO a view through the trait interface, since the view stack does not support downcasting.

The OAuth flow is fully async and inline -- no TUI suspension. The `McpOAuthLogin` event carries `server_name`, `server_url`, `http_headers`, `env_http_headers`, `client_id`, and `client_secret_env_var`. The handler in `app/config_persistence.rs` (`perform_mcp_oauth_login()`) resolves `client_secret` from the environment variable named by `client_secret_env_var` (if provided), then calls `codex_rmcp_client::start_oauth_login()` from `@/nori-rs/rmcp-client/`, passing the optional `client_id` and resolved `client_secret`. This selects between dynamic registration and pre-configured credential OAuth paths (see `@/nori-rs/rmcp-client/docs.md`). The returned `OAuthLoginHandle` includes the generated authorization URL, which the TUI displays so remote/SSH users can copy it manually if the browser launch is not visible. The handle's cancel sender is stored in `App.mcp_oauth_cancel_tx`, and a spawned watcher task awaits the handle's `JoinHandle` and sends `AppEvent::McpOAuthLoginComplete` on finish.

Cancellation uses the oneshot channel pattern: Esc in `OAuthInProgress` mode emits `McpOAuthLoginCancel`, which calls `cancel_mcp_oauth_login()` (sends `()` on the stored cancel sender). The watcher task then resolves with the cancellation error. Completion (`McpOAuthLoginComplete`) shows a success or error info message and forwards to `McpServerPickerView::handle_oauth_complete()`, which transitions the picker from `OAuthInProgress` back to `List` mode. OAuth task failures are formatted with their full error chain so callback and token-exchange failures expose the underlying cause instead of only the top-level context.

**Agent-Provided Commands and Skill Mentions** (`bottom_pane/command_popup.rs`, `bottom_pane/skill_popup.rs`, `bottom_pane/chat_composer/popup_management.rs`, `bottom_pane/chat_composer/key_handling.rs`, `chatwidget/event_handlers.rs`):

ACP agents can advertise commands via the `AvailableCommandsUpdate` session notification. These flow through `nori-protocol` as `ClientEvent::AgentCommandsUpdate` and are forwarded to `BottomPane::set_agent_commands()` -> `ChatComposer::set_agent_commands()`. The same `agent_commands` collection backs both `CommandPopup` (slash-dispatched commands) and `SkillPopup` (`$` skill mentions). The agent slug/prefix (e.g., `"claude-code"`) is set separately via `BottomPane::set_agent_slug()`, called from `ChatWidget::set_agent()` and `set_pending_agent()`.

Non-`$` agent commands appear in the slash command popup alongside builtins and user prompts. They display with a prefixed name (e.g., `/claude-code:loop`) to disambiguate from builtins. If an agent command shares a name with a builtin command, the agent command is excluded from the popup. Commands whose names start with `$` are also excluded from `CommandPopup`; those are treated as Codex-style skill mentions rather than slash-dispatched commands. Fuzzy filtering operates on the prefixed display name. The prefix is a TUI display concept only -- it is stripped before submission so the ACP agent receives the bare command name. Tab autocompletes to `/<prefix>:<name> ` (e.g., `/claude-code:loop `) in the input field, but both the popup selection path and the typed text submission path strip the prefix: Enter from the popup submits `/<name>` (e.g., `/loop`), and typing the prefixed form directly (e.g., `/claude-code:loop 5m hi`) submits `/loop 5m hi` after the prefix-stripping logic in `key_handling.rs`. The Enter submission fallback path checks `agent_commands` after builtins and user prompts. Each `AgentCommandsUpdate` fully replaces the previous set.

The `$` skill picker is a separate composer popup (`ActivePopup::Skill`) rather than a mode of the slash command popup. `ChatComposer::sync_selection_popups()` owns popup precedence: history search keeps its own lifecycle; an `@token` under the cursor opens file search first; otherwise a `$token` can open `SkillPopup`; then slash-command sync runs; file search is the fallback. `current_dollar_token()` finds the whitespace-delimited token under the cursor, records the byte range to replace, strips the leading `$` into the query, and marks Claude slash-command compatibility only when the token begins at true prompt start (`start_idx == 0` in the full composer text).

By default, `skill_picker_items()` exposes ACP/agent command declarations whose command name starts with `$`. `SkillPickerItem.display_name` strips the leading `$` for display and fuzzy matching, while `insert_text` preserves the native command text (for example, selecting `using-skills` inserts `$using-skills`). Selection via Enter or Tab replaces only the current `$token` range and leaves the cursor after the inserted text. Esc stores the dismissed query in `dismissed_skill_popup_token` so the same token does not immediately reopen until the query changes.

Claude-backed agents have an additional compatibility path in `skill_picker_items()`: when the active prefix is `claude` or `claude-code` and the `$token` is at the very start of the composer text, regular non-builtin, non-`$` agent commands are exposed through `SkillPopup` as skill-like rows. These rows display the bare command name, but their `insert_text` preserves Claude's native slash invocation form with the agent prefix and trailing space (for example, `/claude-code:loop `). The same commands are not exposed through the `$` picker in mid-prose or after a newline; those `$token`s are treated as literal text unless a `$`-prefixed command matches.

`SkillPopup` uses the same popup infrastructure as other bottom-pane selection surfaces: `ScrollState` for wrapping Up/Down and Ctrl-P/Ctrl-N movement, `MAX_POPUP_ROWS` for visible-row limits, `fuzzy_match()` for query matching, and `selection_popup_common::render_rows()` / `measure_rows_height()` for row layout. Updating `agent_commands` or the agent prefix while a skill popup is active resynchronizes the open popup from the latest command declarations.

**Slash Command Description Overrides:**

`/agent`, `/model`, and `/approvals` show the current runtime value in parentheses in the slash command popup (e.g., `(current: Mock ACP)`). This is implemented via a `command_description_overrides: HashMap<SlashCommand, String>` that flows through `BottomPane` -> `ChatComposer` -> `CommandPopup`. `BottomPane::set_agent_display_name()` sets overrides for both `/agent` and `/model`; `BottomPane::set_approval_mode_label()` sets the override for `/approvals`. The agent override is populated at startup in `BottomPane::new()` and updated on agent switches. The approval override is set whenever the approval mode changes.

**Live ACP Session Config Picker** (`chatwidget/pickers.rs`, `nori/session_config_picker.rs`):

`/config` opens a two-step picker for the current ACP session. `ChatWidget::open_session_config_popup()` asks the `AcpAgentHandle` for the live `AcpBackend::config_options()` snapshot, renders supported `select` options, then opens a value picker for the selected option. Selecting a value sends `session/set_config_option` through `AcpBackend::set_config_option()` and shows a single final info or error message when the RPC finishes.

The picker does not run during `/agent` switching, and unsupported ACP config kinds and future non-exhaustive select layouts are treated as unavailable rather than guessed. Selections edit the active session, with one persistence exception: when a successful `AppEvent::AcpSessionConfigSetResult` (which carries the raw `config_id` and `value` alongside the display names) names the agent's Model-category option, the `app/event_handling.rs` handler calls `persist_default_model_selection()` in `app/config_persistence.rs`, which writes the value to `[default_models]` in `config.toml` keyed by agent slug via `ConfigEditsBuilder::set_default_model()` (see `@/nori-rs/core/docs.md`). Non-model selections (mode, thought level) are never persisted -- the persistence helper checks the returned config_options snapshot for a matching option with the Model category before writing anything. Persist failures are logged and never block the UI; the live session change still applies, and the history line simply omits the `(saved as default)` suffix. The persisted value is re-applied at the next session start by the ACP backend (see `@/nori-rs/harness/docs.md`).

`/model` acts as a convenience shortcut into the same config_options mechanism and is a two-tier flow. `ChatWidget::open_model_popup()` in `chatwidget/pickers.rs` fetches config_options via `AcpAgentHandle::get_session_config()` and finds a config option with `SessionConfigOptionCategory::Model`: (1) if present, it sends `AppEvent::OpenAcpSessionConfigValuePicker` to open the value picker directly (bypassing the top-level config picker); (2) otherwise it sends `AppEvent::OpenAcpModelPickerUnsupported` to show a "not supported" fallback picker. The previous middle tier (the unstable `SessionModelState` / `session/set_model` fallback) no longer exists -- that API and the harness/`nori-tui` `unstable` feature were removed. Selecting a value runs the same stable Model-category config-option path described above, so the chosen model is persisted as the agent's default and the history line carries the dim `(saved as default)` suffix when the `[default_models]` write succeeds. Note that `ConfigEditsBuilder::set_model` / `AppEvent::PersistAgentSelection` is a separate Codex config-edit that persists the chosen model as the default in `config.toml`; it is unrelated to the removed ACP `session/set_model` RPC and still exists.

**Pending-agent short-circuit:** ACP models are session-scoped -- an agent's models only arrive in the `session/new` response, so they are not knowable until a session starts. Because `/agent` only records a *pending* switch in `ChatWidget.pending_agent` (the live `acp_handle` and subprocess are not swapped until the next prompt submit rebuilds the `ChatWidget`; see "Agent-Provided Commands and Skill Mentions" and `set_pending_agent`), `open_model_popup()` checks `pending_agent` *before* touching the handle. When a switch is pending, it synchronously renders an explanatory picker built by `acp_model_picker_pending_agent_params(display_name)` in `nori/agent_picker.rs` telling the user to send a message to start the new session before `/model` can show that agent's models. This avoids querying the still-live OLD agent's handle, which would otherwise display the wrong agent's models.

**Selection Popup Row Layout (`bottom_pane/selection_popup_common.rs`):**

`render_rows()` and `measure_rows_height()` are the shared rendering functions used by selection popups that render command-like rows (`ListSelectionView`, `CommandPopup`, `SkillPopup`, `FileSearchPopup`). Each popup item has an optional description that appears alongside the item name. The layout engine chooses between two modes per-row via `wrap_row()`:

| Mode         | Condition                                         | Layout                                                                                              |
| ------------ | ------------------------------------------------- | --------------------------------------------------------------------------------------------------- |
| Side-by-side | `total_width - desc_col >= MIN_DESC_COLUMNS` (12) | Description starts at `desc_col` on the same line as the name, wrapped lines indented to `desc_col` |
| Stacked      | `total_width - desc_col < MIN_DESC_COLUMNS`       | Name on its own line(s), description on separate line(s) below with 4-space indent                  |

The `desc_col` is computed once per render pass from the widest visible name plus 2 columns of padding. The stacked fallback prevents descriptions from being squeezed into 1-2 characters of horizontal space on narrow terminals. Because both `render_rows()` and `measure_rows_height()` call the same `wrap_row()` function, layout and height calculation are always consistent.

`SelectionViewParams` supports an optional `on_dismiss: Option<SelectionAction>` callback that fires when the picker is dismissed without selection (Escape or Ctrl-C). The callback is invoked in `ListSelectionView::on_ctrl_c()` before marking the view as complete. It does not fire when the user makes a selection via `accept()`. This is used by the skillset picker to send `SkillsetPickerDismissed` when the deferred agent spawn needs a fallback trigger.

`BottomPane` can also forward item updates and removals into the active selection view. `ListSelectionView` matches rows by the stable id stored at the beginning of `SelectionItem.search_value`, then reapplies filtering after the update. This is used by `/resume` to show metadata-only rows immediately and lazily fill in preview text and turn counts after transcript scans complete.

**ListSelectionView Vim-Mode-Aware Search:**

`ListSelectionView` supports a `vim_mode: bool` field (alongside `is_searchable`) that changes how key input is routed. When a searchable view is created, `BottomPane::show_selection_view()` automatically injects the current `vim_mode_enabled` state into `SelectionViewParams`, so individual callers (skillset picker, config picker, etc.) do not need to pass vim mode explicitly.

The view operates as a state machine with three key-handling branches:

| Config                                 | Sub-state             | Key behavior                                                                                                                         |
| -------------------------------------- | --------------------- | ------------------------------------------------------------------------------------------------------------------------------------ |
| `vim_mode=true`, `is_searchable=true`  | `search_active=false` | `j`/`k` navigate, `/` activates search, digits 1-9 select directly, Esc dismisses                                                    |
| `vim_mode=true`, `is_searchable=true`  | `search_active=true`  | Characters filter the list, Backspace edits query, Esc exits search (clears query, returns to nav mode) without dismissing the popup |
| `vim_mode=false`, `is_searchable=true` | N/A                   | All characters immediately filter the list (no explicit search activation needed)                                                    |
| `is_searchable=false`                  | N/A                   | `j`/`k` navigate, digits 1-9 select directly (unchanged legacy behavior)                                                             |

The `show_search_row()` method controls whether the search input row renders: in vim mode, it only appears when `search_active=true`. In non-vim mode, it always appears for searchable views.

The `effective_footer_hint()` method generates context-sensitive footer hints reflecting the current state (vim nav mode, vim search mode, or non-vim search mode). If a static `footer_hint` was provided in `SelectionViewParams`, it takes precedence over the generated hint.

Number prefixes (e.g. "1. Item Name") are shown on rows when digits can be used for direct selection: either `is_searchable=false`, or `vim_mode=true` with `search_active=false`. When the search input is active (either non-vim searchable or vim search mode), number prefixes are hidden since digits go to the search query.

**Undo Snapshot Picker (`/undo`):**

The `/undo` slash command sends `Op::UndoList` (not `Op::Undo`) to the ACP backend. When the backend responds with `UndoListResult`, the TUI opens a `ListSelectionView` modal (the same pattern used by the approvals popup, etc.) displaying all available snapshots. Each item shows `[short_id] truncated_label` where the label is truncated to 60 characters. Selecting a snapshot dispatches `Op::UndoTo { index }` to restore to that point. If no snapshots are available, an info message is displayed instead of the modal.

**Compact Session Boundary (`/compact`):**

When the ACP backend sends a `ContextCompactedEvent` with a summary, `on_context_compacted()` renders a visual session boundary to show that a new session has begun. The sequence is:

1. Flush the in-progress streamed summary (old session content)
2. Show "Context compacted" as an info message
3. Insert a `NoriSessionHeaderCell` (the "Nori CLI" card, same as starting a fresh session) by constructing a `SessionConfiguredEvent` from the current widget config state
4. Reprint the summary text as the first assistant message of the new session

When the event has no summary (core backend path), only the "Context compacted" info message is shown. This asymmetry exists because the core backend compacts history in-place without producing a summary for the TUI.

**Fork Conversation (`/fork`) (`nori/fork_picker.rs`, `app_backtrack.rs`):**

The `/fork` slash command lets users rewind to a previous user message and branch the conversation from that point. It is only available when no task is running (`available_during_task = false`). The flow:

1. `SlashCommand::Fork` dispatches `AppEvent::OpenForkPicker`
2. The handler calls `collect_user_messages()` in `app_backtrack.rs` to gather all user messages from the current session segment (messages after the last `SessionInfoCell`). If none exist, an info message is shown instead of the picker.
3. `fork_picker_params()` in `nori/fork_picker.rs` builds a `SelectionViewParams` with items displayed newest-first (reversed from chronological order). Message previews are truncated to 80 characters; multiline messages show only the first line with an ellipsis.
4. Selecting a message fires `AppEvent::ForkToMessage { nth_user_message, prefill }`
5. The `ForkToMessage` handler:
   - Calls `build_fork_summary()` to create a plain-text summary of the conversation up to (but not including) the selected message, formatted as `User: ...\nAssistant: ...\n` pairs
   - Shuts down the current conversation
   - Creates a new `ChatWidget` with `fork_context` set to the summary string
   - Trims `transcript_cells` to the fork point via `trim_transcript_cells_to_nth_user()` so the TUI preserves visual history before the fork
   - Prefills the composer with the selected message text

The fork context flows through `ChatWidgetInit.fork_context` -> `spawn_agent()` -> `SessionLaunchSpec.initial_context` -> `AcpBackendConfig.initial_context`, which initializes the ACP backend's `pending_compact_summary`. This reuses the same mechanism as `/compact` and `/resume` -- the summary is prepended to the first user prompt in the new session, giving the agent prior conversation context without a protocol-level session fork.

**Caller-injected agents (`nori cloud`):** `Cli.extra_agents` (a clap-skipped field on `@/nori-rs/tui/src/cli.rs`, never a CLI flag) carries extra `AgentConfigToml` registry entries from the caller. `run_main()` in `@/nori-rs/tui/src/lib.rs` appends them after the config's `[[agents]]` when initializing the agent registry. The CLI's `nori cloud` subcommand uses this to pin a synthetic `nori-cloud` entry that runs `nori-handroll cloud-acp` (see `@/nori-rs/cli/src/cloud.rs` and `@/nori-rs/cli/docs.md`); from the TUI's perspective it is an ordinary local ACP agent and `spawn_agent()` treats it like any other registry entry. The only other cloud-entry plumbing is the clap-skipped `Cli.cloud_session_picker` flag, which triggers picker-first entry (see "Picker-First Cloud Entry" below); the old `cloud_connection` threading through `Cli`/`App`/`ChatWidgetInit` was removed.

**Session context injection:** The shared launch path in `chatwidget/agent.rs` (used for both fresh spawns and resumes) sets `SessionLaunchSpec.session_context` to the contents of `@/nori-rs/tui/session_context.md` (loaded at compile time via `include_str!`). The ACP backend only prepends that fallback `<context>` block to the first user prompt when the active ACP connection lacks HTTP MCP support. MCP-capable agents instead receive the backend-owned `nori-client` server and discover Nori operating context through its resources and prompts (see `@/nori-rs/harness/docs.md` for the hook context injection mechanism).

**Browser Session (`/browser`) (`chatwidget/key_handling.rs`, `app/event_handling.rs`, `app_event.rs`):**

The `/browser` slash command launches a headed Chrome browser with CDP (Chrome DevTools Protocol) remote debugging enabled, then injects the connection details into the conversation so the agent can script the browser via its existing shell tool. It is not available during a task (`available_during_task = false`). The flow:

1. `SlashCommand::Browser` in `key_handling.rs` shows an info message ("Launching browser...") and spawns a `tokio` task calling `nori_harness::backend::browser_session::BrowserSession::launch()` (see `@/nori-rs/harness/docs.md`)
2. On success, the task posts `AppEvent::BrowserLaunched { ws_url, cdp_port }`. On failure, it posts `AppEvent::BrowserLaunchFailed(error_string)`
3. The `BrowserLaunched` handler in `app/event_handling.rs` calls `browser_session::compose_agent_prompt()` to build a structured message containing the CDP HTTP endpoint and WebSocket URL, then submits it as a user message via `submit_user_message_text()`
4. The agent receives the CDP connection details and can use Playwright, Puppeteer, or raw CDP commands via its shell tool to control the browser

The `BrowserSession` is intentionally `std::mem::forget`'d after launch so Chrome stays alive for the duration of the nori session. The `BrowserSession::Drop` impl sends SIGTERM to the Chrome process, which fires when the nori process exits. This is distinct from `/browse` which opens a terminal file manager.

The `/logout` command is only available when the `login` feature is enabled.

**Status Card (`/status`) (`nori/session_header/mod.rs`):**

The `/status` command renders a bordered card in the chat history showing session state. The card is built by `new_nori_status_output()` which creates a `CompositeHistoryCell` containing the `/status` echo and a `NoriSessionHeaderCell`.

Data flows from `ChatWidget::add_status_output()` which pulls live state from `BottomPane`:

```
ChatWidget::add_status_output()
    |-- bottom_pane.prompt_summary()              --> task summary
    |-- bottom_pane.transcript_token_breakdown()   --> token counts from transcript
    |-- bottom_pane.context_window_percent()        --> context % from live API
    |-- approval_mode_label(config)                --> approval mode from config
    v
new_nori_status_output() --> NoriSessionHeaderCell::new_with_status_info()
```

The card always shows: version, a `session:` or `directory:` line (see "Cloud Session Identity" below), agent, skillset (Nori profile). Optionally it shows:

| Section       | Condition                                                    | Example                                     |
| ------------- | ------------------------------------------------------------ | ------------------------------------------- |
| Task summary  | `prompt_summary` present                                     | "Task: Fix auth bug"                        |
| Approval mode | `approval_mode_label` present                                | "approvals: Agent"                          |
| Context line  | `context_window_percent` present, with or without token data | "Context 27% (77.0K)" or just "Context 42%" |
| Token totals  | `token_breakdown` has non-zero total                         | "Tokens: 123K total (32.0K cached)"         |

The Tokens section renders if either `token_breakdown` has a non-zero total OR `context_window_percent` is present. This means context window percentage from the live API (`TokenUsageInfo`) can appear even before transcript token data is available.

Task summaries are truncated to 50 characters via `truncate_summary()`, which uses char-level operations (`chars().count()` / `chars().take()`) rather than byte slicing for UTF-8 safety with multi-byte characters.

**Instruction File Discovery (`nori/session_header/mod.rs`):**

The "Instruction Files" block in the startup welcome banner, the `/status` card, and the `/memory` output (`chatwidget/helpers.rs::add_memory_output()` -> `active_instruction_file_contents()`) all funnel through the same discovery pathway: `discover_all_instruction_files()` -> `discover_all_instruction_files_with_paths(cwd, agent_kind, home_dir, managed_policy_dir)`. The active subset of those files is what the agent will actually load, so the displayed list must mirror each agent's documented inheritance rules instead of using a single shared rule.

The agent kind is inferred by `detect_agent_kind()` from the configured agent/model string ("claude*", "codex*", "gemini\*"). The set of directories searched for instruction files is then chosen per agent:

| Agent   | Search range                                                            | Fallback when no `.git` is found |
| ------- | ----------------------------------------------------------------------- | -------------------------------- |
| Claude  | Full ancestor chain from cwd up to filesystem root (no git-root cutoff) | n/a -- always walks to root      |
| Codex   | cwd up to the nearest `.git` ancestor                                   | cwd only                         |
| Gemini  | cwd up to the nearest `.git` ancestor                                   | cwd only                         |
| Unknown | cwd up to the nearest `.git` ancestor                                   | cwd only                         |

The `.git` ancestor check uses the same `@/nori-rs/tui/src/git_marker.rs::is_git_marker()` helper as effective CWD refreshes: a worktree `.git` file or a repository `.git` directory with `HEAD` marks a real root, while an empty `.git` directory does not change the search range.

Claude's behavior follows Claude Code's documented memory loader (https://code.claude.com/docs/en/memory). Walking only to the git root would underreport which CLAUDE.md files Claude will actually load (e.g. with cwd `/tmp/bar/baz`, a `CLAUDE.md` at `/tmp/bar/.claude/` would be missed), so the displayed list would not match what the agent sees.

In each search directory, the discoverer probes for `CLAUDE.md`, `CLAUDE.local.md`, `AGENTS.md`, `AGENTS.override.md`, `GEMINI.md`, and `.claude/CLAUDE.md`. Two extra passes layer in user-level and system-level configs:

- Home-config pass: `~/.claude/CLAUDE.md`, `~/.codex/AGENTS.md`, `~/.gemini/GEMINI.md`.
- Managed-policy pass (Claude only): platform-specific system-wide CLAUDE.md (`/etc/claude-code/CLAUDE.md` on Linux, `/Library/Application Support/ClaudeCode/CLAUDE.md` on macOS, `C:\Program Files\ClaudeCode\CLAUDE.md` on Windows) chosen by `default_managed_policy_dir()`.

The final list is concatenated lowest-precedence first (managed-policy, then home, then ancestor walk) and then deduplicated by absolute path so a file reachable through more than one pass appears exactly once.

After discovery, an activation pass marks each file `active` for the current agent:

| Agent   | Files marked active                                                                                                                                                                             |
| ------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Claude  | `.claude/CLAUDE.md`, `CLAUDE.md`, `CLAUDE.local.md` (all of them, anywhere they appear)                                                                                                         |
| Codex   | Per directory: `AGENTS.override.md` if present, else `AGENTS.md`. `dirs_with_override` tracks which directories had an override so the sibling `AGENTS.md` in the same directory is suppressed. |
| Gemini  | `GEMINI.md` only (no hidden variants, no overrides)                                                                                                                                             |
| Unknown | nothing is active                                                                                                                                                                               |

Token counts are computed only for active files (via `count_tokens()` from `nori/token_count.rs`), so inactive files render dim and contribute nothing to the per-section total in the status card.

Tests inject fake home and managed-policy directories through `discover_all_instruction_files_with_paths()` (and the test-only `discover_all_instruction_files_with_home()` wrapper) to avoid touching real filesystem locations. In debug builds, setting `NORI_MOCK_INSTRUCTION_FILES=1` short-circuits discovery and returns a single fixed entry so E2E snapshots stay stable across machines.

**Skillset Switching (`nori/skillset_picker.rs`):**

The `/switch-skillset` command integrates with the external `nori-skillsets` CLI tool to manage skillsets:

1. Checks if `nori-skillsets` is available in PATH
2. If not available, shows a message prompting the user to install it with `npm i -g nori-skillsets`
3. If available, runs `nori-skillsets list` to get available skillsets
4. On success (exit code 0), displays a searchable picker (`is_searchable: true`) with skillset names. Each `SelectionItem` sets `search_value` to the skillset name so the picker's search filtering can match against it. In vim mode, users press `/` to start filtering; in non-vim mode, typing immediately filters. When `skillset_per_session` is enabled, a "No Skillset" option is prepended to the list; selecting it sends `AppEvent::SkillsetPickerDismissed` (same as Escape/Ctrl-C dismiss), giving users an explicit way to skip skillset selection.
5. On selection, if an `install_dir` is set (worktree context), runs `nori-skillsets --non-interactive switch <NAME> --install-dir <path>`; otherwise runs `nori-skillsets --non-interactive install <NAME>`. The `--non-interactive` flag is required because the TUI captures stdout/stderr via `.output()` and provides no stdin, so any interactive prompt would hang indefinitely.
6. Shows the install output as a confirmation message (for long output, extracts the last section after double newlines)
7. On successful switch/install, triggers a system info refresh (via `request_system_info_refresh()`) so the footer updates with the new active skillset list from `nori-skillsets list-active`

**Argument shortcut:** `/switch-skillset <name>` (e.g., `/switch-skillset foobar`) bypasses the picker entirely and directly triggers the install or switch. This is intercepted in `submit_user_message()` in `chatwidget/user_input.rs` before the text is sent to the model, following the same `strip_prefix` + early-return pattern used by `/login <agent>`. The handler `handle_switch_skillset_command_with_name()` in `chatwidget/pickers.rs` performs the same worktree/per-session detection as the picker flow but skips the async list step, calling `on_switch_skillset_request()` or `on_install_skillset_request()` directly. An empty name after the prefix (e.g., `/switch-skillset ` with trailing space only) is not intercepted and falls through to normal message submission.

The worktree context is detected by `handle_switch_skillset_command()`: if the cwd's parent directory is named `.worktrees`, the cwd is passed as `install_dir`. When `skillset_per_session` is enabled, the cwd is used as `install_dir` even when not in a worktree. This enables per-worktree or per-session skillset installation.

When `skillset_per_session` is enabled in `NoriConfig`, the skillset picker is automatically triggered at startup in `App::run()`, regardless of whether the session is in a worktree. The agent spawn is deferred (`ChatWidgetInit::deferred_spawn = true`) so that `nori-skillsets switch` can write `.claude/CLAUDE.md` to disk before the agent reads it. During the deferred period, a dummy channel is created in `constructors.rs` so the widget has a valid `op_tx`. The real agent spawns after the user picks a skillset (`SkillsetSwitchResult` triggers `spawn_deferred_agent()`). If the user dismisses the picker without selecting a skillset (Escape/Ctrl-C or choosing the "No Skillset" option), the `AppEvent::SkillsetPickerDismissed` event triggers `spawn_deferred_agent()` -- the agent starts without a skillset, behaving as if the feature were disabled. The `deferred_spawn` flag on `ChatWidgetInit` causes a dummy op channel to be created during construction; the real agent spawns after the user picks a skillset or dismisses the picker. The same deferral machinery is reused by picker-first cloud entry: `Cli.cloud_session_picker` forces `deferred_spawn` on regardless of `skillset_per_session`, and the deferred spawn is resolved by the session picker instead of the skillset picker (see "Picker-First Cloud Entry").

When `skillset_per_session` is on and `auto_worktree` is `Off`, the picker subtitle changes from "Switching skillset in {dir}" to "Warning: skillset files will be added to {dir}" to warn that skillset files will be written directly to the current working directory (no worktree isolation). The `on_skillset_list_result()` method in `pickers.rs` loads `NoriConfig` to determine both the `show_no_skillset` flag (true when `skillset_per_session` is enabled) and the `auto_worktree_off` flag (true when per-session is on and `auto_worktree` is not enabled).

Events: `AppEvent::SkillsetListResult` (carries `install_dir: Option<PathBuf>`), `AppEvent::InstallSkillset`, `AppEvent::SwitchSkillset`, `AppEvent::SkillsetInstallResult`, `AppEvent::SkillsetSwitchResult`, `AppEvent::SkillsetPickerDismissed`, `AppEvent::OpenSkillsetPerSessionWorktreeChoice`

The "Per Session Skillsets" toggle in `/settings` is built in `nori/config_picker.rs`. Toggling it on emits `AppEvent::OpenSkillsetPerSessionWorktreeChoice`, which opens a worktree choice modal (`skillset_worktree_choice_params()`) letting the user choose between "With Auto Worktrees" (sets `auto_worktree` to `Automatic`) and "Without Auto Worktrees". Toggling it off emits `AppEvent::SetConfigSkillsetPerSession`, handled in `app/config_persistence.rs` via `persist_skillset_per_session_setting()` to write `skillset_per_session` under `[tui]` in `config.toml`.

The "Auto Worktree" item in `/settings` uses a sub-picker pattern (matching Notify After Idle / Script Timeout): selecting the config item emits `AppEvent::OpenAutoWorktreePicker`, which opens a second selection view listing all `AutoWorktree` variants (`Automatic`, `Ask`, `Off`) with radio-select style (current variant marked). The config item's display name shows the current mode in parentheses (e.g. "Auto Worktree (automatic)"). Selecting a variant emits `AppEvent::SetConfigAutoWorktree(variant)`, persisted via `persist_auto_worktree_setting()` which writes the string value (e.g. `"automatic"`, `"ask"`, `"off"`) to `[tui]` in `config.toml`.

Active skillset display in the footer is driven entirely by `SystemInfo.active_skillsets`, which is populated by shelling out to `nori-skillsets list-active`. After a successful skillset switch or install, `request_system_info_refresh()` triggers a background re-collection so the footer reflects the updated state. There is no in-memory override -- `nori-skillsets list-active` is the single source of truth.

**Notification Configuration:**

Three notification settings are toggled via `/settings` and persisted to the `[tui]` section of `config.toml`:

- **Terminal Notifications** (`TerminalNotifications` enum from `@/nori-rs/nori-config/src/types/mod.rs`): Controls OSC 9 escape sequences. The ACP config value flows through `codex-core`'s `Config::tui_notifications` as a `bool`, and `chatwidget/user_input.rs::notify()` gates on that bool.
- **OS Notifications** (`OsNotifications` enum from `@/nori-rs/nori-config/src/types/mod.rs`): Controls native desktop notifications via `notify-rust`. Passed as `os_notifications` in `AcpBackendConfig` and read in `backend/mod.rs` to set the `use_native` flag on `UserNotifier`.
- **Notify After Idle** (`NotifyAfterIdle` enum from `@/nori-rs/nori-config/src/types/mod.rs`): Controls how long after the agent goes idle before a notification is sent. Unlike the toggle-style notification settings, this uses a sub-picker pattern (like agent picker) where selecting the config item opens a second selection view with radio-select style options (5s, 10s, 30s, 1 minute, Disabled). The selected value flows through `AcpBackendConfig` to `backend.rs` where it controls the idle timer spawn behavior.

Config changes for terminal and OS notifications emit `AppEvent::SetConfigTerminalNotifications` or `AppEvent::SetConfigOsNotifications`, handled in `app/config_persistence.rs` via `persist_notification_setting()`. The notify-after-idle setting uses a separate flow: `AppEvent::OpenNotifyAfterIdlePicker` opens the sub-picker, and `AppEvent::SetConfigNotifyAfterIdle` persists the chosen value via `persist_notify_after_idle_setting()`. All settings are written to the `[tui]` section of `config.toml`.

**Custom Prompt Script Execution:**

When a user invokes a `Script`-kind custom prompt (`.sh`, `.py`, `.js` files discovered from `~/.nori/cli/commands/`), the TUI follows an async execution pattern:

```
ChatComposer (Enter key)           app/mod.rs                       nori_harness::custom_prompts
       |                              |                                |
       |-- AppEvent::ExecuteScript -->|                                |
       |                              |-- execute_script(prompt, args, timeout) -->
       |                              |                                |
       |                              |<-- Ok(stdout) / Err(msg) ------|
       |                              |
       |<-- ScriptExecutionComplete --|
       |     (queued as user message) |
```

The composer intercepts Script-kind prompts in two places: when a command popup selection is confirmed, and when the user types a `/prompts:<name>` command directly and presses Enter. In both cases, positional arguments are extracted via `extract_positional_args_for_prompt_line()` and the `ExecuteScript` event is dispatched. The composer is cleared immediately.

In `app/event_handling.rs`, the `ExecuteScript` handler shows an info message ("Running script..."), spawns a tokio task that calls `nori_harness::custom_prompts::execute_script()` (see `@/nori-rs/harness/src/custom_prompts.rs`) with the configured `script_timeout` from `NoriConfig`, and on completion sends `ScriptExecutionComplete`. On success, the stdout is submitted as a user message via `queue_text_as_user_message()`. On failure, an error message is displayed and the error context is also submitted as a user message so the agent can see it.

The script timeout is configurable via `/settings` -> "Script Timeout" which opens a sub-picker (same pattern as Notify After Idle). The sub-picker is built by `script_timeout_picker_params()` in `@/nori-rs/tui/src/nori/config_picker.rs` and uses `AppEvent::OpenScriptTimeoutPicker` / `AppEvent::SetConfigScriptTimeout` events for the two-step flow. The setting is persisted to `[tui]` in `config.toml` via `persist_script_timeout_setting()`.

**Configurable Hotkeys:**

Keyboard shortcuts are configurable through the `/settings` panel ("Hotkeys" item) and persisted under `[tui.hotkeys]` in `config.toml`. The implementation is split across two layers:

- **Config layer** (`@/nori-rs/nori-config/src/types/mod.rs`): Defines `HotkeyAction`, `HotkeyBinding`, and `HotkeyConfig` as terminal-agnostic string-based types. No crossterm dependency.
- **TUI layer** (`@/nori-rs/tui/src/nori/hotkey_match.rs`): Converts `HotkeyBinding` strings to crossterm `KeyEvent` matches via `parse_binding()` and `matches_binding()`. Also provides `key_event_to_binding()` for the reverse direction (capturing a key press as a binding string).

The `App` struct holds a `hotkey_config: HotkeyConfig` field loaded at startup. In `handle_key_event()` (`app/event_handling.rs`), configurable hotkeys are checked before the structural `match` block -- if a binding matches, the action fires and returns early. Changes are persisted via `persist_hotkey_setting()` (`app/config_persistence.rs`) which uses `ConfigEditsBuilder` to write to `[tui.hotkeys]` and updates the in-memory `HotkeyConfig` for immediate effect.

Hotkey actions fall into two categories that are consumed at different layers:

| Category    | Actions                                                                                                                                                                                                           | Consumed By                                 |
| ----------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------- |
| App-level   | OpenTranscript, OpenEditor, TogglePlanDrawer                                                                                                                                                                      | `app/event_handling.rs::handle_key_event()` |
| Editing     | MoveBackwardChar, MoveForwardChar, MoveBeginningOfLine, MoveEndOfLine, MoveBackwardWord, MoveForwardWord, DeleteBackwardChar, DeleteForwardChar, DeleteBackwardWord, KillToEndOfLine, KillToBeginningOfLine, Yank | `textarea/mod.rs::input()`                  |
| UI triggers | HistorySearch                                                                                                                                                                                                     | `chat_composer/key_handling.rs`             |

Editing hotkeys are propagated from `App` down to the textarea via a `set_hotkey_config()` chain: App -> ChatWidget -> BottomPane -> ChatComposer -> TextArea. This propagation occurs at startup, after config changes via `persist_hotkey_setting()`, and when new sessions or agent switches create fresh ChatWidgets.

The textarea's `input()` method processes key events in three priority stages: (1) C0 control character fallbacks for terminals that send raw control codes without modifier flags, (2) configurable bindings checked via `matches_binding()` against the propagated `HotkeyConfig`, and (3) remaining hardcoded bindings (character insertion, Enter, arrow keys, Home/End, etc.).

The hotkey picker (`@/nori-rs/tui/src/nori/hotkey_picker.rs`) implements `BottomPaneView` directly (not `ListSelectionView`) because rebinding requires raw key capture. It uses a videogame-style rebind flow: select an action, press Enter, press the desired key. Conflicts are resolved by swapping bindings. The `r` key resets the selected action to its default.

**Vim Mode:**

The textarea supports an optional vim-style navigation mode, configured via `/settings` ("Vim Mode" item) which opens a sub-picker (like Auto Worktree) showing three options. The setting is persisted to `config.toml` under `[tui]`:

```toml
[tui]
vim_mode = "newline"  # or "submit" or "off"
```

The `VimEnterBehavior` enum (from `@/nori-rs/nori-config/src/types/mod.rs`) controls both whether vim mode is enabled and how the Enter key behaves:

| Variant   | Enter in INSERT    | Enter in NORMAL | Vim Enabled |
| --------- | ------------------ | --------------- | ----------- |
| `Newline` | Inserts newline    | Submits prompt  | Yes         |
| `Submit`  | Submits prompt     | Inserts newline | Yes         |
| `Off`     | N/A (vim disabled) | N/A             | No          |

The `ChatComposer` stores a `vim_enter_behavior: VimEnterBehavior` field alongside the textarea's own `vim_mode_enabled: bool`. The textarea only cares about on/off (for the vim state machine), while the composer uses the full enum to route Enter key presses at the top of its Enter handler in `key_handling.rs`.

When enabled, the textarea operates in two modes:

| Mode   | Behavior                                                                                                                                                                                           |
| ------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Insert | Default mode. Characters are inserted as typed. Press `Escape` to enter Normal mode; the cursor moves back one position (standard vim behavior), but never past the beginning of the current line. |
| Normal | Navigation and editing mode. Keys are interpreted as commands rather than character input.                                                                                                         |

Normal mode supports standard vim keybindings:

| Category     | Keys                            | Behavior                                                                                                                                  |
| ------------ | ------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------- |
| Navigation   | `h`/`j`/`k`/`l` (or arrow keys) | Move cursor left/down/up/right                                                                                                            |
| Navigation   | `w`/`b`/`e`                     | Forward/backward/end-of-word navigation (`w` moves to start of next word, `b` to start of previous word, `e` to end of current/next word) |
| Navigation   | `0`/`$`/`^`                     | Beginning of line / end of line / first non-whitespace on line                                                                            |
| Navigation   | `G`/`gg`                        | End of text / beginning of text                                                                                                           |
| Insert entry | `i`/`a`                         | Enter Insert at cursor / after cursor                                                                                                     |
| Insert entry | `I`/`A`                         | Enter Insert at beginning of line / end of line                                                                                           |
| Insert entry | `o`/`O`                         | Open new line below/above and enter Insert                                                                                                |
| Editing      | `x`                             | Delete character under cursor                                                                                                             |
| Editing      | `D`/`C`                         | Delete to end of line (`C` also enters Insert mode)                                                                                       |
| Editing      | `dd`                            | Delete current line                                                                                                                       |
| Editing      | `p`                             | Paste from kill buffer                                                                                                                    |
| Undo/Redo    | `u`                             | Undo last edit or insert session                                                                                                          |
| Undo/Redo    | `Ctrl-R`                        | Redo last undone edit or insert session                                                                                                   |

Two-key sequences (`gg`, `dd`) use a `vim_pending_key: Option<char>` field on TextArea. Pressing `g` or `d` sets the pending key; the second keypress either completes the sequence or cancels it (non-matching keys are discarded).

**Undo/Redo with Insert-Session Grouping:**

The textarea maintains undo/redo stacks of `(text, cursor_pos)` snapshots, capped at 500 entries. In vim mode, all edits made during a single insert session (from entering Insert mode to pressing Escape) are grouped into a single undo unit. This matches standard vim behavior where `u` undoes the entire insert session rather than individual keystrokes.

The grouping mechanism uses `begin_undo_group()` / `end_undo_group()`: entering Insert mode (via `i`, `a`, `A`, `I`, `o`, `O`, `C`, `S`) saves a snapshot and sets `in_undo_group = true`, suppressing per-keystroke snapshots. Pressing Escape to return to Normal mode calls `end_undo_group()`. Outside of vim mode (or when `in_undo_group` is false), each mutation via `insert_str_at()` or `replace_range_raw()` saves its own snapshot. `set_text()` clears both stacks since it represents a complete replacement of the buffer content (e.g., history navigation).

The state machine is implemented in `textarea/mod.rs` via the `VimModeState` enum. Vim mode handling runs as "stage 0" in the `input()` method, before C0 control fallbacks, configurable hotkey bindings, and hardcoded bindings. When in Normal mode, `chat_composer/mod.rs` bypasses paste burst detection and sends input directly to the textarea so navigation keys work without interference.

Config changes use two app events: `AppEvent::OpenVimModePicker` opens the sub-picker, and `AppEvent::SetConfigVimMode(VimEnterBehavior)` applies the selection. The setting propagates down the same chain as hotkeys: App -> ChatWidget -> BottomPane -> ChatComposer via `set_vim_mode()`. The ChatComposer updates both its `vim_enter_behavior` field and calls `set_vim_mode_enabled()` on the textarea (passing `is_enabled()`). When vim mode is disabled, the textarea state resets to Insert mode. Persistence is handled by `persist_vim_mode_setting()` in `app/config_persistence.rs`, which writes the `toml_value()` string to the `[tui]` section.

`BottomPane` also stores `vim_mode_enabled: bool` (set by `set_vim_mode()`), which it injects into `SelectionViewParams` whenever `show_selection_view()` is called for a searchable view. This means vim mode affects both the textarea input and the selection popup key handling (see "ListSelectionView Vim-Mode-Aware Search" above).

**History Search (Configurable Hotkey):**

The history search hotkey is configurable via the `HotkeyAction::HistorySearch` binding (default: `Ctrl+R`). The `ChatComposer` key handler uses `matches_binding()` against the configured binding rather than a hardcoded key pattern. This allows users to remap history search when `Ctrl+R` conflicts with other bindings (e.g., vim redo).

In vim Normal mode, `Ctrl+R` is handled by the textarea as redo before the composer's key handler runs, so the default `HistorySearch` binding does not fire. In Insert mode, the composer's key handler runs and opens history search as expected. Users who want history search accessible in Normal mode can rebind it to a different key.

The history search popup follows the same `ActivePopup` pattern as the slash command popup (`Command`) and file mention popup (`File`). The popup is implemented in `history_search_popup.rs` using the shared `ScrollState` and `MAX_POPUP_ROWS` infrastructure from `popup_consts.rs`.

Data flow:

```
History search hotkey pressed in ChatComposer
  -> Op::SearchHistoryRequest { max_results: 500 }
  -> AcpBackend spawns blocking read of history.jsonl via search_entries()
  -> EventMsg::SearchHistoryResponse
  -> ChatWidget -> BottomPane -> ChatComposer::on_search_history_response()
  -> HistorySearchPopup::set_entries()
```

All entries are loaded once when the popup opens; filtering is performed client-side (case-insensitive substring match on each keystroke). The popup manages its own lifecycle -- the post-key-event `sync_command_popup()` / `sync_file_search_popup()` cycle is skipped when `ActivePopup::HistorySearch` is active, preventing those syncs from closing the history popup.

Vim mode is inherited from the composer's current vim state. When vim mode is enabled, the popup starts in Insert mode (for typing search queries) and supports Esc to enter Normal mode (j/k navigation), then a second Esc to close.

**Composer Placeholder Hints:**

When the composer is empty, `ChatWidget` seeds its placeholder from concise capability hints instead of task examples: `?` for the shortcuts overlay, `/` for the slash command menu, `$` for skill listing, `!` for shell commands, and `@` for file mentions. The always-visible `? for shortcuts` footer hint is intentionally omitted; pressing `?` as the first composer character still opens the full shortcut overlay below the prompt, and typing `/` still opens the slash command popup. Prompt-initial modes replace the normal `›` prompt marker with the active sigil (`?`, `/`, or `!`) using terminal-palette colors, and the duplicated leading sigil is hidden from the editable body.

The `!` shell command affordance is a prompt-initial composer mode, not a slash command or popup. Typing `!` into an empty composer stores the marker as mode state and shows the command body after the `!` prompt marker; `current_text()` reconstructs submitted command text as `!{body}`. While this mode is active, `/`, `$`, `@`, and `?` are treated as literal shell text so nested pickers and the shortcut overlay do not open. `Esc`, `Backspace`, or `Enter` with an empty shell body exits the mode without submitting, and recalling a history item that starts with `!` restores shell mode.

**Status Line Footer:**

The footer displays configurable segments, each of which can be enabled/disabled via `/settings` -> "Footer Segments" or via `[tui.footer_segments]` in config.toml:

| Segment        | TOML Key         | Description                                                                                                                                                                                   |
| -------------- | ---------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Task Summary   | `prompt_summary` | "Task: <summary>" (dim) - generated by ACP backend on first user prompt                                                                                                                       |
| Vim Mode       | `vim_mode`       | "NORMAL" (blue/bold) or "INSERT" (green) when vim mode is enabled                                                                                                                             |
| Git Branch     | `git_branch`     | Current branch name with ⎇ symbol (yellow for main repo, orange for worktree)                                                                                                                 |
| Worktree Name  | `worktree_name`  | "Worktree: {name}" (light red) when running in an auto-worktree session -- the immutable directory name, distinct from the git branch which gets renamed after the first prompt               |
| Git Stats      | `git_stats`      | Lines added/removed in current session                                                                                                                                                        |
| Context Window | `context`        | "Context 27% (34K)" when running within an agent environment                                                                                                                                  |
| Approval Mode  | `approval_mode`  | "Approvals: Agent/Full Access/Read Only"                                                                                                                                                      |
| Nori Profile   | `nori_profile`   | "Skillset: name" for one active skillset, "Skillsets: a, b" for multiple, hidden when none are active. Uses `active_skillsets` from `SystemInfo` (populated by `nori-skillsets list-active`). |
| Nori Version   | `nori_version`   | "Skillsets v<version>"                                                                                                                                                                        |
| Token Usage    | `token_usage`    | "Tokens: 123K total (32K cached)" when running within an agent environment                                                                                                                    |
| Mode Indicator | `mode_indicator` | "[ Plan ]" style ACP mode label, shown only when the active ACP agent exposes a mode config option                                                                                            |
| Cloud Session  | `cloud_session`  | "☁ nori-fast-kazunoko-aac8" -- the active cloud session's identity, shown only when the agent has the cloud (live-reattach) lifecycle; self-hides for local sessions                          |

Example config.toml to disable specific segments and opt in to ones that are off by default:

```toml
[tui.footer_segments]
token_usage = false
git_stats = true
vim_mode = true
```

`FooterSegmentConfig::default()` (in `@/nori-rs/nori-config/src/types/mod.rs`) ships a lean subset enabled by default: `context`, `git_branch`, `worktree_name`, `approval_mode`, `token_usage`, `mode_indicator`, and `cloud_session`. The remaining segments -- `prompt_summary`, `vim_mode`, `git_stats`, `nori_profile`, and `nori_version` -- are off by default and require an explicit `[tui.footer_segments]` opt-in. `FooterSegmentConfig::from_toml` delegates to `Self::default()` for unspecified fields, keeping the two sources of defaults in lockstep. Individual segments still render only when their backing data exists, so an enabled segment with no data stays invisible -- `cloud_session` is enabled by default but only ever renders on a cloud (live-reattach) session.

**Cloud Session Identity** (`chatwidget/helpers.rs`, `chatwidget/constructors.rs`, `chatwidget/event_handlers.rs`, `nori/session_header/mod.rs`, `bottom_pane/footer.rs`): `ChatWidget` stores `acp_session_id: Option<String>` (from `SessionConfiguredEvent.acp_session_id`, which the harness populates ONLY for cloud live-reattach agents -- `None` for local agents, see `@/nori-rs/harness/docs.md`) and `cloud_session_title: Option<String>` (the broker-reported title forwarded by the resume picker through `AppEvent::ResumeAcpSession { title, .. }`). `ChatWidget::cloud_session_identity()` returns `Some(CloudSessionInfo { id, title })` whenever an id is known -- id presence IS the cloud signal (the harness never names local sessions), so no capability gate is applied; gating on capabilities would race their delivery against `SessionConfigured` and could silently drop the session line from the immutable welcome card. `refresh_cloud_session_indicator()` pushes the identity into `BottomPane::set_cloud_session()` from `SessionConfigured` (the only event that changes the id), which drives the `CloudSession` footer segment above. The same identity feeds `NoriSessionHeaderCell`: when present, the welcome banner and `/status` card render a `session: <id> (<title>)` line in place of the local `directory:` line (which would otherwise show a meaningless local cwd for a session running on a remote VM); when absent, the ordinary `directory:` line renders. The reattach info message built by `reattach_info_message()` in `app/event_handling.rs` follows the same rule: a live-reattach resume shows "Reattaching to `<id> (<title>)` -- earlier messages stay in the cloud session (not replayed here)." (title omitted when unknown), while a non-cloud resume shows the generic "Resuming session with `<agent>`...".

Segment placement is configurable through `[tui.footer_layout]`. Missing layout fields use defaults: legacy status segments render on `footer_left`, and `mode_indicator` renders on `footer_right`. A field that is present replaces that placement; listed segments are moved out of other default placements so a partial override can move one segment without duplicating it. The layout supports `footer_left`, `footer_right`, `textarea_top_left`, `textarea_top_right`, `textarea_bottom_left`, and `textarea_bottom_right`.

Example config.toml to move the mode indicator into the textarea's top-right corner:

```toml
[tui.footer_layout]
textarea_top_right = ["mode_indicator"]
```

Token data flows from `TranscriptLocation.token_breakdown` (provided by `nori_harness::discover_transcript_for_agent_with_message()`) through `FooterProps` to the footer renderer. The breakdown includes separate input, output, and cached token counts for accurate usage reporting.
Footer context usage is sourced in priority order: ACP `SessionUpdateInfo { kind: Usage, usage: Some(..) }` updates drive the footer when available, while `TranscriptLocation.token_breakdown` remains the provider-specific fallback for older sessions or agents that do not emit ACP usage updates.

The prompt summary flows from the ACP backend as an `EventMsg::PromptSummary` event, handled by `ChatWidget::on_prompt_summary()`, which propagates it down: `ChatWidget` -> `BottomPane::set_prompt_summary()` -> `ChatComposer::set_prompt_summary()` -> `FooterProps.prompt_summary` -> `segments_for()` renderer.

The harness session runtime (`@/nori-rs/harness/src/runtime.rs`) detects the repo root for auto-worktree branch renaming by inspecting the cwd path structure: when `auto_worktree.is_enabled()` (true for both `Automatic` and `Ask` variants) and the cwd's parent directory is named `.worktrees`, the grandparent is treated as the repo root. This value is passed as `auto_worktree_repo_root` in `AcpBackendConfig`. The branch rename is fire-and-forget; the working directory does not change during a session, so the TUI does not need to handle directory changes.

**External Editor Integration (`editor.rs`):**

The external editor hotkey (default Ctrl-G, configurable via hotkeys) opens the user's preferred text editor for composing prompts. The editor is resolved from `$VISUAL` > `$EDITOR` > platform default (`vi` on Unix, `notepad` on Windows). The lifecycle in `app/session_setup.rs::open_external_editor()`:

1. Reads current composer text via `ChatWidget::composer_text()`
2. Writes content to a temp file (`nori-editor-*.md`)
3. Suspends the TUI via `tui::restore()`
4. Spawns the editor synchronously (blocking) via shell delegation (`sh -c` on Unix, `cmd /C` on Windows)
5. Re-enables the TUI via `tui::set_modes()`
6. On success, reads the temp file content back into the composer; on failure or non-zero exit, discards changes

This uses the same terminal suspend/resume pattern as job control in `lib.rs` (SIGTSTP handling).

**File Browsing (`/browse`):**

The `/browse` slash command launches a configurable terminal file manager in chooser mode, then opens the selected file in the user's editor. It is available during task execution. The flow in `app/session_setup.rs::browse_files()`:

1. Creates a temp file (`nori-browse-*.txt`) for the file manager to write the chosen path into
2. Suspends the TUI via `tui::restore()`
3. Spawns the file manager with chooser-mode arguments (from `FileManager::chooser_args()` in `@/nori-rs/nori-config/src/types/mod.rs`)
4. On success, reads the first line of the temp file as the selected path
5. If the selected path is a file, opens it in the user's editor using the same `editor::resolve_editor()` / `editor::spawn_editor()` as Ctrl-G
6. Re-enables the TUI via `tui::set_modes()`

When `/browse` is invoked, `SlashCommand::Browse` dispatches by loading `NoriConfig` to check `file_manager`. If `None`, an error message directs the user to `/settings`. If set, it sends `AppEvent::BrowseFiles(fm)`.

The file manager setting is configurable via `/settings` -> "File Manager" which opens a sub-picker (same pattern as auto worktree). The sub-picker is built by `file_manager_picker_params()` in `@/nori-rs/tui/src/nori/config_picker.rs` and uses `AppEvent::OpenFileManagerPicker` / `AppEvent::SetConfigFileManager` events for the two-step flow. The setting is persisted to `[tui]` in `config.toml` via `persist_file_manager_setting()`.

**View-Only Transcript Viewing:**
The `/resume-viewonly` command allows viewing previous session transcripts without replaying the conversation. Implementation in `@/nori-rs/tui/src/`:

- `viewonly_transcript.rs`: Converts `nori_harness::transcript::Transcript` entries to `ViewonlyEntry` enum (User, Assistant, Thinking, Info variants)
- `nori/viewonly_session_picker.rs`: Session picker UI for selecting past sessions; also defines `SessionPickerInfo` (shared with `/resume` picker)
- `app/session_setup.rs::display_viewonly_transcript()`: Renders entries in the chat history

Rendering behavior:

- User messages display via `UserHistoryCell` with standard user styling
- Assistant messages render via `AgentMessageCell` with `append_markdown()` for syntax highlighting
- Thinking blocks display with dimmed styling (matching live reasoning display)
- Tool calls, tool results, and patch operations are skipped to focus on conversation content
- Blank line separators between entries improve readability

The async flow uses three AppEvents: `ShowViewonlySessionPicker` -> `LoadViewonlyTranscript` -> `DisplayViewonlyTranscript`.

**Startup Session Resume (`nori resume`):**

The top-level `nori resume` subcommand enters the TUI with an existing transcript session already selected. This path is handled before `App::run()` constructs the chat widget:

```
nori resume [session-id]
    |
    v
run_main() -> run_ratatui_app()
    |  (resolves metadata by ID, --last, or startup picker)
    v
ResumeSelection::Resume(ResumeTarget)
    |  (loads full Transcript, extracts acp_session_id as Option<String>)
    v
ChatWidget::new_resumed_acp(init, acp_session_id, transcript)
    |
    v
spawn_acp_agent_resume() -> launch_session(resume) -> AcpBackend::resume_session()
```

Selection behavior:

- `nori resume <session-id>` searches all transcript projects for that exact session ID.
- `nori resume --last` chooses the newest transcript for the current working directory; `--all` removes the cwd filter.
- `nori resume` opens `resume_picker/`, which lists metadata-only transcript rows and returns a `ResumeTarget`.
- `--agent` is optional. When omitted, the recorded `session_meta.agent` is used. When present, it must match the recorded agent or startup fails with a clear error.

The startup picker in `@/nori-rs/tui/src/resume_picker/` is transcript-backed. It loads its rows in a single one-shot pass through `TranscriptLoader::list_resumable_session_metadata()` and keeps them lightweight by reading only `session_meta` lines before selection. It does not perform provider-specific rollout discovery, and it has no background page-loading or load-more machinery -- that pagination scaffolding was removed as inert once the picker moved to one-shot `TranscriptLoader` loading.

Resume hints use the shared `RESUME_HINT_LEAD` and `resume_command_for_conversation()` helpers from `app/` so the in-TUI new-conversation summary and the post-exit CLI output stay aligned. Both surfaces put the copyable `nori resume <session-id>` command on its own line after the `run:` lead text.

**Session Resume (`/resume`):**

The `/resume` command allows reconnecting to a previous ACP session. It uses the ACP agent's `session/load` RPC (history replay) or `session/resume` RPC (live reattach) when the agent advertises one of them, and otherwise falls back to a fresh ACP session plus normalized replay derived from the saved transcript (see `@/nori-rs/harness/docs.md`).

The picker list itself comes from one of two sources. `ChatWidget::open_resume_session_picker()` has a capability-gated branch: when the agent advertises `session/list` plus at least one resume mechanism (`session_list && (load_session || session_resume)`) and an `acp_handle` exists, it spawns an async task that calls `handle.list_sessions(cwd)` on the live agent (via the `ListSessions { cwd, response_tx }` `AcpAgentCommand`) and emits `AppEvent::ShowAcpResumeSessionPicker { sessions }` with the agent's own `AcpSessionSummary` rows. An empty result inserts a "no resumable sessions" error cell and a list failure inserts an error cell instead of opening a picker. Otherwise it falls back to the existing local-transcript picker (`AppEvent::ShowResumeSessionPicker`) described below. A resume mechanism is required in addition to `session_list` because resuming an agent-sourced row passes `transcript: None` and depends entirely on the agent-side session -- either `session/load` replay or `session/resume` live reattach; with neither, an agent would silently start a blank session, so such agents fall through to the transcript-backed picker. The `session/resume` arm is what makes the picker work for the `nori cloud` agent, which advertises `list`/`resume`/`close` with `loadSession: false`. The capabilities are raw agent-capability projections sourced from `@/nori-rs/harness`; this is generic to any agent that advertises them, not Nori/Codex-specific. Selecting an agent-sourced row emits `AppEvent::ResumeAcpSession { acp_session_id, title }` (`title` is the row's broker-reported title, when known), whose handler shuts down the current conversation and starts a new resumed ACP chat widget via `new_resumed_acp(init, Some(acp_session_id), title, None)` -- there is no local transcript, so the harness's capability-based strategy selection (`session/load` replay or `session/resume` reattach) rehydrates or reattaches the session. The handler's info message, built by `reattach_info_message()`, branches on the outgoing widget's capability view (see "Cloud Session Identity" below for the exact wording). `show_acp_resume_session_picker()` builds the modal via `acp_resume_session_picker_params()` in `@/nori-rs/tui/src/nori/resume_session_picker.rs`, mapping each summary to a row (name = title falling back to session_id, description = relative time plus cwd, action = `ResumeAcpSession`) and sorting rows most-recent-first by `updated_at` (a stable sort, so rows with a missing or unparseable timestamp fall after every dated row but keep the agent's relative order among themselves). The first row is always a pinned "Start a new session" action that emits `AppEvent::NewSession` -- present even when the agent reports no sessions -- so entering the picker never has to claim a session implicitly; creating one is a deliberate pick. Cloud sessions have no real working directory, so the picker hides a row's `cwd` from both the description and the search haystack whenever the row's ACP `_meta` extension marks it with `_meta.nori.origin == "cloud"` (surfaced on `AcpSessionSummary.meta`, see `@/nori-rs/acp-host/src/connection/docs.md`). A legacy transition shim also still treats the bare sentinel cwd `/` as cloud-origin for agents that have not started emitting `_meta` yet; that shim is deleted once every such agent does.

**Builtin Command Scoping** (`slash_command.rs`, `chatwidget/goal.rs`, `chatwidget/key_handling.rs`):

Every builtin slash command has a `CommandScope`: `LocalOnly` (meaningless once the agent runs on a remote VM -- `/switch-skillset`, `/browse`, `/diff`, `/browser`), `CloudOnly` (needs `session/close` -- only `/close`), or `Universal` (everything else, including `/quit`/`/exit`, which must never be disabled). `ChatWidget::builtin_command_availability()` merges this client-side scope verdict with the server-sent `SessionCapabilitiesChanged` availability map: an explicit server disable (with its reason) always wins; otherwise `scope_unavailable_reason()` derives a reason from `CommandScope` and the live `AgentCapabilitiesView` (`LocalOnly` checks `agent.live_reattach()`, `CloudOnly` checks `agent.session_close`). This merged verdict is recomputed on both `SessionCapabilitiesChanged` (refreshing every command's popup greying via `SlashCommand::iter()`, not just the commands the server mentioned) and read fresh at dispatch time, so cold-start defaults are correct even before any capability snapshot arrives (`session_close` defaults to false, so `/close` starts unavailable; `live_reattach()` defaults to false, so local-only commands start usable).

`ChatWidget::dispatch_command()` in `key_handling.rs` runs every command except `/quit`/`/exit` through one unified `ensure_builtin_command_enabled()` gate before the command's `match` arm; a disabled command renders an error cell with the merged reason and returns without dispatching. This replaced a bespoke capability check inside the `/close` arm (and inside `/goal`'s handler) with the same popup-greying-plus-dispatch-time-gate mechanism every scoped command uses.

**Session Close (`/close`):**

The `/close` command releases the current agent-side session over ACP `session/close` and, on success, returns to the agent-sourced session picker. Availability is decided entirely by the unified gate above (`CommandScope::CloudOnly`, unavailable until the agent advertises `session/close`); the handler in `@/nori-rs/tui/src/chatwidget/key_handling.rs` itself only calls `AcpAgentHandle::close_session()` (the `CloseSession` `AcpAgentCommand`, handled by `AcpBackend::close_active_session()` in `@/nori-rs/harness`). Only after the close succeeds does it emit `AppEvent::SessionClosed`; a failure comes back as `AppEvent::SessionCloseFailed`, which renders an error cell and keeps the current session intact. The `SessionClosed` handler in `@/nori-rs/tui/src/app/event_handling.rs` shuts down the conversation, builds a fresh deferred-spawn `ChatWidget`, and re-runs the pre-session probe via `begin_agent_session_picker()` -- it deliberately does NOT emit `NewSession`: on a cloud agent an automatic new session would silently claim a brand-new VM the user never asked for (the old "swap" semantics are gone). While the close is in flight the widget sets `session_close_in_flight`, blocking session-switching commands (`/new`, `/resume`, `/resume-viewonly`, `/agent`, `/close`) so the deferred follow-up cannot clobber a conversation the user switched to mid-close. For the `nori cloud` agent this is the only terminal verb -- the agent enforces a one-active-session contract, so closing is how a user frees the slot before resuming or creating another. `/close` is not available during a task (`available_during_task = false`).

**Picker-First Cloud Entry (`nori cloud`):**

`nori cloud` boots into the agent-sourced session picker before any session exists, so nothing can claim a VM until the user explicitly picks a row. The flow rides an internal flag, not a public CLI flag:

```
nori cloud (cli/src/main.rs sets TuiCli.cloud_session_picker = true)
    |
    v
run_main() -> App::run(..., cloud_session_picker)
    |  (forces the same deferred-spawn machinery as skillset_per_session:
    |   ChatWidgetInit::deferred_spawn = true, dummy op channel)
    v
App::begin_agent_session_picker()  (app/session_setup.rs)
    |  (background task: nori_harness::backend::probe::probe_agent_sessions_for)
    v
AppEvent::AgentSessionListProbed { probe }
    |
    ├── Ok: seed the deferred widget's capability view from the probe
    |       (SessionCapabilitiesChanged), then show_acp_resume_session_picker()
    |       with the probed session/list rows + the pinned "Start a new session" row
    |
    └── Err: error cell ("Couldn't list sessions: ...") then fall back to the
            plain spawn_deferred_agent() path, which has the full error
            handling (auth hints, retry wording)
```

The probe itself lives in the harness (`@/nori-rs/harness/src/backend/probe.rs`): it spawns the agent, completes initialize, reads the capability view, fetches `session/list`, and tears the child down -- it never calls `session/new`, `session/load`, or `session/resume`. Seeding the capability view before any session exists matters because capability-gated behavior (the detach wording on quit, the reattach notice) must be right even while the picker is still open. The same probe-and-picker loop is re-entered after a successful `/close` (see above). Plain `nori` never gets picker-first entry, even against an agent that advertises the full session lifecycle -- the picker boot is a cloud-entry behavior driven by the flag, not a capability reflex.

**Exit Is Detach (`begin_exit`):**

`/quit`, `/exit`, and idle Ctrl+C all route through `ChatWidget::begin_exit()` in `@/nori-rs/tui/src/chatwidget/helpers.rs` instead of submitting `Op::Shutdown` directly. It is idempotent and does four things: shows immediate feedback (on agents with the cloud lifecycle -- `session_resume && !load_session` -- the message is "Exiting -- detaching; this session keeps running in the cloud." with a reattach hint pointing at the `nori cloud` picker; otherwise a plain "Exiting…"), sets the widget's `exiting` flag, submits `Op::Shutdown` for graceful backend teardown, and spawns a hard-exit watchdog that force-sends `AppEvent::ExitRequest` after `EXIT_HARD_DEADLINE` (~1s). The watchdog exists because the backend's child-exit grace can be as long as 25 seconds; post-sessions-#1276 the cloud agent treats connection EOF as a non-terminal detach (`session/close` is the only terminal verb), so there is nothing worth holding the TUI hostage for -- a cooperative child still finishes its EOF path, and a stuck one gets SIGKILLed by the connection layer's grace logic after the TUI is gone. While `exiting` is set, `submit_user_message()` in `@/nori-rs/tui/src/chatwidget/user_input.rs` drops prompts (a fast typist must not start another turn mid-teardown) and `dispatch_command()` in `@/nori-rs/tui/src/chatwidget/key_handling.rs` refuses every slash command except repeated `/quit`/`/exit`.

The transcript-backed flow involves three layers:

```
SlashCommand::Resume
    |
    v
ChatWidget::open_resume_session_picker()
    |  (async: loads first-line session metadata, filters by agent)
    v
AppEvent::ShowResumeSessionPicker -> resume_session_picker modal
    |  (background task lazily streams first-user previews and user-turn counts)
    v
AppEvent::ResumeSessionSummaryReady -> update active picker row
    |  (user selects session)
    v
AppEvent::ResumeSession { nori_home, project_id, session_id }
    |  (loads full Transcript, extracts acp_session_id as Option<String>)
    v
App::shutdown_current_conversation()
    |
    v
ChatWidget::new_resumed_acp(init, acp_session_id, transcript)
    |
    v
spawn_acp_agent_resume() -> launch_session(resume) -> AcpBackend::resume_session()
```

The `ResumeSession` handler loads the full transcript (not just metadata) via `TranscriptLoader::load_transcript()`. The `acp_session_id` is extracted as `Option<String>` from `transcript.meta.acp_session_id` -- sessions without an `acp_session_id` are still resumable via the normalized replay fallback.

Session filtering: `load_resumable_sessions()` in `@/nori-rs/tui/src/nori/resume_session_picker.rs` loads first-line session metadata for the current working directory via `TranscriptLoader::find_session_metadata_for_cwd()`, filters to only sessions whose `agent` field matches the currently active agent, and returns metadata-only picker rows. It does not read transcript bodies before the picker appears.

Lazy picker summaries: after `ShowResumeSessionPicker` is sent, `ChatWidget::open_resume_session_picker()` starts a background task that first streams each matching transcript until the first user message for preview text, then streams full files to count exact user turns. Counts are user-turn counts (`type=user` entries), not raw transcript line counts, and are hidden until known. Sessions with zero user turns are removed from the active picker once their lazy count completes. Summary update events carry a generation id so stale updates from an older picker open do not mutate a newer picker.

The resume session picker reuses the `SessionPickerInfo` type and `format_relative_time()` utility from `@/nori-rs/tui/src/nori/viewonly_session_picker.rs`. The `format_relative_time` function was made `pub(crate)` for this reuse.

`spawn_acp_agent_resume()` in `@/nori-rs/tui/src/chatwidget/agent.rs` calls the same shared launch path as `spawn_agent()` but sets `SessionLaunchSpec.resume` to a `SessionResume` carrying the optional `acp_session_id` and an `Option<Transcript>` (the transcript-backed `/resume` and `nori resume` paths supply `Some`; the agent-sourced `session/list` path supplies `None` and relies on server-side replay); the harness runtime then calls `AcpBackend::resume_session()` instead of `AcpBackend::spawn()`. Both spawn paths receive a single `SessionEvent` stream from `nori_harness::runtime`: normalized `ClientEvent` items drive ACP session rendering, while `Control` events still carry shared app-level concerns such as `SessionConfigured`, warnings, and shutdown.

**Agent Connection Lifecycle & Failure Recovery:**

Agent registration validation is performed exclusively in `spawn_agent()` (`chatwidget/agent.rs`). When the configured model is not in the ACP registry, `spawn_agent()` routes to `spawn_error_agent()` which sends `AppEvent::AgentSpawnFailed` -- triggering `on_agent_spawn_failed()` to display the error and reopen the agent picker for recovery. There is no early validation in `App::run()`; this single validation point ensures that unregistered agents (including custom agents that were configured but later removed) always get graceful recovery through the agent picker rather than a fatal startup error.

When the user selects an agent (or resumes a session), the TUI shows a "Connecting to [Agent]" status indicator via `ChatWidget::show_connecting_status()` (emitted from `chatwidget/agent.rs` as `AppEvent::AgentConnecting` before launching). The connection race itself lives in the harness: `launch_session()` in `@/nori-rs/harness/src/runtime.rs` uses a `tokio::select!` to race backend initialization against shutdown requests and a two-phase timeout, and the TUI's event-forwarding task in `chatwidget/agent.rs` maps the resulting `SessionEvent`s onto `AppEvent`s:

| Runtime outcome                  | Trigger                                                                 | TUI action                                                        |
| -------------------------------- | ----------------------------------------------------------------------- | ------------------------------------------------------------------ |
| `SessionEvent::Backend`          | `AcpBackend::spawn()` / `resume_session()` returns `Ok`, events flowing | Forwards as `AppEvent::CodexEvent` / `AppEvent::ClientEvent`       |
| `SessionEvent::SpawnFailed`      | Init returns `Err`, or the 8s-warning + 30s-abort timeout elapses       | Sends `AppEvent::AgentSpawnFailed`                                 |
| `SessionEvent::ShutdownRequested`| User sends `Op::Shutdown` during connection                             | Sends `AppEvent::ExitRequest`                                      |

`drain_until_shutdown()` (in `@/nori-rs/harness/src/runtime.rs`) reads ops from the channel, discarding everything until it sees `Op::Shutdown`. This allows the user to exit (via `/exit`, Ctrl-C) even while the backend is still attempting to connect. `spawn_timeout_sequence()` provides user feedback: at 8 seconds the runtime emits a `WarningEvent` visible in the chat, and after 30 more seconds it aborts the connection attempt entirely.

`on_agent_spawn_failed()` in `chatwidget/helpers.rs` performs three recovery steps in order:

1. Clears the "Connecting" status indicator via `bottom_pane.hide_status_indicator()`
2. Displays an error message in chat history: "Failed to start agent '{name}': {error}"
3. Reopens the agent picker so the user can select a different agent

**Status Indicator Whimsical Messages (`status_indicator_widget.rs`):**

When the agent begins processing a task, the `StatusIndicatorWidget` displays an animated header. The header is selected by `pick_status_message(custom_working_messages, &custom_working_message_list)`:

- `custom_working_messages = false` → plain `"Working"` label.
- `custom_working_messages = true` (default) and `custom_working_message_list` is empty → randomly samples from `WHIMSICAL_STATUS_MESSAGES` (e.g., "Thinking really hard", "Hallucinating responsibly").
- `custom_working_messages = true` and `custom_working_message_list = ["..."]` → randomly samples from the user's list, overriding the builtin pool.

The same `pick_status_message` helper is used for the initial header in `ChatWidget::new`, in `BottomPane::set_task_running`/`ensure_status_indicator`, in the `/config` toggle's live update, and in `chatwidget::event_handlers::on_task_started` so all task starts respect the user's preference. Users edit the toggle from `[tui].custom_working_messages` (TOML) or the `/config` menu; the user list is TOML-only via `[tui].custom_working_message_list`. The `/config` "Custom Working Messages" entry indicates when a custom list is active so the user is reminded that flipping the toggle preserves their TOML list. During streaming, reasoning chunk headers (extracted from bold markdown text) dynamically replace this initial message via `update_status_header()`.

**Terminal Title Management (`terminal_title.rs`, `chatwidget/helpers.rs`):**

The TUI sets the terminal window/tab title via OSC 0 escape sequences so users can see whether Nori is idle or working at a glance, even when the tab is not focused. The title is written directly to stdout via crossterm's `execute!` macro with a custom `SetWindowTitle` command implementation -- this bypasses the ratatui draw buffer entirely.

When the agent is working (`mcp_startup_status` is present or `bottom_pane.is_task_running()` is true), an animated braille dot-spinner (`SPINNER_FRAMES`, 10 frames at 100ms intervals) appears before the project name in the title bar. When idle, only the project name (derived from `config.cwd`) is shown. The animation is gated on `config.animations` -- when disabled, the spinner is suppressed but the project name still appears.

The animation is demand-driven rather than timer-based: each `refresh_terminal_title()` call schedules the next frame via `FrameRequester::schedule_frame_in(100ms)`, and `pre_draw_tick()` (called before every frame in the `TuiEvent::Draw` handler in `app/event_handling.rs`) advances the spinner only when progress is active. This creates a self-stopping loop -- when progress ends, no further frames are scheduled. Title writes are deduplicated via a `last_terminal_title: Option<String>` cache to avoid redundant OSC writes.

`refresh_terminal_title()` is hooked into `on_session_configured()`, `on_task_started()`, `on_task_complete()`, and `on_mcp_startup_complete()` in `chatwidget/event_handlers.rs`. The title is cleared (set to empty string) on `ChatWidget` drop. The module does not attempt to save or restore the terminal's previous title because that is not portable across terminals.

Title content is sanitized by `sanitize_terminal_title()` which strips control characters, bidi overrides, zero-width characters, and collapses whitespace, with a 240-character cap.

**Exit Path When Backend Is Dead:**

Every error/timeout/shutdown arm in the runtime's `tokio::select!` (`@/nori-rs/harness/src/runtime.rs`) explicitly drops the op receiver before returning. This closes the receiver end of the channel so that the op sender (held by `ChatWidget`) has no listener. If the user then attempts to exit (via `/exit`, `/quit`, or Ctrl-C), `submit_op(Op::Shutdown)` detects the dead channel (the `send()` returns `Err`) and falls back to sending `AppEvent::ExitRequest` directly via `app_event_tx`. This ensures the TUI can always exit cleanly even when no backend is running.

**Loop Mode (Prompt Repetition):**

Loop mode allows the same first prompt to be re-run multiple times, each time in a completely fresh conversation session. This is configured via `/settings` -> "Loop Count" or by setting `loop_count` in `config.toml` (see `@/nori-rs/nori-config/src/types/mod.rs`).

The loop is orchestrated entirely within the TUI layer -- `codex-core` has no awareness of loop semantics:

```
User submits first prompt
       |
       v
ChatWidget::submit_user_message()
  - Reads NoriConfig::loop_count
  - If count > 1: sets loop_remaining = count-1, loop_total = count
       |
       v
Agent completes task -> on_task_complete()
  - If loop_remaining > 0: emits AppEvent::LoopIteration
       |
       v
App::handle_event(LoopIteration)
  - Shuts down current conversation
  - Creates a fresh ChatWidget with the same prompt
  - Calls set_loop_state() on the new widget
  - Displays "Loop iteration N of M" info message
       |
       v
(repeat until remaining == 0)
```

State fields on `ChatWidget`: `loop_remaining: Option<i32>` and `loop_total: Option<i32>`. These are initialized on the first `submit_user_message()` call and carried forward across iterations via `App`-level event handling.

The loop survives transient failures and cancels only on fatal ones. The decision is owned by the prompt **completion**, not by the error event: on a prompt failure the ACP backend emits both an `EventMsg::Error` (display) and a `ClientEvent::PromptCompleted` carrying a `failure: Option<nori_protocol::TurnFailure>` (`Retryable`/`Fatal`; `None` for a clean success or user cancel — see `@/nori-rs/harness/docs.md`). `handle_client_prompt_completed()` in `@/nori-rs/tui/src/chatwidget/event_handlers.rs` calls `cancel_loop()` iff `failure == Some(Fatal)`, and it does so *before* the same completion drives `on_task_complete` to re-fire the next iteration. A `Retryable` failure leaves the loop armed so the next iteration retries; a `Fatal` failure disarms both `loop_remaining` and `loop_total` first. `on_error()` is now display-only (it appends the error cell and finalizes the turn but never touches loop state); concentrating the disposition on the single ordered `PromptCompleted` event removes the prior cross-channel race where an unconditional `Error`-driven `cancel_loop()` could disarm the loop before the completion re-fired it (e.g. on a momentary Anthropic `529`/overloaded or rate-limit blip). The completion also suppresses the generic "Conversation interrupted" notice whenever `failure.is_some()`, since the failure already surfaces its own error cell; only a clean user cancellation (`Cancelled` with `failure == None`) shows the interrupted notice. `cancel_loop()` (in `@/nori-rs/tui/src/chatwidget/pickers.rs`) is a no-op when no loop is active and logs (`tracing::info`) only when it actually cancels. The `/settings` sub-picker is a custom `BottomPaneView` implemented by `LoopCountPickerView` in `@/nori-rs/tui/src/nori/loop_count_picker.rs`. It offers preset options (Disabled, 2, 3, 5, 10) plus a "Custom..." option that enters an input mode where the user can type an arbitrary number (2-1000). Values <= 1 are treated as disabled, values > 1000 are capped. This follows the same `BottomPaneView` pattern used by `HotkeyPickerView`. The setting persists to `[tui]` in `config.toml` via `persist_loop_count_setting()`.

**History Insertion and Scrollback (`insert_history.rs`, `tui.rs`):**

`insert_history_lines()` pushes content into the terminal's native scrollback buffer above the ratatui viewport without disturbing ratatui's diff-based renderer. It works by manipulating ANSI scroll regions (DECSTBM, `\x1b[Pt;Pbr`) directly against the crossterm backend writer, bypassing the normal ratatui render pass. It returns `io::Result<bool>` where `false` means no room was available above the viewport (`area.top() == 0`) and the lines were not inserted.

The insertion algorithm:

```
1. If viewport is not at screen bottom: scroll viewport downward using RI (ESC M) inside
   a temporary scroll region covering [viewport.top()+1 .. screen_height].
2. Early return false if area.top() == 0 (viewport fills the whole screen; no space above it).
3. Set scroll region to [1 .. area.top()] (only the history area above the viewport).
4. Write lines into that region with \r\n advancement.
5. Reset scroll region to full screen.
6. Restore cursor to its pre-call position.
7. Return true.
```

The critical invariant: **DECSTBM `Pb=0` means "bottom of screen"**, not row 0. Calling `SetScrollRegion(1..0)` when `area.top() == 0` produces `\x1b[1;0r`, which sets the scroll region to the entire terminal rather than an empty region. Any subsequent writes then scroll through the viewport, corrupting ratatui's content in ways the diff-based renderer cannot detect. The `area.top() == 0` early return guards against this.

Two crossterm `Command` implementations support the function:

- `SetScrollRegion(Range<u16>)` — emits `\x1b[{start};{end}r`
- `ResetScrollRegion` — emits `\x1b[r` (restores full-screen scrolling)

**Viewport Repositioning in the Draw Loop (`tui.rs` `Tui::draw`):**

The draw loop manages viewport position bidirectionally to ensure the viewport stays anchored to the bottom of the terminal screen:

```
area.bottom() > size.height  --> viewport grew past screen bottom
                                  scroll history up, reposition viewport to bottom

area.y == 0 && height < size --> viewport was full-screen and has shrunk
                                  write pending lines directly into vacated rows,
                                  then reposition viewport to bottom
```

Both branches set `area.y = size.height - area.height`. The shrink branch guards on `area.y == 0` specifically because the stale-content problem only occurs when the viewport was at the top of the screen (full-screen). Normal height fluctuations where `area.y > 0` do not need repositioning because the viewport is already positioned with room above it.

When the shrink branch fires, the rows above the new viewport position contain stale rendered widget content from when the viewport was full-screen. Using `insert_history_lines()` here would push that stale content into terminal scrollback via the DECSTBM scroll region mechanism. Instead, the draw loop calls `write_pending_lines_directly()` to overwrite those rows in-place. If there are no pending history lines, the vacated rows are cleared directly.

**Direct Write for Vacated Rows (`insert_history.rs` `write_pending_lines_directly`):**

`write_pending_lines_directly()` writes history lines to specific terminal positions using `MoveTo` commands without scroll regions. This prevents stale viewport content from leaking into terminal scrollback. It is only used during the viewport shrink-from-full-screen transition in `Tui::draw`.

The function bottom-aligns content within the available rows (the last consumed line sits immediately above the viewport). It word-wraps each line individually to count screen rows, drains as many lines as fit from the input `Vec`, clears any remaining rows above the written content, then writes each wrapped line at its target position. Unconsumed lines remain in the `Vec` for later insertion via `insert_history_lines()`.

**Pending History Lines Retry Semantics:**

`Tui` holds a `pending_history_lines: Vec<Line>` buffer. On each draw, if the buffer is non-empty, `insert_history_lines()` is called. The buffer is only cleared when `insert_history_lines` returns `true` (lines were actually inserted). When it returns `false` (viewport at `y=0`, no room), the buffer is retained and insertion is retried on subsequent draws. This means once the viewport repositioning logic moves the viewport away from `y=0`, the retained lines will be inserted on the next frame. The buffer is capped at 1000 lines to prevent unbounded growth while the viewport is full-screen and insertion is blocked.

### Things to Know

**Module Structure Convention:**

Large modules use a directory layout (`foo/mod.rs` + submodules) instead of a single `foo.rs` file. This separates concerns and keeps individual files manageable. Modules using this pattern include `app/` (with `event_handling.rs`, `config_persistence.rs`, `session_setup.rs`), `chatwidget/` (with `event_handlers.rs`, `helpers.rs`, `user_input.rs`, `key_handling.rs`, `constructors.rs`, `approvals.rs`, `pickers.rs`, `login.rs`, `agent.rs`, `interrupts.rs`, `pending_exec_cells.rs`), `bottom_pane/chat_composer/` (with `key_handling.rs`, `paste_handling.rs`, `popup_management.rs`, `rendering.rs`), `bottom_pane/textarea/`, `resume_picker/` (with `helpers.rs`, `rendering.rs`, `state.rs`, `tests.rs`), `history_cell/`, and `nori/session_header/`. Test submodules use `tests/mod.rs` + `tests/part*.rs` for large test suites (e.g., `bottom_pane/textarea/tests/`). Snapshot `.snap` files live in a `snapshots/` subdirectory within each test module directory.

**Cargo Feature Flags:**

| Feature       | Dependencies                     | Default | Purpose                                    |
| ------------- | -------------------------------- | ------- | ------------------------------------------ |
| `login`       | `codex-login`, `codex-utils-pty` | Yes     | ChatGPT/API login functionality            |
| `otel`        | `opentelemetry-appender-tracing` | No      | OpenTelemetry tracing export               |
| `vt100-tests` | -                                | No      | vt100-based emulator tests                 |
| `debug-logs`  | -                                | No      | Verbose debug logging                      |

The old `nori-config` feature (which switched config sourcing between the harness crate, then named `nori-acp`, and `codex-core` at compile time) was removed in the crate-layering cleanup (`@/docs/specs/crate-layering.md`); the Nori config path (`~/.nori/cli/config.toml` via `@/nori-rs/nori-config/src/`) is now the only path.

**--yolo Flag:**

The `--dangerously-bypass-approvals-and-sandbox` flag (alias: `--yolo`) works in all builds. When enabled, it overrides any configured sandbox or approval policies to auto-approve all tool operations without prompting.

**Update Checking:**

The TUI uses Nori-specific update checking via the modules in `@/nori-rs/tui/src/nori/`:

- `nori/update_action.rs`: Update action handling
- `nori/updates.rs`: Version checking against GitHub releases
- `nori/update_prompt.rs`: User prompting for updates

**Error Reporting:**

When errors occur, users are directed to report bugs at `https://github.com/tilework-tech/nori-cli/issues`.

- Snapshot testing via `insta` is used extensively - see `snapshots/` directory
- Markdown rendering uses `pulldown-cmark` for parsing with `tree-sitter-highlight` for syntax highlighting
- Clipboard integration provided via `arboard` crate (disabled on Android/Termux)
- Terminal state is restored on exit or crash via the `tui.rs` module using `color-eyre` for panic handling. The `tui::restore()` / `tui::set_modes()` pair is also used for temporary terminal suspension (job control signals, external editor spawning).
- The `chatwidget/` module (split across `mod.rs` + submodules) contains most of the chat rendering logic
- The `first_prompt_text` field in `ChatWidget` is set when the user submits their first message and is used for both transcript matching in Claude Code sessions and as the prompt text replayed during loop mode iterations

Created and maintained by Nori.
