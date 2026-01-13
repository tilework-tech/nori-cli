# "Connecting to [Agent]" Status Indicator Implementation Plan

**Goal:** Show a "Connecting to [Agent]" shimmer status message while an ACP agent is starting up, providing user feedback during the potentially slow subprocess spawn (especially for npx/bunx package resolution).

**Architecture:** Add an `AgentConnecting` AppEvent that's sent before ACP backend spawn, handled by the chatwidget to show a status indicator with shimmer animation. The mock agent gains a configurable startup delay for testing.

**Tech Stack:** Rust (TUI framework), existing `StatusIndicatorWidget` with shimmer, E2E tests with `tui-pty-e2e`

---

## Testing Plan

I will add an E2E test that:
1. Configures the mock agent with a 2-second startup delay via `MOCK_AGENT_STARTUP_DELAY_MS` env var
2. Spawns the TUI with this configuration
3. Verifies "Connecting" appears in the screen contents during startup
4. Verifies the agent eventually becomes ready (shows the `›` prompt)

This tests BEHAVIOR (the user sees "Connecting" during slow agent startup) not just implementation.

I will also add a unit test that verifies the mock agent respects the startup delay env var.

NOTE: I will write *all* tests before I add any implementation behavior.

---

## Phase 1: Add Startup Delay to Mock Agent

### Step 1.1: Write E2E test for "Connecting" status

**File:** `/home/clifford/Documents/source/nori/cli/.worktrees/improve-agent-discovery/codex-rs/tui-pty-e2e/tests/agent_switching.rs`

Add a new test at the end of the file:

```rust
/// Test that "Connecting to [Agent]" status appears during slow agent startup
#[test]
#[cfg(target_os = "linux")]
fn test_connecting_status_during_slow_agent_startup() {
    // Configure mock agent with 2 second startup delay
    let config = SessionConfig::new()
        .with_model("mock-model".to_string())
        .with_agent_env("MOCK_AGENT_STARTUP_DELAY_MS", "2000");

    let mut session = TuiSession::spawn_with_config(24, 80, config).expect("Failed to spawn TUI");

    // Should see "Connecting" status while agent is starting up
    session
        .wait_for_text("Connecting", Duration::from_secs(1))
        .expect("Should show 'Connecting' status during slow startup");

    // Eventually the agent should be ready (prompt appears)
    session
        .wait_for_text("›", Duration::from_secs(5))
        .expect("TUI should eventually show prompt after agent connects");
}
```

**Run test (should fail - mock agent doesn't have startup delay, and TUI doesn't show "Connecting"):**
```bash
cargo t -p tui-pty-e2e --test agent_switching test_connecting_status_during_slow_agent_startup
```

### Step 1.2: Implement `MOCK_AGENT_STARTUP_DELAY_MS` in mock agent

**File:** `/home/clifford/Documents/source/nori/cli/.worktrees/improve-agent-discovery/codex-rs/mock-acp-agent/src/main.rs`

In the `initialize()` method, after the `MOCK_AGENT_HANG` check (~line 168), add:

```rust
// Support configurable startup delay for testing "Connecting" status
if let Ok(delay_str) = std::env::var("MOCK_AGENT_STARTUP_DELAY_MS")
    && let Ok(delay) = delay_str.parse::<u64>()
{
    eprintln!("Mock agent: sleeping for {}ms during startup", delay);
    tokio::time::sleep(tokio::time::Duration::from_millis(delay)).await;
}
```

