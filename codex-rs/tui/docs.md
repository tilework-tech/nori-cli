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

**Input Handling:**

- `public_widgets/composer_input.rs`: Text input with multi-line support
- `clipboard_paste.rs`: Clipboard integration
- `slash_command.rs`: `/command` parsing and execution
- `file_search.rs`: Fuzzy file finder

**ACP Agent Switching:**

- `/agent` now opens the Nori-specific agent picker popup in `tui/src/chatwidget.rs`, which drives `nori::agent_picker::agent_picker_params()` and renders the metadata returned by `codex_acp::list_available_agents()` as `SelectionItem`s.
- Selecting an agent sends `AppEvent::SetPendingAgent`, so both the App and `ChatWidget` store a `pending_agent` (see `PendingAgentSelection` and `PendingAgentInfo`). The UI informs the user that the switch will happen on the next prompt submission.
- When the next prompt is submitted, `ChatWidget` intercepts the queued `UserMessage`, forwards it as `AppEvent::SubmitWithAgentSwitch`, and lets the App restart the conversation with the new model (clearing the pending flag, updating `Config`, shutting down the old conversation, and creating a `ChatWidget` with `expected_model` to filter out leftover events).
- `/model` now checks `codex_acp::get_agent_config()`; if the workspace is in ACP mode it shows the disabled `acp_model_picker_params()` view that explicitly tells users to use `/agent` instead of selecting models directly.
- This workflow avoids disrupting active turns and powers the agent-switching verification in `tui-pty-e2e/tests/agent_switching.rs`, including the message-flow and pending-selection tests added in the last commits.

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
- `save_pending()`: Called during flush if the ExecCell has pending (incomplete) call_ids - saves the cell keyed by call_id instead of pushing to history
- `retrieve()`: Called in `handle_exec_end_now()` - retrieves and removes the saved cell, restoring it to `active_cell` for completion
- `drain_failed()`: Called in `on_task_complete()` - marks any uncompleted pending cells as failed and returns them for insertion into history

This follows the same encapsulation pattern as `InterruptManager`: self-contained state in its own module file with typed public methods instead of exposing raw data structures.

**ACP File Tracing:**

- The TUI calls `codex_acp::init_file_tracing()` at startup (`tui/src/lib.rs`) to write `.codex-acp.log` in the current directory. Every mock agent logs `ACP agent spawned (pid: ...)` there, which makes the agent-switching tests in `tui-pty-e2e` deterministic and ensures developers can inspect agent subprocess lifecycles during debugging.

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

**Configuration Flow:**

TUI respects config overrides from:
1. CLI flags (`--model`, `--sandbox`, etc.)
2. `-c key=value` overrides
3. Config profiles (`-p profile-name`)
4. `~/.codex/config.toml`

Created and maintained by Nori.
