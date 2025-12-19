# Noridoc: tui

Path: @/codex-rs/tui

### Overview

The `codex-tui` crate provides the interactive terminal user interface for Codex, built with the Ratatui framework. It handles the fullscreen TUI experience including chat display, input composition, onboarding flows, session management, and real-time streaming of model responses with markdown rendering.

### How it fits into the larger codebase

TUI is one of the primary entry points, invoked when running `codex` without a subcommand:

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
- `system_info.rs`: Background system info collection for footer (git branch, Nori profile/version, git stats)

**Input Handling:**

- `public_widgets/composer_input.rs`: Text input with multi-line support
- `clipboard_paste.rs`: Clipboard integration
- `slash_command.rs`: `/command` parsing and execution
- `file_search.rs`: Fuzzy file finder

**ACP Agent Switching:**

- `/agent` opens the Nori-specific agent picker popup in `tui/src/chatwidget.rs`, which drives `nori::agent_picker::agent_picker_params()` and renders the metadata returned by `codex_acp::list_available_agents()` as `SelectionItem`s.
- Selecting an agent sends `AppEvent::SetPendingAgent`, so both the App and `ChatWidget` store a `pending_agent` (see `PendingAgentSelection` and `PendingAgentInfo`). The UI informs the user that the switch will happen on the next prompt submission.
- When the next prompt is submitted, `ChatWidget` intercepts the queued `UserMessage`, forwards it as `AppEvent::SubmitWithAgentSwitch`, and lets the App restart the conversation with the new model (clearing the pending flag, updating `Config`, shutting down the old conversation, and creating a `ChatWidget` with `expected_model` to filter out leftover events).
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

**Interrupt Queueing and Approval Handling:**

Most event types (exec begin/end, MCP calls, elicitation) are queued during active streaming and flushed when streaming completes via `InterruptManager`. However, **approval requests are handled immediately** (not deferred):

- `on_exec_approval_request()` and `on_apply_patch_approval_request()` call their handlers directly
- This prevents deadlocks in ACP mode where the agent subprocess blocks waiting for approval
- If approval were deferred, the agent would wait for approval, but TaskComplete (which flushes the queue) wouldn't arrive until the agent finished
- The `InterruptManager` still contains `ExecApproval` and `ApplyPatchApproval` variants for completeness, but these methods are marked `#[allow(dead_code)]`
- `on_task_complete()` calls `flush_interrupt_queue()` for any remaining queued items

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
- Integration tests spawn real `codex` binary with `mock-acp-agent` backend

**System Info Background Refresh:**

The footer displays system information (git branch, Nori profile, Nori version, git stats) that is collected asynchronously to avoid blocking TUI startup:

1. `BottomPane::new()` initializes the footer with default (empty) `SystemInfo`
2. `App::run()` spawns a background thread that calls `SystemInfo::collect_fresh()`
3. When collection completes, the thread sends `AppEvent::SystemInfoRefreshed(info)`
4. `App` receives the event and calls `ChatWidget::apply_system_info_refresh()`
5. The update propagates through `BottomPane::set_system_info()` to the composer

```
┌─────────────┐     spawn thread     ┌──────────────────┐
│  App::run() │ ─────────────────────▶│  collect_fresh() │
└─────────────┘                       └────────┬─────────┘
                                               │
     ┌─────────────────────────────────────────┘
     │  AppEvent::SystemInfoRefreshed
     ▼
┌───────────────────┐     set_system_info()    ┌────────────┐
│ ChatWidget        │ ─────────────────────────▶│ BottomPane │
└───────────────────┘                          └────────────┘
```

For E2E testing, `NORI_SYNC_SYSTEM_INFO=1` env var enables synchronous collection in debug builds. This is set automatically by `@/codex-rs/tui-pty-e2e` to ensure footer data appears immediately in tests.

**Configuration Flow:**

TUI respects config overrides from:
1. CLI flags (`--model`, `--sandbox`, etc.)
2. `-c key=value` overrides
3. Config profiles (`-p profile-name`)
4. `~/.codex/config.toml`

Created and maintained by Nori.
