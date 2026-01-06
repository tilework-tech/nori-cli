# Noridoc: tui

Path: @/codex-rs/tui

### Overview

The `codex-tui` crate provides the interactive terminal user interface for Codex, built with the Ratatui framework. It handles the fullscreen TUI experience including chat display, input composition, onboarding flows, session management, and real-time streaming of model responses with markdown rendering.

### How it fits into the larger codebase

TUI is one of the primary entry points, invoked when running `nori` without a subcommand:

- **Depends on** `codex-core` for conversation management, configuration, and authentication
- **Depends on** `codex-acp` for ACP agent backend (alternative to HTTP-based LLM providers)
- **Depends on** `codex-common` for CLI argument parsing and shared utilities
- **Uses** `codex-protocol` types for events and messages
- **Optionally integrates** `codex-feedback`, `codex-login`, `codex-backend-client` via feature flags

The `cli/` crate's `main.rs` dispatches to `codex_tui::run_main()` for interactive mode. Feature flags propagate from CLI to TUI for coordinated modular builds.

### Core Implementation

**Entry Point:**

`run_main()` in `lib.rs`:
1. Parses CLI arguments and loads configuration
2. Initializes tracing (file + OpenTelemetry)
3. Runs onboarding if needed (login, trust screen)
4. Handles session resume selection
5. Launches the main `App::run()` loop

**Application Core:**

- `app.rs`: Main `App` struct managing application state and event loop
- `app_event.rs`: Application-level events (key input, model responses, etc.)
- `tui.rs`: Terminal initialization and restoration

**Agent Spawning (`chatwidget/agent.rs`):**

The TUI supports two backend modes, selected automatically at startup based on model name:

- `spawn_agent()`: Entry point that detects ACP vs HTTP mode via `codex_acp::get_agent_config()`
- `spawn_acp_agent()`: Uses `AcpBackend` for ACP-registered models (e.g., "mock-model", "mock-model-alt", "claude-acp", "gemini-acp")
- `spawn_http_agent()`: Uses `codex-core` for HTTP-based LLM providers (OpenAI, Anthropic, etc.)
- `spawn_error_agent()`: Displays error and exits for unregistered models when HTTP fallback is disabled

Both backends produce `codex_protocol::Event` for the TUI event loop, enabling unified event handling.

**ACP Backend Arc Reference Handling:**

In `spawn_acp_agent()`, the main task must drop its `Arc<AcpBackend>` reference after spawning the op forwarding task. This prevents a self-reference deadlock:
- The op task holds `Arc<AcpBackend>` for submitting operations
- The backend contains `event_tx` internally
- The main task waits on `event_rx` for events
- If the main task also held an Arc reference, dropping the backend would require the main task to exit first, but the main task waits on `event_rx`, which can't close until `event_tx` is dropped
- Solution: `drop(backend)` after spawning the op task, so when the op channel closes (when `codex_op_rx` closes), the backend is fully dropped, closing `event_tx` and allowing `event_rx` to return `None`

**UI Components:**

- `chatwidget.rs`: Main conversation display widget
- `bottom_pane.rs`: Status bar and key hints
- `markdown_render.rs` / `markdown_stream.rs`: Markdown to Ratatui rendering
- `diff_render.rs`: Patch diff visualization
- `selection_list.rs`: Generic selection popup widget
- `shimmer.rs`: Loading animation effects
- `status_indicator_widget.rs`: Status display
- `nori/`: Nori-specific branding and customization (see `@/codex-rs/tui/src/nori/docs.md`)
- `system_info.rs`: Background system info collection for footer (git branch, Nori profile/version, git stats, worktree detection)
- `effective_cwd_tracker.rs`: Tracks effective CWD from tool call locations with debounce logic

**Input Handling:**

- `public_widgets/composer_input.rs`: Text input with multi-line support
- `clipboard_paste.rs`: Clipboard integration
- `slash_command.rs`: `/command` parsing and execution
- `file_search.rs`: Fuzzy file finder

**ACP Agent Switching:**

