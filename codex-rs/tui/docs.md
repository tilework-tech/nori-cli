# Noridoc: nori-tui

Path: @/codex-rs/tui

### Overview

The `nori-tui` crate provides the interactive terminal user interface for Nori, built with the Ratatui framework. It handles the fullscreen TUI experience including chat display, input composition, onboarding flows, and real-time streaming of agent responses with markdown rendering.

### How it fits into the larger codebase

```
User Input --> nori-tui --> codex-acp (ACP backend)
                       \--> codex-core (config, auth)
                       \--> codex-rmcp-client (MCP OAuth login)
                       \--> nori-protocol (ACP session events)
                       \--> codex-protocol (shared control-plane events)
```

The TUI acts as the frontend layer. It:
- Uses `codex-acp` for ACP agent communication (see `@/codex-rs/acp/`)
- Uses `codex-core` for configuration loading and authentication (see `@/codex-rs/core/`)
- Consumes `nori-protocol` for ACP session-domain rendering (messages, plans, tool snapshots, approvals, replay, lifecycle)
- Displays approval requests from the ACP layer and forwards user decisions back
- Renders streaming AI responses with markdown and syntax highlighting

The `cli/` crate's `main.rs` dispatches to `nori_tui::run_main()` for interactive mode. Feature flags propagate from CLI to TUI for coordinated modular builds.

Key dependencies: `ratatui` for rendering, `crossterm` for terminal events, `pulldown-cmark` for markdown parsing, `tree-sitter-highlight` for syntax highlighting.

### Core Implementation

Entry point is `main.rs` which delegates to `run_app()` in `lib.rs`. The `run_main()` function loads `NoriConfig` once early and reuses it for both the auto-worktree setup and the `vertical_footer` setting (passed as a parameter to `run_ratatui_app()`). After loading config, `run_main()` initializes the agent registry via `codex_acp::initialize_registry()` with any custom `[[agents]]` defined in `config.toml` (see `@/codex-rs/acp/docs.md` for registry details). Initialization failure is non-fatal (logged as a warning).

The auto-worktree startup flow branches on the `AutoWorktree` enum (see `@/codex-rs/acp/docs.md`):

| Variant | Timing | Behavior |
|---------|--------|----------|
| `Automatic` | Before TUI init, in `run_main()` | Calls `setup_auto_worktree()` immediately and overrides cwd |
| `Ask` | After TUI init, in `run_ratatui_app()` | Sets `pending_worktree_ask = true`, deferred to a TUI popup shown after onboarding but before `App::run()` |
| `Off` | N/A | Skips worktree creation entirely |

The `Ask` popup is implemented by `nori::worktree_ask::run_worktree_ask_popup()`, a standalone mini-app screen using the same pre-`App` event-loop pattern as `nori::update_prompt` in release builds. It presents two options ("Yes, create a worktree" / "No, continue without a worktree") and returns a boolean. If the user confirms, `setup_auto_worktree()` is called and config is reloaded with the new cwd via `load_config_or_exit()`. Ctrl-C, Escape, and the "No" option all skip worktree creation. On failure, the TUI continues with the original cwd.

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

The transcript pager overlay uses each history cell's transcript view rather than the live summary view. To keep reopened transcripts readable, the overlay caps non-patch cells at 20 lines and appends an omission marker, while patch cells keep their full diff output for review. In ACP sessions, `ClientToolCell` provides differentiated `transcript_lines()` for Execute tools (shell-style `$ command` format via `render_execute_transcript_lines()`) while exploring and edit cells reuse their `display_lines()` rendering for transcripts.

**Approval Request Routing** (`chatwidget/event_handlers.rs`, `bottom_pane/approval_overlay.rs`): ACP approval requests arrive as `ClientEvent::ApprovalRequest` containing a `nori_protocol::ToolSnapshot`. The `approval_request_from_client_event()` function performs two-way routing: Execute tools with `Invocation::Command` map to `ApprovalRequest::Exec` (bash-highlighted overlay), and everything else (including Edit/Delete/Move) maps to `ApprovalRequest::AcpTool`. The `AcpTool` variant carries a boxed `ToolSnapshot`, a `cwd: PathBuf` (threaded from `self.config.cwd` in the chat widget), and dispatches decisions via `Op::ExecApproval`, which gives users the "always approve" option that `ApplyPatch` did not have. The `From<ApprovalRequest>` impl in `approval_overlay.rs` applies `relativize_paths_in_text` to the title before building the overlay prompt and `DiffSummary`, so users see relative paths instead of absolute ones. The fullscreen approval preview in `app/event_handling.rs` also uses the real `cwd` from the request for `DiffSummary` construction. `ApprovalRequest::ApplyPatch` is now only used by the legacy non-ACP codex backend. History cells for AcpTool decisions are produced by `history_cell::new_acp_approval_decision_cell()`, using `format_tool_kind()` for the kind label.

For edit-like tools (Edit/Delete/Move), both the approval overlay and the fullscreen preview extract diff data from the `ToolSnapshot` and render a `DiffSummary`. The diff extraction reuses two `pub(crate)` helpers from `client_tool_cell.rs`: `diff_changes_from_artifacts()` (checks `Artifact::Diff` entries) with fallback to `changes_from_invocation()` (handles `Invocation::FileChanges` and `Invocation::FileOperations`). When diff data is available, the overlay renders a `DiffSummary` via `ColumnRenderable` and the fullscreen preview renders a `DiffSummary` overlay titled "P A T C H". When no diff data is available, both paths fall back to text-only rendering of title, invocation, and artifacts.

**ClientToolCell Rendering** (`client_tool_cell.rs`):

`ClientToolCell` wraps a `nori_protocol::ToolSnapshot` (and a `cwd` path for path normalization) and implements `HistoryCell`. All ACP tool kinds route through `ClientToolCell` via `handle_client_native_tool_snapshot`. The cell selects between four rendering paths based on cell state: exploring cells (Read/Search, auto-detected via `is_exploring_snapshot()` or merged via `exploring_snapshots`) use `render_exploring_lines(width)`, `ToolKind::Execute` uses `render_execute_lines(width)` for display and `render_execute_transcript_lines(width)` for shell-style transcripts, Edit/Delete/Move kinds use `render_edit_lines()` for semantic verb headers with diff content, and all remaining tool kinds use `render_generic_lines()` for the generic `"Tool [phase]: title (kind)"` format with invocation/artifact details.

**Exploring cell grouping**: When consecutive Read/Search/ListFiles snapshots arrive, they are merged into a single `ClientToolCell` with a grouped exploring rendering. The exploring display shows a compact `Explored`/`Exploring` header with tree-prefixed sub-items that group consecutive reads by basename (e.g., `Read file1.rs, file2.rs`) and show `Search`/`List` labels with compact arguments. Read output content is omitted from exploring cells since it is noise in history. The merge logic in `handle_client_native_tool_snapshot` checks whether the active cell is an exploring `ClientToolCell` and the new snapshot is also exploring; if so, it merges the snapshot via `merge_exploring()` rather than creating a new cell. `merge_exploring()` deduplicates by `call_id` — if a snapshot with the same call_id already exists in the group, it is updated in place rather than appended. Merged call_ids are tracked in `completed_client_tool_calls` so completions arriving after the cell is flushed to history don't get re-merged into a later exploring cell. A standalone Read/Search snapshot (not merged with others) still uses `render_exploring_lines` — the auto-detection via `is_exploring_snapshot()` in `display_lines`/`transcript_lines` routes it there without requiring explicit `mark_exploring()`. The generic fallback sub-item renderer avoids duplicating the kind label when the title already starts with it (case-insensitive prefix check), e.g., `List /path` instead of `List List /path`.

