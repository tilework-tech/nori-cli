# ACP Cancel Turn Boundary Bug Report

## Summary

Nori releases the ACP session back to the UI too early after a cancelled prompt turn.
In the reproduced Claude ACP session, Nori emits `SessionPhaseChanged(Idle)` and
`PromptCompleted(stop_reason=Cancelled)` immediately after the cancelled prompt
response, then accepts the user's next prompt as a fresh turn. That follow-up
prompt is then immediately completed with `stopReason=end_turn` and no content.

This appears to be a Nori turn-boundary handling bug, not an ACP adapter bug.
The reproduced wire stream is permissible ACP behavior and matches the shape
that other ACP clients are expected to tolerate.

This document records the investigation only. It does not propose a fix.

## Impact

- User presses `Ctrl-C` during an ACP agent turn.
- Nori shows the interrupted state and returns the prompt.
- The next user prompt is accepted immediately.
- That prompt is then completed immediately with an empty `end_turn`.
- The UI appears to consume a stale stop/completion signal instead of treating
  the post-cancel tail as part of the cancelled turn's completion lifecycle.

The captured TUI state is backed up in:

- `evidence/tui-capture-after-cancel.txt`
- `evidence/tui-capture-after-followup.txt`

## Reproduction

### Environment

- Branch: `debug-acp-cancel-ordering-trace`
- Binary: `codex-rs/target/debug/nori`
- Agent: `claude-debug-acp`
- Logging:
  - `RUST_LOG=acp_event_flow=debug,sacp::jsonrpc::handlers=debug,nori_tui=info`
- Runtime home:
  - `.tmp/claude-debug-repro-home-2`

### Steps

1. Start `nori --agent claude-debug-acp --skip-trust-directory`.
2. Submit:
   - `I'm testing something. Just run a foreground sleep 30 task, then say 'all done!'`
3. Wait until the tool call shows `sleep 30`.
4. Press `Ctrl-C`.
5. Submit:
   - `what have you finished`

### Observed Result

- The cancelled turn ends.
- Nori returns to idle and accepts the follow-up prompt.
- The follow-up prompt immediately receives `stopReason=end_turn` with zero usage.
- The TUI jumps to a fresh prompt with no assistant answer.

## Evidence Directory

All backed-up artifacts for this investigation live in:

- `spec/acp-cancel-turn-boundary-investigation/evidence/`

Artifacts:

- `wire-session.log`
- `nori-acp-trace.log`
- `nori-tui.log`
- `tui-capture-after-cancel.txt`
- `tui-capture-after-followup.txt`
- `acp-cancellation-spec.txt`
- `acp-session-update-note.txt`
- `sacp-ordering-excerpt.txt`
- `sacp-session-excerpt.txt`
- `toad-conversation-excerpt.py`
- `toad-agent-excerpt.py`
- `instrumentation.diff`

## Raw Wire Evidence

The reproduced wire sequence for the main session is:

1. Client sends `session/cancel`.
2. Agent sends `session/update` with `usage_update`.
3. Agent responds to the original prompt request with `stopReason=cancelled`.
4. Client sends the next `session/prompt`.
5. Agent immediately responds with `stopReason=end_turn` and zero usage.

See `evidence/wire-session.log`:

- line 22: `session/cancel`
- line 23: post-cancel `usage_update`
- line 24: cancelled prompt response
- line 25: next `session/prompt`
- line 26: immediate empty `end_turn`

This ordering is also visible in the original log copy:

- [codex-rs/debug-acp-claude.log](/home/clifford/Documents/source/nori/cli/.worktrees/debug-acp-cancel-ordering-trace/codex-rs/debug-acp-claude.log:22)

## ACP Specification References

The ACP reference explicitly allows final `session/update` traffic after cancel,
as long as those updates are sent before the cancelled prompt response:

- `acp-cancellation-spec.txt`
- Source reference:
  - `/home/clifford/Documents/source/nori/docs/references/acp-llms-full.txt:3048`
  - `/home/clifford/Documents/source/nori/docs/references/acp-llms-full.txt:3078`

Key points from the reference:

- The client may cancel with `session/cancel`.
- The agent must eventually respond to the original `session/prompt` with
  `stopReason=cancelled`.
- The agent may still send `session/update` notifications after receiving
  `session/cancel`, but before responding to `session/prompt`.
- The client should still accept those updates.

There is also a specific note in the `session/update` section that clients
should continue accepting updates after cancel:

- `acp-session-update-note.txt`
- Source reference:
  - `/home/clifford/Documents/source/nori/docs/references/acp-llms-full.txt:3895`

Nothing in the reproduced wire stream violates these requirements.

## Investigation Log

### 1. Initial hypothesis check

The first investigation pass considered whether Nori was locally reordering
`session/update` notifications and prompt results while merging:

- transport notifications from `event_rx`
- prompt results from `prompt_result_rx`

Instrumentation was added at:

- transport ingress:
  - [codex-rs/acp/src/connection/sacp_connection.rs](/home/clifford/Documents/source/nori/cli/.worktrees/debug-acp-cancel-ordering-trace/codex-rs/acp/src/connection/sacp_connection.rs:168)
- relay merge point:
  - [codex-rs/acp/src/backend/spawn_and_relay.rs](/home/clifford/Documents/source/nori/cli/.worktrees/debug-acp-cancel-ordering-trace/codex-rs/acp/src/backend/spawn_and_relay.rs:258)
- reducer:
  - [codex-rs/acp/src/backend/session_reducer.rs](/home/clifford/Documents/source/nori/cli/.worktrees/debug-acp-cancel-ordering-trace/codex-rs/acp/src/backend/session_reducer.rs:44)
- runtime driver:
  - [codex-rs/acp/src/backend/session_runtime_driver.rs](/home/clifford/Documents/source/nori/cli/.worktrees/debug-acp-cancel-ordering-trace/codex-rs/acp/src/backend/session_runtime_driver.rs:127)

That tracing established that Nori did see the post-cancel `usage_update` before
the cancelled prompt response in the reproduced run.

### 2. Added turn-boundary tracing

To determine exactly when Nori released control back to the UI, two more
tracing points were added:

- prompt admission into the ACP backend:
  - [codex-rs/acp/src/backend/user_input.rs](/home/clifford/Documents/source/nori/cli/.worktrees/debug-acp-cancel-ordering-trace/codex-rs/acp/src/backend/user_input.rs:164)
- emitted client events, especially:
  - `SessionPhaseChanged`
  - `PromptCompleted`
  - [codex-rs/acp/src/backend/session_runtime_driver.rs](/home/clifford/Documents/source/nori/cli/.worktrees/debug-acp-cancel-ordering-trace/codex-rs/acp/src/backend/session_runtime_driver.rs:304)

These extra traces are what closed the loop.

### 3. Reproduced with boundary tracing enabled

The decisive sequence in `evidence/nori-acp-trace.log` is:

1. Nori marks the active prompt as cancelling.
   - line 54
2. Nori forwards `SessionPhaseChanged(Cancelling)`.
   - line 56
3. Nori sends `session/cancel`.
   - line 57
4. Nori receives the cancelled prompt response.
   - lines 64-66
5. Nori finalizes that prompt response.
   - line 68
6. Nori forwards `SessionPhaseChanged(Idle)`.
   - line 70
7. Nori forwards `PromptCompleted(stop_reason=Cancelled)`.
   - line 71
8. Only after that, Nori accepts the user's follow-up prompt as a new turn.
   - line 72
9. That new turn immediately receives `stopReason=EndTurn`.
   - lines 78-85

This sequence is the key investigation result.

## What The Trace Proves

The new trace proves all of the following in the reproduced session:

### A. Nori can receive and parse both stop reasons off the wire

Nori successfully parses:

- the cancelled response for the interrupted turn
- the subsequent `end_turn` response that arrives after the next `session/prompt`

Evidence:

- `wire-session.log` lines 24 and 26
- `nori-acp-trace.log` lines 64-66 and 78-80

So this is not a failure to deserialize or understand the wire payloads.

### B. Nori releases the cancelled turn before the next logical stop boundary is fully resolved

Nori explicitly emits:

- `SessionPhaseChanged(Idle)` at line 70
- `PromptCompleted(Cancelled)` at line 71

and then admits the follow-up prompt at line 72.

That is the precise point where control returns to the UI and a new user turn
becomes possible.

### C. The follow-up prompt is treated as a brand new turn

The follow-up prompt is not queued behind additional cancel-tail processing.
It is accepted from idle:

- `phase_before_submit="idle"`
- `active_request_id_before_submit="<none>"`

Evidence:

- `nori-acp-trace.log` line 72

### D. The empty `end_turn` disrupts the following prompt turn, not the cancelled one

The prompt request for `what have you finished` is started with a new request id:

- line 73

That request then immediately gets `EndTurn`:

- lines 78-85

This means the disruptive `end_turn` is observed as the response to the new
prompt turn after Nori has already returned to idle.

## Comparison With SACP Ordering APIs

Nori's current prompt path uses `block_task()`:

- [codex-rs/acp/src/connection/sacp_connection.rs](/home/clifford/Documents/source/nori/cli/.worktrees/debug-acp-cancel-ordering-trace/codex-rs/acp/src/connection/sacp_connection.rs:527)

SACP documents that `block_task()` acknowledges the response immediately:

- `sacp-ordering-excerpt.txt`
- source:
  - `/home/clifford/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/sacp-10.1.0/src/jsonrpc.rs:2747`

By contrast, `on_receiving_result()` keeps ordering until the callback completes:

- `sacp-ordering-excerpt.txt`
- source:
  - `/home/clifford/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/sacp-10.1.0/src/jsonrpc.rs:2915`

The session-oriented helper in SACP uses `on_receiving_result()` for prompt
completion and then keeps reading updates until a stop reason is drained:

- `sacp-session-excerpt.txt`
- source:
  - `/home/clifford/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/sacp-10.1.0/src/session.rs:554`
  - `/home/clifford/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/sacp-10.1.0/src/session.rs:588`

This comparison does not by itself prove the root cause, but it is relevant
because Nori's current prompt handling takes the "ack immediately and process
later" path rather than a session-scoped "keep consuming until the turn is
actually over" path.

## Comparison With Toad

The example ACP client `toad` is also useful as a reference point.

Its conversation layer waits for `agent.send_prompt(prompt)` and only then
calls `agent_turn_over(stop_reason)`:

- `toad-conversation-excerpt.py`
- source:
  - `/home/clifford/Documents/source/nori/cli/.worktrees/plan-session-update-support/toad/src/toad/widgets/conversation.py:822`

Its ACP agent layer waits on the ACP prompt request and returns the stop reason:

- `toad-agent-excerpt.py`
- source:
  - `/home/clifford/Documents/source/nori/cli/.worktrees/plan-session-update-support/toad/src/toad/acp/agent.py:739`

Again, this report is not claiming a fix from Toad's implementation. The
comparison is included because the same adapter is known to work in other ACP
clients, so Nori's turn-boundary handling is the thing under investigation.

## Conclusion

The evidence gathered here supports the following bug statement:

> Nori completes and releases a cancelled ACP turn too early. It forwards
> `PromptCompleted(Cancelled)` and `SessionPhaseChanged(Idle)` immediately after
> the cancelled prompt response, then admits the next user prompt as a fresh
> turn. In the reproduced session, that fresh turn is immediately completed by
> an empty `end_turn`, producing the visible off-by-one stop-boundary bug.

This report intentionally stops at attribution and evidence. It does not
recommend or document a fix.