- `/agent` opens the Nori-specific agent picker popup in `tui/src/chatwidget.rs`, which drives `nori::agent_picker::agent_picker_params()` and renders the metadata returned by `codex_acp::list_available_agents()` as `SelectionItem`s.
- Selecting an agent sends `AppEvent::SetPendingAgent`, so both the App and `ChatWidget` store a `pending_agent` (see `PendingAgentSelection` and `PendingAgentInfo`). The UI informs the user that the switch will happen on the next prompt submission.
- When the next prompt is submitted, `ChatWidget` intercepts the queued `UserMessage`, forwards it as `AppEvent::SubmitWithAgentSwitch`, and lets the App restart the conversation with the new model. The App persists the agent selection to `~/.nori/cli/config.toml` via `ConfigEditsBuilder::set_agent()`, updates `Config`, shuts down the old conversation, and creates a `ChatWidget` with `expected_model` to filter out leftover events.
- This workflow avoids disrupting active turns and powers the agent-switching verification in `tui-pty-e2e/tests/agent_switching.rs`.

**ACP Model Switching (Unstable Feature):**

When the `unstable` feature is enabled, `/model` in ACP mode allows switching between models provided by the ACP agent:

- `ChatWidget` stores an optional `acp_handle: Option<AcpAgentHandle>` (only present in ACP mode)
- `AcpAgentHandle` in `chatwidget/agent.rs` provides async methods: `get_model_state()` and `set_model()`
- When `/model` is invoked in ACP mode:
  1. `open_model_popup()` detects ACP mode via `codex_acp::get_agent_config()`
  2. If `acp_handle` exists, it asynchronously fetches model state via `handle.get_model_state()`
  3. Sends `AppEvent::OpenAcpModelPicker` with available models and current selection
  4. `App` calls `ChatWidget::open_acp_model_picker()` to display the selection popup
  5. User selection sends `AppEvent::SetAcpModel { model_id, display_name }`
  6. `ChatWidget::set_acp_model()` calls `handle.set_model()` asynchronously
  7. Success/failure reported via `AppEvent::AcpModelSetResult`

