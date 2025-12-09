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
- **Integrates** `codex-feedback` for tracing/feedback collection

The `cli/` crate's `main.rs` dispatches to `codex_tui::run_main()` for interactive mode.

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

**ACP Agent Switching (`chatwidget/agent.rs`):**

The `spawn_acp_agent()` function runs a loop that supports dynamic agent switching:

```
┌─────────────────────────────────────────────────────────────────────────┐
│                    ACP Agent Switch Flow                                │
├─────────────────────────────────────────────────────────────────────────┤
│  1. User selects agent in /agent picker → sets pending_agent            │
│  2. Warning message shown: "history will be lost"                       │
│  3. User submits next prompt                                            │
│  4. submit_user_message() detects pending_agent                         │
│  5. Sends Op::OverrideTurnContext with new model                        │
│  6. Agent loop detects model change, sets pending_switch                │
│  7. Sends Op::Shutdown to current backend                               │
│  8. Waits for ShutdownComplete or event channel close                   │
│  9. Breaks inner loop, spawns new AcpBackend with new model             │
│ 10. New ACP subprocess started with new model configuration             │
└─────────────────────────────────────────────────────────────────────────┘
```

Key invariant: Agent switching is deferred to the next prompt submission. This ensures the current turn completes before switching, preventing data loss or inconsistent state during active streaming.

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
- `slash_command.rs`: `/command` parsing and execution (includes `/agent` for ACP agent picker)
- `file_search.rs`: Fuzzy file finder

**Slash Commands:**

| Command | Description |
|---------|-------------|
| `/model` | Choose model and reasoning effort (HTTP mode) |
| `/agent` | Choose ACP agent (opens picker, deferred switch) |
| `/approvals` | Choose approval/sandbox policy |
| `/new` | Start a new chat session |
| `/review` | Review current changes |
| `/diff` | Show git diff |
| `/status` | Show session configuration and token usage |

The `/agent` command opens a selection popup populated from `codex_acp::list_available_agents()`. Selection sets `pending_agent` which is applied on the next prompt submission.

**Onboarding:**

The `onboarding/` module handles first-run experience:
- Login screen (ChatGPT OAuth or API key)
- Trust screen (directory permission settings)
- Windows WSL setup instructions

**Session Management:**

- `resume_picker.rs`: UI for selecting sessions to resume
- `session_log.rs`: High-fidelity session event logging
- `chatwidget/session_header.rs`: Displays current model and pending agent selection

**Pending Agent Selection Pattern:**

The `ChatWidget` maintains a `pending_agent: Option<String>` field that holds a deferred agent selection:

1. User opens `/agent` picker and selects an agent
2. `SetPendingAgent` event fires, setting `pending_agent` and `session_header.pending_agent`
3. Warning message displayed: "Agent will switch to '{model}' on your next prompt. Conversation history will be lost."
4. On next `submit_user_message()`, if `pending_agent.take()` returns a different model:
   - Sends `Op::OverrideTurnContext` with new model
   - Sends `AppEvent::UpdateModel` to update UI
   - Clears `session_header.pending_agent`
   - Agent loop in `agent.rs` detects model change and restarts backend

### Things to Know

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