**Run test again (should still fail - TUI doesn't show "Connecting" yet):**
```bash
cargo t -p tui-pty-e2e --test agent_switching test_connecting_status_during_slow_agent_startup
```

---

## Phase 2: Add AgentConnecting AppEvent

### Step 2.1: Add `AgentConnecting` variant to `AppEvent`

**File:** `/home/clifford/Documents/source/nori/cli/.worktrees/improve-agent-discovery/codex-rs/tui/src/app_event.rs`

Add new variant after `AgentSpawnFailed` (~line 222):

```rust
/// Agent is connecting (spawning subprocess). Show "Connecting to [Agent]" status.
/// Sent before AcpBackend::spawn() and cleared when SessionConfigured is received.
AgentConnecting {
    /// The display name of the agent being connected to
    display_name: String,
},
```

### Step 2.2: Emit `AgentConnecting` before spawning ACP backend

**File:** `/home/clifford/Documents/source/nori/cli/.worktrees/improve-agent-discovery/codex-rs/tui/src/chatwidget/agent.rs`

In `spawn_acp_agent()` (~line 166), before the `tokio::spawn` block, emit the event:

```rust
fn spawn_acp_agent(config: Config, app_event_tx: AppEventSender) -> SpawnAgentResult {
    let (codex_op_tx, mut codex_op_rx) = unbounded_channel::<Op>();

    // ... existing model command channel setup ...

    // Get agent display name for "Connecting" status
    let display_name = codex_acp::get_agent_display_name(&config.model)
        .unwrap_or_else(|| config.model.clone());

    // Emit "Connecting" status before spawning the backend
    app_event_tx.send(AppEvent::AgentConnecting {
        display_name: display_name.clone(),
    });

    tokio::spawn(async move {
        // ... existing spawn logic ...
    });

    // ... rest of function ...
}
```

### Step 2.3: Add `get_agent_display_name()` helper to acp crate

**File:** `/home/clifford/Documents/source/nori/cli/.worktrees/improve-agent-discovery/codex-rs/acp/src/registry.rs`

Add a public function:

```rust
/// Get the display name for an agent by model name.
/// Returns None if the agent is not registered.
pub fn get_agent_display_name(model_name: &str) -> Option<String> {
    get_agent_config(model_name)
        .ok()
        .map(|config| config.display_name)
}
```

---

## Phase 3: Handle AgentConnecting in the App/ChatWidget

### Step 3.1: Handle `AgentConnecting` in App

**File:** `/home/clifford/Documents/source/nori/cli/.worktrees/improve-agent-discovery/codex-rs/tui/src/app.rs`

In `handle_app_event()`, add handler after `AgentSpawnFailed` case (~line 1055):

```rust
AppEvent::AgentConnecting { display_name } => {
    tracing::info!(
        display_name = %display_name,
        "Agent connecting, showing status indicator"
    );
    self.chat_widget.show_connecting_status(&display_name);
}
```

### Step 3.2: Add `show_connecting_status()` to ChatWidget

**File:** `/home/clifford/Documents/source/nori/cli/.worktrees/improve-agent-discovery/codex-rs/tui/src/chatwidget.rs`

Add a new public method:

```rust
/// Show "Connecting to [Agent]" status indicator during agent startup.
/// This is called when an ACP agent is being spawned and may take time
/// (e.g., npx/bunx resolving dependencies).
pub fn show_connecting_status(&mut self, display_name: &str) {
    let header = format!("Connecting to {}", display_name);
    self.bottom_pane.ensure_status_indicator();
    self.bottom_pane.set_interrupt_hint_visible(false); // Can't interrupt during connect
    self.set_status_header(header);
    self.request_redraw();
}
```

### Step 3.3: Clear connecting status when agent is ready

The existing code already handles this - when `SessionConfigured` event arrives, the status indicator is hidden or replaced with the composer. No changes needed.

---

## Phase 4: Run Tests and Verify

### Step 4.1: Run the E2E test
```bash
cargo t -p tui-pty-e2e --test agent_switching test_connecting_status_during_slow_agent_startup
```

### Step 4.2: Run all agent_switching tests to ensure no regressions
```bash
cargo t -p tui-pty-e2e --test agent_switching
```

### Step 4.3: Run full test suite
```bash
cargo t --workspace
```

---

## Testing Details

The E2E test `test_connecting_status_during_slow_agent_startup` tests the actual user-visible BEHAVIOR:
- When an agent takes time to start (simulated by `MOCK_AGENT_STARTUP_DELAY_MS`), the user sees "Connecting" status
- After the agent connects, the normal prompt appears

This is not testing mocks or implementation details - it's testing what the user actually sees.

## Implementation Details

- `MOCK_AGENT_STARTUP_DELAY_MS` env var delays the mock agent's `initialize()` response
- `AgentConnecting` AppEvent is emitted synchronously before `tokio::spawn()` in `spawn_acp_agent()`
- The status indicator shows "Connecting to [Display Name]" with shimmer animation
- No interrupt hint is shown during connecting (nothing to interrupt yet)
- When `SessionConfigured` arrives, the connecting status is implicitly cleared

## Questions

1. Should the connecting status show elapsed time like the "Working" status does? (Current plan: no, keep it simple)
2. Should there be a timeout for connecting? (Current plan: no, let the spawn fail naturally if needed)
3. Should the "Connecting" status also be shown for HTTP fallback mode? (Current plan: only ACP mode for now)

---