**Tool title sanitization** (`client_event_format.rs`): The `sanitize_tool_title()` function cleans up noisy tool titles produced by some agents (notably Gemini). It strips `[current working directory ...]` bracket patterns and trailing `(description text)` parenthetical metadata, then trims whitespace. This is applied in the approval request path and helper functions in `event_handlers.rs`, ensuring that tool kinds display clean titles in the TUI.

**Execute rendering**: The execute rendering path reuses shared utilities from `exec_cell/render.rs` (`truncate_lines_middle`, `limit_lines_from_start`, `output_lines`, `spinner`) and layout constants that match the `ExecCell` display layout. Output text is sourced preferentially from `raw_output["stdout"]`, falling back to `Artifact::Text` with code fence stripping only for completed/failed snapshots. During pending/in-progress phases, artifact text for execute tools contains the agent's description (e.g., "Print current UTC date/time"), not stdout, so the fallback is suppressed via `is_active_phase` gating in `execute_output_text()`. Exit code success is determined from `raw_output["exit_code"]` when present, otherwise inferred from `ToolPhase`.

For Codex-backed ACP sessions, this rendering path depends on `nori-protocol` normalizing shell-wrapper `rawInput.command` arrays and `rawInput.parsed_cmd` metadata into structured `Invocation::Command` / `Invocation::Read` / `Invocation::Search` / `Invocation::ListFiles` values. Without that normalization, `ClientToolCell` falls back to rendering raw protocol JSON instead of the compact command and exploration details the TUI expects.

**Edit/Delete/Move rendering** (`render_edit_lines()`): Edit, Delete, and Move tool kinds use a dedicated rendering path with semantic verb-based headers from `format_edit_tool_header()` (in `client_event_format.rs`):

| Kind | In-Progress | Completed | Failed |
|------|-------------|-----------|--------|
| Edit | `Editing {path}` | `Edited {path}` | `Edit failed: {path}` |
| Delete | `Deleting {path}` | `Deleted {path}` | `Delete failed: {path}` |
| Move | `Moving {path}` | `Moved {path}` | `Move failed: {path}` |

The path is extracted from `locations[0].path` when available, falling back to parsing the title (stripping the kind prefix, e.g., `"Edit README.md"` -> `"README.md"`). Bullet styling: green bold for completed, red bold for failed, spinner for active. For failed edits, error text is extracted via `extract_error_text()` (checks `raw_output` for `"error"`, `"stderr"`, `"output"`, or bare string), with a `"(failed)"` fallback.

Diff content is rendered from two sources in priority order: (1) `Artifact::Diff` entries via `diff_changes_from_artifacts()`, (2) invocation data via `changes_from_invocation()` which handles both `Invocation::FileChanges` and `Invocation::FileOperations` (Create, Update, Delete, Move). Both helpers convert `nori_protocol` types to `codex_core::protocol::FileChange` for `create_diff_summary` from `diff_render.rs`. This means completed edits show inline diffs whether the diff data arrives as artifacts or as invocation-level file changes.

**Header promotion**: For all Edit/Delete/Move tools (both single-file and multi-file), the `DiffSummary`'s first header line is promoted to the outer header position. For a single-file edit this is the verb+path+line counts (e.g., "Edited README.md (+1 -1)"); for a multi-file edit this is the aggregate header (e.g., "Edited 2 files (+2 -2)"). The promoted line's "• " bullet prefix is stripped and replaced with the phase-aware bullet styling (green bold for completed, red bold for failed). For Move tools, the "Edited" verb span is swapped to "Moved" during header construction. This produces exactly one header line per edit cell. Diff content lines below the header come directly from `create_diff_summary`, which applies a single 4-space `prefix_lines()` indent — matching the indentation used by `PatchHistoryCell` in the non-ACP path. The `prefix_lines()` helper (from `@/codex-rs/tui/src/render/line_utils.rs`) propagates `Line.style.bg` onto the indent prefix span so that diff background tints (add/delete colors) extend edge-to-edge across the full terminal width.

**Generic rendering**: The generic rendering path (`render_generic_lines()`) applies several cleanup passes to produce compact output: code fences are stripped from text artifacts via `strip_code_fences()` (shared with the execute path), the `Output:` prefix is omitted so artifact text renders directly as detail lines, invocation detail lines that are redundant with the title are suppressed (e.g., `Read: /path` when the title already says `Read /path`), and absolute paths under `cwd` are relativized in both the header and invocation lines.

Bullet styling is phase-aware: active tools show a spinner, failed tools (`ToolPhase::Failed`) show a red bold bullet (`"•".red().bold()`), and all other completed tools show a dim bullet.

For failed tools, error detail is extracted via a cascade: (1) text artifacts (via `format_artifacts`), (2) `extract_error_text()` which checks `raw_output` for `"error"`, `"output"`, or bare string values, (3) a `"(failed)"` fallback when no detail is available at all. For non-failed tools, the location fallback still applies: when both invocation formatting and artifact formatting produce zero detail lines, it displays the `locations` paths from the `ToolSnapshot` as dim sub-items. This prevents completed tool cells from rendering as bare headers with no context, which occurs when agents (e.g., Gemini) send tool calls with empty `content` arrays and no `rawInput`/`rawOutput`.

