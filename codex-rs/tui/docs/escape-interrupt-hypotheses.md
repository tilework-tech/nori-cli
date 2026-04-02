# Escape/Interrupt Event Ordering — Hypotheses

**Bug:** When the user presses Escape to interrupt the current conversational turn, the next user message does not begin working immediately. A second user message is required to begin the working state in the TUI.

## Confirmed Root Cause

**Hypothesis #1 (CONFIRMED):** Late `TurnLifecycle::Completed` from the cancelled prompt task resets a newer turn's working state.

When `Op::Interrupt` is processed, the ACP backend (`submit_and_ops.rs`) immediately emits `TurnLifecycle::Aborted`. But the spawned prompt task (`user_input.rs`) continues running until `connection.prompt()` returns after the agent processes the cancel. The task then **unconditionally** emits `TurnLifecycle::Completed`. If the user submits a new message before this late `Completed` arrives, the event sequence becomes:

```
Aborted → Started (new turn) → Completed (stale, from old turn)
```

The stale `Completed` calls `on_task_complete()` which sets `is_task_running(false)` and `turn_finished = true`, killing the new turn's working state.

**Fix applied:**
1. **ACP backend** (`submit_and_ops.rs`): Added `turn_generation` counter that increments on `Op::Interrupt`. The spawned prompt task captures the generation at spawn and only emits `Completed` if the generation still matches.
2. **TUI** (`event_handlers.rs`): Added guard in `handle_client_turn_lifecycle` that ignores `TurnLifecycle::Completed` when `!is_task_running()`, preventing stale events from resetting state.

---

## All 6 Hypotheses

### 1. Late `TurnLifecycle::Completed` from cancelled prompt task ✅ CONFIRMED

See above. The spawned prompt task unconditionally emits `Completed` when `connection.prompt()` returns, even after the turn was already aborted via `Op::Interrupt`.

### 2. `turn_finished` gate poisoning

The late `Completed` sets `turn_finished = true` in `on_task_complete()`. If the new task's `TurnLifecycle::Started` hasn't yet reset it to `false`, all tool events from the new task would be silently discarded via the `turn_finished` gate. This is a downstream effect of hypothesis #1 — once `Completed` fires, `turn_finished = true` blocks tool events until the next `on_task_started()`.

**Status:** Mitigated by the same fix as #1 (stale `Completed` is suppressed).

### 3. `active_update_tx` channel swap race

The SACP connection uses `Arc<Mutex<Option<Sender<SessionUpdate>>>>` for routing session notifications. When the old prompt task's `connection.prompt()` returns, it clears `active_update_tx` (sets to `None`) in its "uninstall" block. If the new prompt task has already installed its `update_tx`, the old task's teardown **drops the new task's sender**, breaking the update channel for the new turn.

Session updates for the new turn then fall through to the `persistent_tx` channel, which forwards them to the TUI via the persistent relay. Text still arrives, but through a different path.

**Status:** Real issue but text still reaches TUI via persistent fallback. The broken update channel means the new task's `update_handler` receives no data and exits early.

### 4. Op serialization vs task lifetime mismatch

`Op::Interrupt` and `Op::UserInput` are processed sequentially by the op forwarding task (`agent.rs`). But `handle_user_input()` spawns an async task that runs concurrently. Multiple spawned tasks can overlap — their `TurnLifecycle` events interleave unpredictably on the shared `backend_event_tx` channel.

The SACP JSON-RPC protocol also supports concurrent requests. With a single-threaded mock agent (using `LocalSet`), the second `PromptRequest` can be processed during the first prompt's `await` points (e.g., `sleep`).

**Status:** Real design issue. The interleaving is what causes the race in hypothesis #1.

### 5. Mock agent `cancel_requested` not resetting between prompts

Initially suspected that `Cell<bool> cancel_requested` might not reset, causing the second prompt to see a stale cancel flag.

**Status:** RULED OUT. The mock agent resets `cancel_requested.set(false)` at the start of every `prompt()` call (line 284).

### 6. Bounded channel backpressure dropping events

The `backend_event_tx` channel has capacity 32 (`mpsc::channel(32)`). During the overlap period where both old and new tasks emit events, the channel could fill. If `TurnLifecycle::Started` for the new task is dropped via `try_send`, the TUI would never enter working state.

**Status:** Unlikely in practice. The channel uses `send().await` (blocking), not `try_send`. However, the SACP notification handler uses `try_send` on `active_update_tx` and `persistent_tx`, which silently drops events when full. This could cause data loss under heavy streaming.

---

## Test Coverage

- `tui-pty-e2e/tests/escape_interrupt_ordering.rs`: Two E2E tests (`test_escape_then_resubmit_enters_working_state` and `test_escape_then_fast_resubmit_enters_working_state`) that reproduce the bug using a mock agent with configurable cancel delay. A third test (`test_escape_resubmit_with_sacp_tee`) captures ACP wire traffic for manual debugging.