Key difference from agent switching: Model switching preserves conversation history (uses ACP's `session/set_model`), while agent switching rebuilds the entire session.

When the `unstable` feature is disabled, `/model` in ACP mode shows a disabled picker via `acp_model_picker_params()` directing users to use `/agent`.

**Onboarding:**

The `onboarding/` module handles first-run experience:
- Login screen (ChatGPT OAuth or API key) - requires `login` feature
- Trust screen (directory permission settings)
- Windows WSL setup instructions

**Session Management:**

- `resume_picker.rs`: UI for selecting sessions to resume
- `session_log.rs`: High-fidelity session event logging

### Things to Know

**Feature Flags Architecture:**

The TUI crate uses Cargo feature flags to enable modular builds with two primary modes:

| Feature | Optional Dep | Description |
|---------|-------------|-------------|
| `full` | - | Meta-feature enabling all optional features |
| `login` | `codex-login` | ChatGPT/API login functionality |
| `feedback` | `codex-feedback` | Sentry feedback integration |
| `backend-client` | `codex-backend-client` | Cloud tasks backend client |
| `upstream-updates` | - | OpenAI/Codex update checking mechanism |
| `oss-providers` | `codex-common/oss-providers` | Ollama/LM Studio local model support |
| `codex-features` | - | Gates `/undo`, `/compact`, `/review` slash commands |

Feature gating patterns:
- Import gating: `#[cfg(feature = "backend-client")] use codex_backend_client::Client`
- Struct field gating: `#[cfg(feature = "feedback")] feedback: CodexFeedback`
- Function parameter gating: `#[cfg(feature = "feedback")] feedback: CodexFeedback` in `App::run()`
- Enum variant gating: `AppEvent::Feedback` only exists with `feedback` feature
- Compatibility module pattern: `feedback_compat.rs` provides stub types when `feedback` feature is disabled

**Feedback Compatibility Layer:**

The `feedback_compat.rs` module provides API-compatible types when the `feedback` feature is disabled:
- **With `feedback` enabled:** Re-exports `CodexFeedback` and `CodexLogSnapshot` from `codex_feedback`
- **With `feedback` disabled:** Provides stub implementations with no-op behavior (e.g., `upload_feedback()` returns `Ok(())`, `make_writer()` returns a writer that discards output)

This pattern allows TUI code to use feedback types unconditionally without `#[cfg]` attributes at every call site. The stub structure is designed as a placeholder for future Nori-specific feedback functionality.

**Update System Selection:**

The update checking system is selected at compile time via `upstream-updates`:
- With `upstream-updates`: Uses `update_action.rs`, `updates.rs`, `update_prompt.rs` from `@/codex-rs/tui/src/`
- Without `upstream-updates`: Uses Nori-specific versions from `@/codex-rs/tui/src/nori/`
- Re-exports in `lib.rs` provide unified access: `pub mod update_action` re-exports from either location

Update modules are only compiled in release builds (`#[cfg(not(debug_assertions))]`) to avoid unnecessary checks during development.

**Rendering Patterns:**

The crate uses Ratatui's `Stylize` trait for concise styling:
```rust
// Preferred
"text".red(), "text".dim(), vec![...].into()
// Avoid
Span::styled("text", Style::default().fg(Color::Red))
```

Text wrapping uses `textwrap::wrap` for plain strings and custom `wrapping.rs` helpers for styled `Line` objects.

**Markdown Streaming:**

`markdown_stream.rs` handles incremental markdown rendering as tokens arrive, maintaining rendering state across deltas for smooth display updates.

**Event Loop Architecture:**

The app uses a tokio-based event loop that multiplexes:
- Terminal input events (crossterm)
- Model response events (from core)
- Timers for animations

State updates flow through `app_event_sender.rs` channels.

**Status Indicator Lifecycle:**

The "Working (Xs)" status indicator in the bottom pane is controlled exclusively by conversational turn boundaries:

- **Shown**: When `TaskStarted` event arrives, `on_task_started()` calls `set_task_running(true)`
- **Hidden**: When `TaskComplete` event arrives, `on_task_complete()` calls `set_task_running(false)`

**Invariant**: Streaming operations (e.g., `on_commit_tick()`) must NOT call `hide_status_indicator()`. The status indicator should remain visible throughout the entire conversational turn, including during:
- Streaming text deltas being committed to history
- Subagent completions (when parent turn is still active)
- Tool call executions

This ensures users see continuous "Working" feedback until the agent's conversational turn fully completes.

**Interrupt Queueing and Approval Handling:**

Most event types (exec begin/end, MCP calls, elicitation) are queued during active streaming and flushed when streaming completes via `InterruptManager`. However, **approval requests are handled immediately** (not deferred):

- `on_exec_approval_request()` and `on_apply_patch_approval_request()` call their handlers directly
- This prevents deadlocks in ACP mode where the agent subprocess blocks waiting for approval
- If approval were deferred, the agent would wait for approval, but TaskComplete (which flushes the queue) wouldn't arrive until the agent finished
- The `InterruptManager` still contains `ExecApproval` and `ApplyPatchApproval` variants for completeness, but these methods are marked `#[allow(dead_code)]`
- `on_task_complete()` calls `flush_interrupt_queue()` for any remaining queued items

**Approval Overlay Model Display Name:**

The approval overlay displays the current agent's display name (e.g., "Claude", "Gemini") instead of a hardcoded name in options like "No, and tell Claude what to do differently". The display name flows through:

1. `ChatWidget` initialization resolves the display name via `nori::agent_picker::get_agent_info(model)`, falling back to the raw model name if not found
2. `BottomPaneParams.model_display_name` carries the name to `BottomPane`
3. `BottomPane.set_model_display_name()` allows dynamic updates when agent switches
4. `ApprovalOverlay::new()` receives the display name and passes it to `exec_options()` / `patch_options()`
5. If the display name is empty, "the agent" is used as fallback

Updates occur via `set_pending_agent()` (when user selects new agent) and `set_model()` (when model changes), ensuring the approval dialog always reflects the active agent.

**Pending ExecCell Tracking:**

The `PendingExecCellTracker` (`chatwidget/pending_exec_cells.rs`) prevents duplicate ACP tool call messages in the chat history. The problem it solves:

1. Agent makes a tool call (e.g., `shell`) which creates an ExecCell in `active_cell`
2. Agent streams text *during* the tool call execution
3. Streaming text causes `flush_active_cell()`, which would normally push the incomplete ExecCell to history and clear `active_cell`
4. When `ExecCommandEnd` arrives, `handle_exec_end_now()` would create a *new* ExecCell since `active_cell` is empty
5. Result: duplicate entries for the same tool call

The tracker intercepts this by:
- `save_pending()`: Called during flush if the ExecCell has pending (incomplete) call_ids - saves the cell with ALL pending call_ids mapped to it (multi-key storage)
- `retrieve()`: Called in `handle_exec_end_now()` - retrieves and removes the saved cell by any of its call_ids, restoring it to `active_cell` for completion
- `drain_failed()`: Called in `on_task_complete()` - marks any uncompleted pending cells as failed and returns them for insertion into history

**Multi-Key Storage for Multi-Call Exploring Cells:**

Exploring cells (Read, ListFiles, Search operations) can group multiple tool calls into a single ExecCell. When such a cell is flushed during streaming and completion events arrive out-of-order (e.g., call-2 completes before call-1), the tracker must be able to retrieve the cell by ANY of its pending call_ids:

```
call_id_to_primary: { "call-1" -> "call-1", "call-2" -> "call-1", "call-3" -> "call-1" }
cells:              { "call-1" -> ExecCell }
```

When `retrieve("call-2")` is called, it looks up the primary key via `call_id_to_primary`, removes all mappings for that cell, and returns the cell.

**ExecCell Completion Handling (`handle_exec_end_now`):**

After completing a call, the handler decides whether to keep the cell visible or flush it:

1. **Cell still has pending calls** (`is_active()`): Keep in `active_cell` so it remains visible during streaming
2. **Cell fully complete AND exploring**: Keep in `active_cell` to allow grouping with subsequent exploring commands
3. **Cell fully complete AND NOT exploring**: Flush to history immediately

This ensures exploring cells remain visible during streaming instead of disappearing into `pending_exec_cells`.

**ExecCell Lifecycle Tracing:**

The TUI provides detailed tracing for debugging ExecCell state transitions:

```bash
RUST_LOG=tui_event_flow=debug,cell_flushing=debug,pending_exec_cells=debug cargo run
```

| Target | What it logs |
|--------|-------------|
| `tui_event_flow` | Event reception (`on_exec_command_begin`, `on_exec_command_end`) with cell state |
| `cell_flushing` | `flush_active_cell` decisions (save to pending vs flush to history) |
| `pending_exec_cells` | `save_pending`, `retrieve`, `drain_failed` operations with call_id mappings |

Combined with `acp_event_flow` from the ACP backend, these enable full end-to-end debugging of tool call display issues. See `@/codex-rs/tui/src/chatwidget/EXEC_CELL_LIFECYCLE.md` for comprehensive documentation.

**ACP File Tracing:**

- ACP rolling file tracing is initialized by the CLI (`cli/src/main.rs`) at startup, writing to `$NORI_HOME/log/nori-acp.YYYY-MM-DD`. Every mock agent logs `ACP agent spawned (pid: ...)` there, which makes the agent-switching tests in `tui-pty-e2e` deterministic and ensures developers can inspect agent subprocess lifecycles during debugging.

**Agent Switch Event Filtering:**

When switching between ACP agents (e.g., via `/agent` command), `ChatWidget` uses an event filtering mechanism to prevent race conditions:

- `expected_model: Option<String>` in `ChatWidgetInit` specifies which model the widget expects
- `session_configured_received: bool` tracks whether `SessionConfigured` has arrived from the expected model
- When `expected_model` is set, `handle_codex_event()` filters events:
  - All events are ignored until `SessionConfigured` arrives
  - `SessionConfigured` is only accepted if `event.model` matches `expected_model` (case-insensitive)
  - Once matching `SessionConfigured` arrives, `session_configured_received` is set to `true` and normal event processing resumes
- This prevents the OLD agent's final events (completion, shutdown) from being processed by the NEW agent's widget
- Fresh sessions, resumed sessions, and `/new` command use `expected_model: None` (no filtering)

**Color System:**

The `color.rs` and `terminal_palette.rs` modules handle terminal color detection and theming. The app queries terminal colors at startup for theme adaptation.

**Test Infrastructure:**

- `test_backend.rs`: Test terminal backend for snapshot testing
- Uses `insta` for snapshot tests of rendered output
- `AGENTS.md` documents testing conventions
- Black-box integration tests in `@/codex-rs/tui-pty-e2e` test full TUI via PTY
- Integration tests spawn real `nori` binary with `mock-acp-agent` backend

**System Info and Effective CWD Tracking:**

The footer displays system information (git branch, Nori profile, Nori version, git stats) that is collected asynchronously. The system also dynamically updates when the agent works in different directories (e.g., git worktrees).

*Initial Collection:*

1. `BottomPane::new()` initializes the footer with default (empty) `SystemInfo`
2. `App::run()` spawns a background thread that calls `SystemInfo::collect_fresh()`
3. When collection completes, the thread sends `AppEvent::SystemInfoRefreshed(info)`
4. `App` receives the event and calls `ChatWidget::apply_system_info_refresh()`
5. The update propagates through `BottomPane::set_system_info()` to the composer

*Dynamic CWD Tracking:*

The ACP protocol sets CWD at session creation and it is immutable. However, when the agent works in different directories (e.g., git worktrees created via `/skill using-git-worktrees`), the footer dynamically updates:

1. `ChatWidget` maintains an `EffectiveCwdTracker` initialized with the session CWD
2. Directory changes are detected from two event sources:
   - **Shell commands**: `handle_exec_begin_now()` calls `observe_directory(ev.cwd)`
   - **File writes**: `on_patch_apply_begin()` and `handle_patch_apply_end_now()` call `observe_directories_from_changes()`, which extracts parent directories from file paths and calls `observe_file_path()` for each
3. For file paths, relative paths are resolved against `config.cwd` before extracting the parent directory
4. The tracker uses 500ms debounce to avoid flickering during rapid operations in different directories
5. When effective CWD changes (debounce threshold met), ChatWidget sends `AppEvent::RefreshSystemInfoForDirectory(dir)`
6. App spawns a background thread calling `SystemInfo::collect_for_directory(&dir)`
7. The thread sends `AppEvent::SystemInfoRefreshed(info)` when complete
8. Footer re-renders with the new directory's git branch and stats

```
┌────────────────────┐  observe_directory()       ┌──────────────────────┐
│ handle_exec_begin  │ ──────────────────────────▶│                      │
└────────────────────┘                            │                      │
                                                  │ EffectiveCwdTracker  │
┌────────────────────┐  observe_file_path()       │                      │
│ on_patch_apply_*   │ ──────────────────────────▶│                      │
└────────────────────┘                            └──────────┬───────────┘
                                                             │ (if changed after 500ms)
     ┌───────────────────────────────────────────────────────┘
     │  AppEvent::RefreshSystemInfoForDirectory
     ▼
┌─────────────┐     spawn thread     ┌─────────────────────────┐
│     App     │ ─────────────────────▶│ collect_for_directory() │
└─────────────┘                       └───────────┬─────────────┘
                                                  │
     ┌────────────────────────────────────────────┘
     │  AppEvent::SystemInfoRefreshed
     ▼
┌───────────────────┐     set_system_info()    ┌────────────┐
│ ChatWidget        │ ─────────────────────────▶│ BottomPane │
└───────────────────┘                          └────────────┘
```
*Git Worktree Detection:*

`SystemInfo` includes an `is_worktree: bool` field that indicates whether the current directory is a git worktree (not the main repository). Detection works by comparing `git rev-parse --git-common-dir` with `--git-dir` - in a worktree, these paths differ.

The footer uses visual differentiation:
- **Yellow** branch indicator: Main repository
- **Orange** branch indicator: Git worktree (RGB 255, 165, 0)

For E2E testing, `NORI_SYNC_SYSTEM_INFO=1` env var enables synchronous collection in debug builds. This is set automatically by `@/codex-rs/tui-pty-e2e` to ensure footer data appears immediately in tests.

**Configuration Flow:**

TUI respects config overrides from:
1. CLI flags (`--model` always available; `--sandbox`, `--oss`, `-a`, `--full-auto`, etc. require `codex-features`)
2. `-c key=value` overrides
3. Config profiles (`-p profile-name`)
4. `~/.nori/cli/config.toml`

Created and maintained by Nori.