**Edit/Delete/Move routing**: All Edit/Delete/Move snapshots (all phases including Completed) are routed to `handle_client_native_tool_snapshot`, the same handler used by Execute tools. In-progress snapshots create a spinner cell in `active_cell`. When the completed snapshot arrives with the same `call_id`, `apply_snapshot()` updates the cell in place, transitioning it from the spinner state to the completed state with diff content. The completed cell is then flushed to history. For completed Edit/Delete/Move snapshots, `handle_client_native_tool_snapshot` also calls `observe_directories_from_paths()` (using the snapshot's `locations`) and records tool call stats. `PatchHistoryCell` is no longer used in the ACP rendering path -- it remains only for the non-ACP codex backend path (via `on_patch_apply_begin`). Edit/Delete/Move approval requests route through `ApprovalRequest::AcpTool` (not `ApplyPatch`), so there are no bridge functions converting `nori_protocol` types to `codex_core::protocol::FileChange` for the approval path -- the diff extraction for approval overlays reuses the same `pub(crate)` helpers in `client_tool_cell.rs` that the completed-cell rendering uses.

**Execute Cell Completion Buffering** (`chatwidget/event_handlers.rs`, `chatwidget/mod.rs`):

When the ACP backend sends parallel execute tool calls (e.g., `date --utc`, `uptime -p`, `df -h` simultaneously), the TUI's single `active_cell` slot can only hold one cell at a time. Without buffering, when a new tool snapshot displaces the current active Execute cell, the displaced cell would be flushed to history with incomplete content -- showing the agent's description text (e.g., "Print current UTC date/time") as command output instead of actual stdout.

The `pending_client_tool_cells: HashMap<String, ClientToolCell>` buffer holds incomplete Execute cells that were displaced from `active_cell`. The flow in `handle_client_native_tool_snapshot()`:

1. **Buffer lookup first**: Before creating a new cell, the handler checks if the incoming snapshot's `call_id` matches a buffered cell. If found, the buffered cell is updated via `apply_snapshot()`. If the cell is now complete, it is inserted directly into history via `AppEvent::InsertHistoryCell` (bypassing `add_boxed_history` to avoid flushing the current active cell). If still incomplete, it goes back into the buffer.

2. **Conditional displacement**: When a new snapshot arrives and the current `active_cell` is an incomplete Execute `ClientToolCell`, instead of calling `flush_active_cell()` (which would send it to history with wrong content), the cell is moved to the buffer keyed by its `call_id`. Non-Execute cells and completed cells still go through the normal `flush_active_cell()` path.

3. **Turn-boundary drain**: The buffer is cleared (orphans discarded) at all turn boundaries: `on_agent_message()`, `on_task_complete()`, `finalize_turn()`, and `on_context_compacted()`. Discarding orphans is preferred over flushing them with description text.

The displacement check uses `into_any()` on `dyn HistoryCell` (added in `history_cell/mod.rs`) for owned downcasting from `Box<dyn HistoryCell>` to the concrete `ClientToolCell` type, and `snapshot_kind()` to confirm the cell is `ToolKind::Execute`.

**Chronological Ordering Invariant** (`chatwidget/event_handlers.rs`, `chatwidget/user_input.rs`):

Tool cells always appear in scrollback history before the agent text that follows them, matching the chronological order of execution. This is enforced by two mechanisms:

- `handle_streaming_delta()` always calls `flush_active_cell()` before streaming text, even when the active cell contains an incomplete (still-running) ExecCell. The incomplete cell is sent to history immediately rather than held in `active_cell` until completion.
- `flush_active_cell()` marks pending call_ids of incomplete ExecCells as completed (via `completed_client_tool_calls`) so that later completion events for the same call_ids do not create duplicate cells. The `pending_exec_cells` tracker is bypassed for this path -- cells go directly to history.
- `add_boxed_history()` also always flushes the active cell first, applying the same ordering guarantee when non-streaming history cells are inserted.

The trade-off: incomplete cells may appear in scrollback showing "Running"/"Exploring" status rather than their final "Ran"/"Explored" state, because they are flushed before completion events arrive.

**Interrupt Queue & Tool Event Deferral** (`chatwidget/event_handlers.rs`):

When the agent streams text, ACP `ClientEvent::ToolSnapshot` updates can arrive concurrently with answer or reasoning deltas. All ACP tool kinds route directly through `ClientToolCell` via `handle_client_native_tool_snapshot`, and the handler calls `flush_answer_stream_with_separator()` before deferring or rendering so tool cells appear in their correct interleaved position relative to text rather than being grouped after all text. The `InterruptManager` queues events via `defer_or_handle()` when the queue is already non-empty, preserving FIFO ordering for events that arrive while earlier deferred events are pending.

One operation consumes the queue:

| Method | Called From | Behavior |
|--------|------------|----------|
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

ACP cancel no longer makes the TUI idle on its own. The UI stays in `Cancelling` until the backend reduces the matching prompt response and emits `PromptCompleted`. See `@/codex-rs/acp/docs.md` for the backend-side reducer rules.

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

| Mode | `PlanDrawerMode` | Behavior |
|------|-------------------|----------|
| History cells | `Off` (default) | Each plan update creates a `PlanUpdateCell` in scrollback history |
| Collapsed drawer | `Collapsed` | One-line progress summary: `Plan: X/Y completed  *  > Current: step_name` |
| Expanded drawer | `Expanded` | Full plan checklist (same as the previous boolean `true` behavior) |

The toggle cycle (bound to `Ctrl+O` via `HotkeyAction::TogglePlanDrawer`) is: `Off -> Collapsed -> Expanded -> Collapsed -> ...`. Once the drawer enters a visible mode, it cycles between Collapsed and Expanded without returning to Off. The `toggle_plan_drawer()` method on `ChatWidget` implements this state machine. The `App` layer intercepts the hotkey binding in `handle_key_event()` and updates both the widget and its own `plan_drawer_mode` field.

The `pinned_plan` field on `ChatWidget` always tracks the latest plan update, regardless of the current mode. In the ACP path, `handle_client_plan_snapshot()` converts the normalized snapshot into `UpdatePlanArgs`, stores it in `pinned_plan`, and when the mode is `Off`, clones it into scrollback as a `PlanUpdateCell`. This "always-store" invariant means toggling the drawer on mid-conversation immediately shows the most recent plan without waiting for the next update.

The drawer is inserted into the `FlexRenderable` layout in `ChatWidget::as_renderable()` as a flex=0 child between the active cell (flex=1) and the bottom pane (flex=0):
- `Collapsed` renders `PinnedPlanDrawerCollapsed` (1 line, shows progress count and current/next step with truncation)
- `Expanded` renders `PinnedPlanDrawer` (full checklist via `render_plan_lines()`)
- `Off` contributes zero height

The config persists a boolean `pinned_plan_drawer` in `[tui]` of `config.toml`. At startup, `true` maps to `Expanded` and `false` maps to `Off`. Runtime toggling via Ctrl+O does not persist -- only the `/config` toggle persists.

The Nori-specific agent picker UI lives in `nori/agent_picker.rs`, allowing users to select between available ACP agents.

**System Info Collection** (`system_info.rs`):

The `SystemInfo` struct collects environment data in a background thread to avoid blocking TUI startup:

| Field | Source |
|-------|--------|
| `git_branch` | Git repository branch name |
| `active_skillsets` | Active skillsets from `nori-skillsets list-active` (one name per line; returns all skillsets active for the current directory). Empty vec if the command is unavailable or fails. |
| `git_lines_added` / `git_lines_removed` | Git diff statistics relative to the merge-base with the default branch (PR-like stats) |
| `is_worktree` | Whether CWD is a git worktree |
| `worktree_name` | Last path component of CWD when parent directory is `.worktrees`; used to display the immutable worktree directory identifier in the footer |
| `transcript_location` | Discovered transcript path and token usage when running within an agent environment |
| `worktree_cleanup_warning` | Warning when git worktrees exist and disk space is below 10% free (unix only) |

The `transcript_location` field includes both `token_usage` (total tokens) and `token_breakdown` (detailed input/output/cached breakdown) which are displayed in the TUI footer when Nori runs as a nested agent inside Claude Code, Codex, or Gemini.

**Git Diff Base Resolution** (`system_info.rs: resolve_diff_base()`):

The git diff stats are computed against the merge-base with the default branch, so they reflect what a PR would show rather than only uncommitted changes. The resolution order is:
1. `origin/HEAD` via `git symbolic-ref` -- detects the remote's default branch name
2. Falls back to checking if local `main` or `master` branches exist
3. Computes `git merge-base HEAD <branch>` to find the common ancestor
4. Falls back to `HEAD` if no default branch can be resolved (shows only uncommitted changes)

Untracked files (via `git ls-files --others --exclude-standard`) are also counted: their line counts are added to the insertion total. Binary files (non-UTF-8) are silently skipped. This means the statusline stats include new files that haven't been `git add`ed yet.

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

**Version caching:**

`get_nori_version()` shells out to `nori-skillsets --version` (or `nori-ai --version` as a legacy fallback) and caches the result in a process-wide `OnceLock` (`NORI_VERSION_CACHE`). The installed CLI version is stable for the lifetime of a TUI process, so the subprocess runs at most once per session. Only `nori-skillsets list-active` is re-invoked on every refresh.

**`/diff` Slash Command** (`get_git_diff.rs`):

The `/diff` handler in `key_handling.rs` resolves the effective CWD from the `effective_cwd_tracker` (falling back to `config.cwd`) and passes it to `get_git_diff()`. This ensures `/diff` works correctly in git worktrees and directories different from the process launch directory. All git commands in `get_git_diff.rs` use `.current_dir()` when a directory is provided.

`get_git_diff.rs` uses the same diff base resolution strategy as `system_info.rs` (`origin/HEAD` -> `main` -> `master` -> `HEAD` fallback), but implemented as async functions rather than the sync versions in `system_info.rs`. This duplication exists because the sync/async boundary makes sharing impractical. The result is that `/diff` output and the statusline diff stats are consistent -- both show PR-like diffs against the merge-base with the default branch.

**Worktree Cleanup Warning:**

During background system info collection on unix, `check_worktree_cleanup()` runs three checks in sequence: confirms the directory is a git repo via `git rev-parse --show-toplevel`, lists extra worktrees via `codex_git::list_worktrees()` (see `@/codex-rs/utils/git/`), and checks disk space via `df -Pk`. If worktrees exist and free disk space is below the `DISK_SPACE_LOW_PERCENT` threshold (10%), a `WorktreeCleanupWarning` is attached to the `SystemInfo` result. When the `App` event loop handles `SystemInfoRefreshed`, it checks for this warning and calls `chat_widget.add_warning_message()` to display a yellow warning cell in the chat history suggesting the user clean up unused worktrees. Non-unix platforms skip this check entirely.

**Slash Commands:**

| Command | Description |
|---------|-------------|
| `/agent` | Switch between available ACP agents (dynamically shows current agent name) |
| `/model` | Choose model (dynamically shows current agent/model name) |
| `/approvals` | Choose what Nori can do without approval (dynamically shows current approval mode) |
| `/config` | Toggle TUI settings (pinned plan drawer, vertical footer, terminal notifications, OS notifications, vim mode with enter behavior sub-picker, auto worktree, per session skillsets, notify after idle, hotkeys, script timeout, loop count, footer segments, file manager) |
| `/browse` | Open a terminal file manager to browse and edit files |
| `/new` | Start a new chat during a conversation |
| `/resume` | Resume a previous ACP session |
| `/init` | Create an AGENTS.md file with instructions |
| `/resume-viewonly` | View a previous session transcript (read-only) |
| `/compact` | Summarize conversation to prevent context limit |
| `/undo` | Open undo snapshot picker to select a restore point |
| `/diff` | Show PR-like git diff (changes since merge-base with default branch, plus untracked files) |
| `/mention` | Mention a file |
| `/status` | Show session configuration and context window usage |
| `/first-prompt` | Show the first prompt from this session |
| `/mcp` | Manage MCP server connections (add, toggle, delete) via interactive wizard |
| `/login` | Log in to the current agent |
| `/logout` | Show logout instructions |
| `/switch-skillset [name]` | Switch between available skillsets (with optional direct name) |
| `/fork` | Rewind conversation to a previous message |
| `/quit` | Exit Nori |
| `/exit` | Exit Nori (alias for /quit) |

**`/mcp` Picker** (`nori/mcp_server_picker.rs`):

The `/mcp` command opens an interactive `BottomPaneView` for managing MCP server connections (same pattern as `HotkeyPickerView`). It is not available during a task. The picker operates as a state machine with these modes:

| Mode | Purpose | Transitions |
|------|---------|-------------|
| `List` | Browse servers; "Add new..." row at index 0, servers below | Enter on "Add new..." -> `TransportSelect`; Enter on server -> toggle enabled; `d` on server -> `ConfirmDelete`; `l` on server -> OAuth login |
| `ConfirmDelete` | Confirm server deletion | `d` -> delete + save + `List`; Esc -> `List` |
| `TransportSelect` | Choose Stdio or HTTP transport | Enter -> `NameInput` |
| `NameInput` | Type server name | Enter -> `CommandInput` (stdio) or `UrlInput` (http) |
| `CommandInput` | Type command for stdio transport | Enter -> `ArgsInput` |
| `ArgsInput` | Type space-separated args | Enter -> `EnvInput` |
| `UrlInput` | Type URL for HTTP transport | Enter -> `HeaderInput` |
| `EnvInput` | Type env vars as `KEY=VAL` | Enter with empty -> finalize (stdio only); Enter with value -> adds to list, stays in `EnvInput` |
| `HeaderInput` | Type headers as `Key: Value` (HTTP only) | Enter with empty -> `SecretInput`; Enter with value -> adds to list, stays in `HeaderInput` |
| `SecretInput` | Type bearer token env var name (HTTP only) | Enter with value -> finalize (bearer token and client credentials are mutually exclusive); Enter with empty -> `ClientIdInput` |
| `ClientIdInput` | Type pre-registered OAuth client ID (HTTP only, for servers without dynamic registration) | Enter with value -> `ClientSecretEnvVarInput`; Enter with empty -> finalize (skip client credentials); Esc -> `SecretInput` |
| `ClientSecretEnvVarInput` | Type env var name for OAuth client secret (HTTP only) | Enter -> finalize; Esc -> `ClientIdInput` (restores typed client ID) |
| `OAuthInProgress` | Inline OAuth status display | Esc -> emits `McpOAuthLoginCancel`, returns to `List` |

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

The OAuth flow is fully async and inline -- no TUI suspension. The `McpOAuthLogin` event carries `server_name`, `server_url`, `http_headers`, `env_http_headers`, `client_id`, and `client_secret_env_var`. The handler in `app/config_persistence.rs` (`perform_mcp_oauth_login()`) resolves `client_secret` from the environment variable named by `client_secret_env_var` (if provided), then calls `codex_rmcp_client::start_oauth_login()` from `@/codex-rs/rmcp-client/`, passing the optional `client_id` and resolved `client_secret`. This selects between dynamic registration and pre-configured credential OAuth paths (see `@/codex-rs/rmcp-client/docs.md`). The returned `OAuthLoginHandle`'s cancel sender is stored in `App.mcp_oauth_cancel_tx`, and a spawned watcher task awaits the handle's `JoinHandle` and sends `AppEvent::McpOAuthLoginComplete` on finish.

Cancellation uses the oneshot channel pattern: Esc in `OAuthInProgress` mode emits `McpOAuthLoginCancel`, which calls `cancel_mcp_oauth_login()` (sends `()` on the stored cancel sender). The watcher task then resolves with the cancellation error. Completion (`McpOAuthLoginComplete`) shows a success or error info message and forwards to `McpServerPickerView::handle_oauth_complete()`, which transitions the picker from `OAuthInProgress` back to `List` mode.

**Agent-Provided Slash Commands** (`command_popup.rs`, `chat_composer/popup_management.rs`, `chat_composer/key_handling.rs`, `chatwidget/event_handlers.rs`):

ACP agents can advertise slash commands via the `AvailableCommandsUpdate` session notification. These flow through `nori-protocol` as `ClientEvent::AgentCommandsUpdate` and are forwarded to `BottomPane::set_agent_commands()` -> `ChatComposer::set_agent_commands()` -> `CommandPopup::set_agent_commands()`. The agent slug (e.g., `"claude-code"`) is set separately via `BottomPane::set_agent_slug()`, called from `ChatWidget::set_agent()` and `set_pending_agent()`.

Agent commands appear in the slash command popup alongside builtins and user prompts. They display with a prefixed name (e.g., `/claude-code:loop`) to disambiguate from builtins. If an agent command shares a name with a builtin command, the agent command is excluded from the popup. Fuzzy filtering operates on the prefixed display name. The prefix is a TUI display concept only -- it is stripped before submission so the ACP agent receives the bare command name. Tab autocompletes to `/<prefix>:<name> ` (e.g., `/claude-code:loop `) in the input field, but both the popup selection path and the typed text submission path strip the prefix: Enter from the popup submits `/<name>` (e.g., `/loop`), and typing the prefixed form directly (e.g., `/claude-code:loop 5m hi`) submits `/loop 5m hi` after the prefix-stripping logic in `key_handling.rs`. The Enter submission fallback path checks `agent_commands` after builtins and user prompts. Each `AgentCommandsUpdate` fully replaces the previous set.

**Slash Command Description Overrides:**

`/agent`, `/model`, and `/approvals` show the current runtime value in parentheses in the slash command popup (e.g., `(current: Mock ACP)`). This is implemented via a `command_description_overrides: HashMap<SlashCommand, String>` that flows through `BottomPane` -> `ChatComposer` -> `CommandPopup`. `BottomPane::set_agent_display_name()` sets overrides for both `/agent` and `/model`; `BottomPane::set_approval_mode_label()` sets the override for `/approvals`. The agent override is populated at startup in `BottomPane::new()` and updated on agent switches. The approval override is set whenever the approval mode changes.

**Selection Popup Row Layout (`bottom_pane/selection_popup_common.rs`):**

`render_rows()` and `measure_rows_height()` are the shared rendering functions used by all selection popups (`ListSelectionView`, `CommandPopup`, `FileSearchPopup`). Each popup item has an optional description that appears alongside the item name. The layout engine chooses between two modes per-row via `wrap_row()`:

| Mode | Condition | Layout |
|------|-----------|--------|
| Side-by-side | `total_width - desc_col >= MIN_DESC_COLUMNS` (12) | Description starts at `desc_col` on the same line as the name, wrapped lines indented to `desc_col` |
| Stacked | `total_width - desc_col < MIN_DESC_COLUMNS` | Name on its own line(s), description on separate line(s) below with 4-space indent |

The `desc_col` is computed once per render pass from the widest visible name plus 2 columns of padding. The stacked fallback prevents descriptions from being squeezed into 1-2 characters of horizontal space on narrow terminals. Because both `render_rows()` and `measure_rows_height()` call the same `wrap_row()` function, layout and height calculation are always consistent.

`SelectionViewParams` supports an optional `on_dismiss: Option<SelectionAction>` callback that fires when the picker is dismissed without selection (Escape or Ctrl-C). The callback is invoked in `ListSelectionView::on_ctrl_c()` before marking the view as complete. It does not fire when the user makes a selection via `accept()`. This is used by the skillset picker to send `SkillsetPickerDismissed` when the deferred agent spawn needs a fallback trigger.

**ListSelectionView Vim-Mode-Aware Search:**

`ListSelectionView` supports a `vim_mode: bool` field (alongside `is_searchable`) that changes how key input is routed. When a searchable view is created, `BottomPane::show_selection_view()` automatically injects the current `vim_mode_enabled` state into `SelectionViewParams`, so individual callers (skillset picker, config picker, etc.) do not need to pass vim mode explicitly.

The view operates as a state machine with three key-handling branches:

| Config | Sub-state | Key behavior |
|--------|-----------|-------------|
| `vim_mode=true`, `is_searchable=true` | `search_active=false` | `j`/`k` navigate, `/` activates search, digits 1-9 select directly, Esc dismisses |
| `vim_mode=true`, `is_searchable=true` | `search_active=true` | Characters filter the list, Backspace edits query, Esc exits search (clears query, returns to nav mode) without dismissing the popup |
| `vim_mode=false`, `is_searchable=true` | N/A | All characters immediately filter the list (no explicit search activation needed) |
| `is_searchable=false` | N/A | `j`/`k` navigate, digits 1-9 select directly (unchanged legacy behavior) |

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

The fork context flows through `ChatWidgetInit.fork_context` -> `spawn_agent()` -> `spawn_acp_agent()` -> `AcpBackendConfig.initial_context`, which initializes the ACP backend's `pending_compact_summary`. This reuses the same mechanism as `/compact` and `/resume` -- the summary is prepended to the first user prompt in the new session, giving the agent prior conversation context without a protocol-level session fork.

**Session context injection:** Both `spawn_acp_agent()` and `spawn_acp_agent_resume()` in `chatwidget/agent.rs` set `AcpBackendConfig.session_context` to the contents of `@/codex-rs/tui/session_context.md` (loaded at compile time via `include_str!`). This tells the ACP agent that it is running inside the nori CLI and provides a source-code URL for self-referential questions. The context is prepended (without `SUMMARY_PREFIX` framing) to the first user prompt only and then consumed (see `@/codex-rs/acp/docs.md` for the hook context injection mechanism).

The `/logout` command is only available when the `login` feature is enabled. The `/config` command requires the `nori-config` feature.


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

The card always shows: version, directory, agent, skillset (Nori profile). Optionally it shows:

| Section | Condition | Example |
|---------|-----------|---------|
| Task summary | `prompt_summary` present | "Task: Fix auth bug" |
| Approval mode | `approval_mode_label` present | "approvals: Agent" |
| Context line | `context_window_percent` present, with or without token data | "Context 27% (77.0K)" or just "Context 42%" |
| Token totals | `token_breakdown` has non-zero total | "Tokens: 123K total (32.0K cached)" |

The Tokens section renders if either `token_breakdown` has a non-zero total OR `context_window_percent` is present. This means context window percentage from the live API (`TokenUsageInfo`) can appear even before transcript token data is available.

Task summaries are truncated to 50 characters via `truncate_summary()`, which uses char-level operations (`chars().count()` / `chars().take()`) rather than byte slicing for UTF-8 safety with multi-byte characters.

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

When `skillset_per_session` is enabled in `NoriConfig`, the skillset picker is automatically triggered at startup in `App::run()`, regardless of whether the session is in a worktree. The agent spawn is deferred (`ChatWidgetInit::deferred_spawn = true`) so that `nori-skillsets switch` can write `.claude/CLAUDE.md` to disk before the agent reads it. During the deferred period, a dummy channel is created in `constructors.rs` so the widget has a valid `op_tx`. The real agent spawns after the user picks a skillset (`SkillsetSwitchResult` triggers `spawn_deferred_agent()`). If the user dismisses the picker without selecting a skillset (Escape/Ctrl-C or choosing the "No Skillset" option), the `AppEvent::SkillsetPickerDismissed` event triggers `spawn_deferred_agent()` -- the agent starts without a skillset, behaving as if the feature were disabled. The `deferred_spawn` flag on `ChatWidgetInit` causes a dummy op channel to be created during construction; the real agent spawns after the user picks a skillset or dismisses the picker.

When `skillset_per_session` is on and `auto_worktree` is `Off`, the picker subtitle changes from "Switching skillset in {dir}" to "Warning: skillset files will be added to {dir}" to warn that skillset files will be written directly to the current working directory (no worktree isolation). The `on_skillset_list_result()` method in `pickers.rs` loads `NoriConfig` to determine both the `show_no_skillset` flag (true when `skillset_per_session` is enabled) and the `auto_worktree_off` flag (true when per-session is on and `auto_worktree` is not enabled).

Events: `AppEvent::SkillsetListResult` (carries `install_dir: Option<PathBuf>`), `AppEvent::InstallSkillset`, `AppEvent::SwitchSkillset`, `AppEvent::SkillsetInstallResult`, `AppEvent::SkillsetSwitchResult`, `AppEvent::SkillsetPickerDismissed`, `AppEvent::OpenSkillsetPerSessionWorktreeChoice`

The "Per Session Skillsets" toggle in `/config` is built in `nori/config_picker.rs`. Toggling it on emits `AppEvent::OpenSkillsetPerSessionWorktreeChoice`, which opens a worktree choice modal (`skillset_worktree_choice_params()`) letting the user choose between "With Auto Worktrees" (sets `auto_worktree` to `Automatic`) and "Without Auto Worktrees". Toggling it off emits `AppEvent::SetConfigSkillsetPerSession`, handled in `app/config_persistence.rs` via `persist_skillset_per_session_setting()` to write `skillset_per_session` under `[tui]` in `config.toml`.

The "Auto Worktree" item in `/config` uses a sub-picker pattern (matching Notify After Idle / Script Timeout): selecting the config item emits `AppEvent::OpenAutoWorktreePicker`, which opens a second selection view listing all `AutoWorktree` variants (`Automatic`, `Ask`, `Off`) with radio-select style (current variant marked). The config item's display name shows the current mode in parentheses (e.g. "Auto Worktree (automatic)"). Selecting a variant emits `AppEvent::SetConfigAutoWorktree(variant)`, persisted via `persist_auto_worktree_setting()` which writes the string value (e.g. `"automatic"`, `"ask"`, `"off"`) to `[tui]` in `config.toml`.

Active skillset display in the footer is driven entirely by `SystemInfo.active_skillsets`, which is populated by shelling out to `nori-skillsets list-active`. After a successful skillset switch or install, `request_system_info_refresh()` triggers a background re-collection so the footer reflects the updated state. There is no in-memory override -- `nori-skillsets list-active` is the single source of truth.


**Notification Configuration:**

Three notification settings are toggled via `/config` and persisted to the `[tui]` section of `config.toml`:

- **Terminal Notifications** (`TerminalNotifications` enum from `@/codex-rs/acp/src/config/types/mod.rs`): Controls OSC 9 escape sequences. The ACP config value flows through `codex-core`'s `Config::tui_notifications` as a `bool`, and `chatwidget/user_input.rs::notify()` gates on that bool.
- **OS Notifications** (`OsNotifications` enum from `@/codex-rs/acp/src/config/types/mod.rs`): Controls native desktop notifications via `notify-rust`. Passed as `os_notifications` in `AcpBackendConfig` and read in `backend/mod.rs` to set the `use_native` flag on `UserNotifier`.
- **Notify After Idle** (`NotifyAfterIdle` enum from `@/codex-rs/acp/src/config/types/mod.rs`): Controls how long after the agent goes idle before a notification is sent. Unlike the toggle-style notification settings, this uses a sub-picker pattern (like agent picker) where selecting the config item opens a second selection view with radio-select style options (5s, 10s, 30s, 1 minute, Disabled). The selected value flows through `AcpBackendConfig` to `backend.rs` where it controls the idle timer spawn behavior.

Config changes for terminal and OS notifications emit `AppEvent::SetConfigTerminalNotifications` or `AppEvent::SetConfigOsNotifications`, handled in `app/config_persistence.rs` via `persist_notification_setting()`. The notify-after-idle setting uses a separate flow: `AppEvent::OpenNotifyAfterIdlePicker` opens the sub-picker, and `AppEvent::SetConfigNotifyAfterIdle` persists the chosen value via `persist_notify_after_idle_setting()`. All settings are written to the `[tui]` section of `config.toml`.

**Custom Prompt Script Execution:**

When a user invokes a `Script`-kind custom prompt (`.sh`, `.py`, `.js` files discovered from `~/.nori/cli/commands/`), the TUI follows an async execution pattern:

```
ChatComposer (Enter key)           app/mod.rs                       codex_core::custom_prompts
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

In `app/event_handling.rs`, the `ExecuteScript` handler shows an info message ("Running script..."), spawns a tokio task that calls `codex_core::custom_prompts::execute_script()` with the configured `script_timeout` from `NoriConfig`, and on completion sends `ScriptExecutionComplete`. On success, the stdout is submitted as a user message via `queue_text_as_user_message()`. On failure, an error message is displayed and the error context is also submitted as a user message so the agent can see it.

The script timeout is configurable via `/config` -> "Script Timeout" which opens a sub-picker (same pattern as Notify After Idle). The sub-picker is built by `script_timeout_picker_params()` in `@/codex-rs/tui/src/nori/config_picker.rs` and uses `AppEvent::OpenScriptTimeoutPicker` / `AppEvent::SetConfigScriptTimeout` events for the two-step flow. The setting is persisted to `[tui]` in `config.toml` via `persist_script_timeout_setting()`.

**Configurable Hotkeys:**

Keyboard shortcuts are configurable through the `/config` panel ("Hotkeys" item) and persisted under `[tui.hotkeys]` in `config.toml`. The implementation is split across two layers:

- **Config layer** (`@/codex-rs/acp/src/config/types/mod.rs`): Defines `HotkeyAction`, `HotkeyBinding`, and `HotkeyConfig` as terminal-agnostic string-based types. No crossterm dependency.
- **TUI layer** (`@/codex-rs/tui/src/nori/hotkey_match.rs`): Converts `HotkeyBinding` strings to crossterm `KeyEvent` matches via `parse_binding()` and `matches_binding()`. Also provides `key_event_to_binding()` for the reverse direction (capturing a key press as a binding string).

The `App` struct holds a `hotkey_config: HotkeyConfig` field loaded at startup. In `handle_key_event()` (`app/event_handling.rs`), configurable hotkeys are checked before the structural `match` block -- if a binding matches, the action fires and returns early. Changes are persisted via `persist_hotkey_setting()` (`app/config_persistence.rs`) which uses `ConfigEditsBuilder` to write to `[tui.hotkeys]` and updates the in-memory `HotkeyConfig` for immediate effect.

Hotkey actions fall into two categories that are consumed at different layers:

| Category | Actions | Consumed By |
|----------|---------|-------------|
| App-level | OpenTranscript, OpenEditor, TogglePlanDrawer | `app/event_handling.rs::handle_key_event()` |
| Editing | MoveBackwardChar, MoveForwardChar, MoveBeginningOfLine, MoveEndOfLine, MoveBackwardWord, MoveForwardWord, DeleteBackwardChar, DeleteForwardChar, DeleteBackwardWord, KillToEndOfLine, KillToBeginningOfLine, Yank | `textarea/mod.rs::input()` |
| UI triggers | HistorySearch | `chat_composer/key_handling.rs` |

Editing hotkeys are propagated from `App` down to the textarea via a `set_hotkey_config()` chain: App -> ChatWidget -> BottomPane -> ChatComposer -> TextArea. This propagation occurs at startup, after config changes via `persist_hotkey_setting()`, and when new sessions or agent switches create fresh ChatWidgets.

The textarea's `input()` method processes key events in three priority stages: (1) C0 control character fallbacks for terminals that send raw control codes without modifier flags, (2) configurable bindings checked via `matches_binding()` against the propagated `HotkeyConfig`, and (3) remaining hardcoded bindings (character insertion, Enter, arrow keys, Home/End, etc.).

The hotkey picker (`@/codex-rs/tui/src/nori/hotkey_picker.rs`) implements `BottomPaneView` directly (not `ListSelectionView`) because rebinding requires raw key capture. It uses a videogame-style rebind flow: select an action, press Enter, press the desired key. Conflicts are resolved by swapping bindings. The `r` key resets the selected action to its default.

**Vim Mode:**

The textarea supports an optional vim-style navigation mode, configured via `/config` ("Vim Mode" item) which opens a sub-picker (like Auto Worktree) showing three options. The setting is persisted to `config.toml` under `[tui]`:

```toml
[tui]
vim_mode = "newline"  # or "submit" or "off"
```

The `VimEnterBehavior` enum (from `@/codex-rs/acp/src/config/types/mod.rs`) controls both whether vim mode is enabled and how the Enter key behaves:

| Variant | Enter in INSERT | Enter in NORMAL | Vim Enabled |
|---------|----------------|-----------------|-------------|
| `Newline` | Inserts newline | Submits prompt | Yes |
| `Submit` | Submits prompt | Inserts newline | Yes |
| `Off` | N/A (vim disabled) | N/A | No |

The `ChatComposer` stores a `vim_enter_behavior: VimEnterBehavior` field alongside the textarea's own `vim_mode_enabled: bool`. The textarea only cares about on/off (for the vim state machine), while the composer uses the full enum to route Enter key presses at the top of its Enter handler in `key_handling.rs`.

When enabled, the textarea operates in two modes:

| Mode | Behavior |
|------|----------|
| Insert | Default mode. Characters are inserted as typed. Press `Escape` to enter Normal mode; the cursor moves back one position (standard vim behavior), but never past the beginning of the current line. |
| Normal | Navigation and editing mode. Keys are interpreted as commands rather than character input. |

Normal mode supports standard vim keybindings:

| Category | Keys | Behavior |
|----------|------|----------|
| Navigation | `h`/`j`/`k`/`l` (or arrow keys) | Move cursor left/down/up/right |
| Navigation | `w`/`b`/`e` | Forward/backward/end-of-word navigation (`w` moves to start of next word, `b` to start of previous word, `e` to end of current/next word) |
| Navigation | `0`/`$`/`^` | Beginning of line / end of line / first non-whitespace on line |
| Navigation | `G`/`gg` | End of text / beginning of text |
| Insert entry | `i`/`a` | Enter Insert at cursor / after cursor |
| Insert entry | `I`/`A` | Enter Insert at beginning of line / end of line |
| Insert entry | `o`/`O` | Open new line below/above and enter Insert |
| Editing | `x` | Delete character under cursor |
| Editing | `D`/`C` | Delete to end of line (`C` also enters Insert mode) |
| Editing | `dd` | Delete current line |
| Editing | `p` | Paste from kill buffer |
| Undo/Redo | `u` | Undo last edit or insert session |
| Undo/Redo | `Ctrl-R` | Redo last undone edit or insert session |

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

**Status Line Footer:**

The footer displays configurable segments, each of which can be enabled/disabled via `/config` -> "Footer Segments" or via `[tui.footer_segments]` in config.toml:

| Segment | TOML Key | Description |
|---------|----------|-------------|
| Task Summary | `prompt_summary` | "Task: <summary>" (dim) - generated by ACP backend on first user prompt |
| Vim Mode | `vim_mode` | "NORMAL" (blue/bold) or "INSERT" (green) when vim mode is enabled |
| Git Branch | `git_branch` | Current branch name with ⎇ symbol (yellow for main repo, orange for worktree) |
| Worktree Name | `worktree_name` | "Worktree: {name}" (light red) when running in an auto-worktree session -- the immutable directory name, distinct from the git branch which gets renamed after the first prompt |
| Git Stats | `git_stats` | Lines added/removed in current session |
| Context Window | `context` | "Context 27% (34K)" when running within an agent environment |
| Approval Mode | `approval_mode` | "Approvals: Agent/Full Access/Read Only" |
| Nori Profile | `nori_profile` | "Skillset: name" for one active skillset, "Skillsets: a, b" for multiple, hidden when none are active. Uses `active_skillsets` from `SystemInfo` (populated by `nori-skillsets list-active`). |
| Nori Version | `nori_version` | "Skillsets v<version>" |
| Token Usage | `token_usage` | "Tokens: 123K total (32K cached)" when running within an agent environment |

Example config.toml to disable specific segments:
```toml
[tui.footer_segments]
token_usage = false
git_stats = false
```

All segments are enabled by default. The order of segments in the footer is fixed (cannot be reordered via config).

Token data flows from `TranscriptLocation.token_breakdown` (provided by `codex_acp::discover_transcript_for_agent_with_message()`) through `FooterProps` to the footer renderer. The breakdown includes separate input, output, and cached token counts for accurate usage reporting.
Footer context usage is sourced in priority order: ACP `SessionUpdateInfo { kind: Usage, usage: Some(..) }` updates drive the footer when available, while `TranscriptLocation.token_breakdown` remains the provider-specific fallback for older sessions or agents that do not emit ACP usage updates.

The prompt summary flows from the ACP backend as an `EventMsg::PromptSummary` event, handled by `ChatWidget::on_prompt_summary()`, which propagates it down: `ChatWidget` -> `BottomPane::set_prompt_summary()` -> `ChatComposer::set_prompt_summary()` -> `FooterProps.prompt_summary` -> `footer_segments()` renderer.

The TUI detects the repo root for auto-worktree branch renaming by inspecting the cwd path structure: when `auto_worktree.is_enabled()` (true for both `Automatic` and `Ask` variants) and the cwd's parent directory is named `.worktrees`, the grandparent is treated as the repo root. This value is passed as `auto_worktree_repo_root` in `AcpBackendConfig` (see `chatwidget/agent.rs`). The branch rename is fire-and-forget; the working directory does not change during a session, so the TUI does not need to handle directory changes.

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
3. Spawns the file manager with chooser-mode arguments (from `FileManager::chooser_args()` in `@/codex-rs/acp/src/config/types/mod.rs`)
4. On success, reads the first line of the temp file as the selected path
5. If the selected path is a file, opens it in the user's editor using the same `editor::resolve_editor()` / `editor::spawn_editor()` as Ctrl-G
6. Re-enables the TUI via `tui::set_modes()`

When `/browse` is invoked, `SlashCommand::Browse` dispatches by loading `NoriConfig` to check `file_manager`. If `None`, an error message directs the user to `/config`. If set, it sends `AppEvent::BrowseFiles(fm)`.

The file manager setting is configurable via `/config` -> "File Manager" which opens a sub-picker (same pattern as auto worktree). The sub-picker is built by `file_manager_picker_params()` in `@/codex-rs/tui/src/nori/config_picker.rs` and uses `AppEvent::OpenFileManagerPicker` / `AppEvent::SetConfigFileManager` events for the two-step flow. The setting is persisted to `[tui]` in `config.toml` via `persist_file_manager_setting()`.

**View-Only Transcript Viewing:**
The `/resume-viewonly` command allows viewing previous session transcripts without replaying the conversation. Implementation in `@/codex-rs/tui/src/`:

- `viewonly_transcript.rs`: Converts `codex_acp::transcript::Transcript` entries to `ViewonlyEntry` enum (User, Assistant, Thinking, Info variants)
- `nori/viewonly_session_picker.rs`: Session picker UI for selecting past sessions
- `app/session_setup.rs::display_viewonly_transcript()`: Renders entries in the chat history

Rendering behavior:
- User messages display via `UserHistoryCell` with standard user styling
- Assistant messages render via `AgentMessageCell` with `append_markdown()` for syntax highlighting
- Thinking blocks display with dimmed styling (matching live reasoning display)
- Tool calls, tool results, and patch operations are skipped to focus on conversation content
- Blank line separators between entries improve readability

The async flow uses three AppEvents: `ShowViewonlySessionPicker` -> `LoadViewonlyTranscript` -> `DisplayViewonlyTranscript`.

**Session Resume (`/resume`):**

The `/resume` command allows reconnecting to a previous ACP session. It uses the ACP agent's `session/load` RPC when available, and otherwise falls back to a fresh ACP session plus normalized replay derived from the saved transcript (see `@/codex-rs/acp/docs.md`).

The flow involves three layers:

```
SlashCommand::Resume
    |
    v
ChatWidget::open_resume_session_picker()
    |  (async: loads sessions via TranscriptLoader, filters by agent)
    v
AppEvent::ShowResumeSessionPicker -> resume_session_picker modal
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
spawn_acp_agent_resume() -> AcpBackend::resume_session()
```

The `ResumeSession` handler loads the full transcript (not just metadata) via `TranscriptLoader::load_transcript()`. The `acp_session_id` is extracted as `Option<String>` from `transcript.meta.acp_session_id` -- sessions without an `acp_session_id` are still resumable via the normalized replay fallback.

Session filtering: `load_resumable_sessions()` in `@/codex-rs/tui/src/nori/resume_session_picker.rs` loads all sessions for the current working directory via the viewonly session picker's `load_sessions_with_preview()`, then filters to only sessions whose `agent` field matches the currently active agent.

The resume session picker reuses the `SessionPickerInfo` type and `format_relative_time()` utility from `@/codex-rs/tui/src/nori/viewonly_session_picker.rs`. The `format_relative_time` function was made `pub(crate)` for this reuse.

`spawn_acp_agent_resume()` in `@/codex-rs/tui/src/chatwidget/agent.rs` mirrors `spawn_acp_agent()` but calls `AcpBackend::resume_session()` instead of `AcpBackend::spawn()`, passing both the optional `acp_session_id` and the full `Transcript`. Both spawn paths receive a single `BackendEvent` stream from `codex-acp`: normalized `ClientEvent` items drive ACP session rendering, while `Control` events still carry shared app-level concerns such as `SessionConfigured`, warnings, and shutdown.

**Agent Connection Lifecycle & Failure Recovery:**

Agent registration validation is performed exclusively in `spawn_agent()` (`chatwidget/agent.rs`). When the configured model is not in the ACP registry, `spawn_agent()` routes to `spawn_error_agent()` which sends `AppEvent::AgentSpawnFailed` -- triggering `on_agent_spawn_failed()` to display the error and reopen the agent picker for recovery. There is no early validation in `App::run()`; this single validation point ensures that unregistered agents (including custom agents that were configured but later removed) always get graceful recovery through the agent picker rather than a fatal startup error.

When the user selects an agent (or resumes a session), the TUI shows a "Connecting to [Agent]" status indicator via `ChatWidget::show_connecting_status()`. Each spawn function (`spawn_acp_agent`, `spawn_acp_agent_resume`) uses a `tokio::select!` to race three concurrent futures during backend initialization:

| Arm | Trigger | Action |
|-----|---------|--------|
| Backend init completes (success) | `AcpBackend::spawn()` / `resume_session()` returns `Ok` | Proceeds to op forwarding and event forwarding |
| Backend init completes (failure) | Returns `Err` | Sends `AppEvent::AgentSpawnFailed`, drops `codex_op_rx` |
| `drain_until_shutdown()` | User sends `Op::Shutdown` during connection | Sends `AppEvent::ExitRequest`, drops `codex_op_rx` |
| `spawn_timeout_sequence()` | 8s warning + 30s abort elapse | Sends warning at 8s, then `AgentSpawnFailed` at 38s, drops `codex_op_rx` |

`drain_until_shutdown()` reads ops from the channel, discarding everything until it sees `Op::Shutdown`. This allows the user to exit (via `/exit`, Ctrl-C) even while the backend is still attempting to connect. `spawn_timeout_sequence()` provides user feedback: at 8 seconds it sends a `WarningEvent` visible in the chat, and after 30 more seconds it aborts the connection attempt entirely.

`on_agent_spawn_failed()` in `chatwidget/helpers.rs` performs three recovery steps in order:
1. Clears the "Connecting" status indicator via `bottom_pane.hide_status_indicator()`
2. Displays an error message in chat history: "Failed to start agent '{name}': {error}"
3. Reopens the agent picker so the user can select a different agent

**Status Indicator Whimsical Messages (`status_indicator_widget.rs`):**

When the agent begins processing a task, the `StatusIndicatorWidget` displays an animated header with a randomly selected tongue-in-cheek message (e.g., "Thinking really hard", "Hallucinating responsibly") drawn from the `WHIMSICAL_STATUS_MESSAGES` pool via `random_status_message()`. A new random message is selected each time `on_task_started()` fires in `chatwidget/event_handlers.rs`. During streaming, reasoning chunk headers (extracted from bold markdown text) dynamically replace this initial message via `update_status_header()`.

**Terminal Title Management (`terminal_title.rs`, `chatwidget/helpers.rs`):**

The TUI sets the terminal window/tab title via OSC 0 escape sequences so users can see whether Nori is idle or working at a glance, even when the tab is not focused. The title is written directly to stdout via crossterm's `execute!` macro with a custom `SetWindowTitle` command implementation -- this bypasses the ratatui draw buffer entirely.

When the agent is working (`mcp_startup_status` is present or `bottom_pane.is_task_running()` is true), an animated braille dot-spinner (`SPINNER_FRAMES`, 10 frames at 100ms intervals) appears before the project name in the title bar. When idle, only the project name (derived from `config.cwd`) is shown. The animation is gated on `config.animations` -- when disabled, the spinner is suppressed but the project name still appears.

The animation is demand-driven rather than timer-based: each `refresh_terminal_title()` call schedules the next frame via `FrameRequester::schedule_frame_in(100ms)`, and `pre_draw_tick()` (called before every frame in the `TuiEvent::Draw` handler in `app/event_handling.rs`) advances the spinner only when progress is active. This creates a self-stopping loop -- when progress ends, no further frames are scheduled. Title writes are deduplicated via a `last_terminal_title: Option<String>` cache to avoid redundant OSC writes.

`refresh_terminal_title()` is hooked into `on_session_configured()`, `on_task_started()`, `on_task_complete()`, and `on_mcp_startup_complete()` in `chatwidget/event_handlers.rs`. The title is cleared (set to empty string) on `ChatWidget` drop. The module does not attempt to save or restore the terminal's previous title because that is not portable across terminals.

Title content is sanitized by `sanitize_terminal_title()` which strips control characters, bidi overrides, zero-width characters, and collapses whitespace, with a 240-character cap.

**Exit Path When Backend Is Dead:**

Every error/timeout/shutdown arm in the `tokio::select!` explicitly calls `drop(codex_op_rx)` before returning. This closes the receiver end of the channel so that `codex_op_tx` (held by `ChatWidget`) has no listener. If the user then attempts to exit (via `/exit`, `/quit`, or Ctrl-C), `submit_op(Op::Shutdown)` detects the dead channel (the `send()` returns `Err`) and falls back to sending `AppEvent::ExitRequest` directly via `app_event_tx`. This ensures the TUI can always exit cleanly even when no backend is running.

**Loop Mode (Prompt Repetition):**

Loop mode allows the same first prompt to be re-run multiple times, each time in a completely fresh conversation session. This is configured via `/config` -> "Loop Count" or by setting `loop_count` in `config.toml` (see `@/codex-rs/acp/src/config/types/mod.rs`).

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

The loop is cancelled (both fields set to `None`) when an error occurs or a turn ends unsuccessfully. The `/config` sub-picker is a custom `BottomPaneView` implemented by `LoopCountPickerView` in `@/codex-rs/tui/src/nori/loop_count_picker.rs`. It offers preset options (Disabled, 2, 3, 5, 10) plus a "Custom..." option that enters an input mode where the user can type an arbitrary number (2-1000). Values <= 1 are treated as disabled, values > 1000 are capped. This follows the same `BottomPaneView` pattern used by `HotkeyPickerView`. The setting persists to `[tui]` in `config.toml` via `persist_loop_count_setting()`.

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

Large modules use a directory layout (`foo/mod.rs` + submodules) instead of a single `foo.rs` file. This separates concerns and keeps individual files manageable. Modules using this pattern include `app/` (with `event_handling.rs`, `config_persistence.rs`, `session_setup.rs`), `chatwidget/` (with `event_handlers.rs`, `helpers.rs`, `user_input.rs`, `key_handling.rs`, `constructors.rs`, `approvals.rs`, `pickers.rs`, `login.rs`, `agent.rs`, `session_header.rs`, `interrupts.rs`, `pending_exec_cells.rs`), `bottom_pane/chat_composer/` (with `key_handling.rs`, `paste_handling.rs`, `popup_management.rs`, `rendering.rs`), `bottom_pane/textarea/`, `resume_picker/` (with `helpers.rs`, `rendering.rs`, `state.rs`, `tests.rs`), `history_cell/`, and `nori/session_header/`. Test submodules use `tests/mod.rs` + `tests/part*.rs` for large test suites (e.g., `bottom_pane/textarea/tests/`). Snapshot `.snap` files live in a `snapshots/` subdirectory within each test module directory.

**Cargo Feature Flags:**

| Feature | Dependencies | Default | Purpose |
|---------|--------------|---------|---------|
| `unstable` | `codex-acp/unstable` | Yes | Unstable ACP features like agent switching |
| `nori-config` | - | Yes | Use Nori's simplified ACP-only config |
| `login` | `codex-login`, `codex-utils-pty` | Yes | ChatGPT/API login functionality |
| `otel` | `opentelemetry-appender-tracing` | No | OpenTelemetry tracing export |
| `vt100-tests` | - | No | vt100-based emulator tests |
| `debug-logs` | - | No | Verbose debug logging |

**--yolo Flag:**

The `--dangerously-bypass-approvals-and-sandbox` flag (alias: `--yolo`) works in all builds. When enabled, it overrides any configured sandbox or approval policies to auto-approve all tool operations without prompting.

**Update Checking:**

The TUI uses Nori-specific update checking via the modules in `@/codex-rs/tui/src/nori/`:
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
