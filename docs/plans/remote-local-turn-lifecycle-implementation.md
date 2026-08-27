# Remote Local-Turn Lifecycle Bugfix Implementation Plan

**Goal:** Make a remote Nori frontend observe a locally initiated turn once, render all agent output, return to idle, and fail remote prompt attempts while that local turn owns the session.

**Architecture:** The harness emits ACP-visible `working`/`idle` lifecycle hints through the existing shared session fan-out. Prompt admission stays serialized in the harness reducer. The remote host only enforces controller ownership, rewrites the session ID, and forwards ACP events.

**Tech Stack:** Rust, Tokio, ACP v1 `session/update`, existing Nori `_meta`, mock ACP agent, Cargo tests

---

## Testing Plan

Add failing reducer tests in `nori-rs/harness/src/backend/session_reducer/tests.rs`:

- A reject-if-busy prompt is not queued and does not disturb the active local turn.
- Ordinary local prompts retain the existing queue behavior.
- A Nori `working`/`idle` envelope bounds observer activity without creating a local request; bounded chunks do not produce the orphan-update warning, while truly unowned chunks still do.

Replace the queued-remote-prompt expectation and add lifecycle coverage in `nori-rs/harness/tests/remote_host.rs`:

- A local prompt produces outward events in this order: `working`, canonical user chunks, agent chunks, `idle`.
- The observer receives no prompt response for a request it did not initiate.
- A remote prompt attempted during that turn fails immediately and never enters the local queue.
- Remote cancel cannot cancel a local-owned turn.
- Remote-owned prompts still stream updates and receive their correlated response.
- Every forwarded notification has only its session ID rewritten.

Strengthen `nori-rs/tui/src/chatwidget/tests/part10.rs` so non-newline agent chunks become visible on `idle`, the proactive Working state clears, and the submitter still renders its prompt once.

<!-- prettier-ignore -->
NOTE: I will write *all* tests before I add any implementation behavior.

## Observed Failure

The agent response was present in the trace. The remote TUI buffered its non-newline chunks, then stayed in proactive Working because no observer-visible completion arrived. The remote host also queued remote prompts behind local activity and allowed remote cancel to target whichever turn was active.

## Implementation Tasks

### 1. Add serialized prompt admission

**Files:**

- `nori-rs/harness/src/runtime.rs`
- `nori-rs/harness/src/backend/submit_and_ops.rs`
- `nori-rs/harness/src/backend/session_reducer.rs`
- `nori-rs/harness/src/backend/session_runtime_driver.rs`

Add an explicit prompt admission policy: normal callers queue; the remote controller uses reject-if-busy. Make the reducer decide admission so simultaneous submissions cannot pass a stale phase check. A rejection resolves the pending prompt request immediately with a typed busy error and leaves phase, active request, and queue unchanged.

Keep `HarnessHandle::prompt()` and local TUI behavior unchanged. Add a crate-private remote submission method that selects reject-if-busy.

### 2. Publish observer lifecycle through the shared fan-out

**Files:**

- `nori-rs/harness/src/backend/session_runtime_driver.rs`
- `nori-rs/harness/src/normalized/session_runtime.rs`
- `nori-rs/harness/src/backend/session_reducer.rs`

Publish `SessionInfoUpdate` notifications with `_meta.nori.status`:

- `working` after the downstream prompt receives its wire request ID and before canonical user chunks;
- `idle` after success, cancellation, or failure, after all agent chunks.

Send these as ordinary public ACP notifications. Do not synthesize assistant text: the original agent chunks are already present, and `idle` flushes the observer's stream buffer.

Track the Nori status as observer activity separate from local `SessionPhase`. It may suppress the orphan warning only between `working` and `idle`; it must not create an owned request, affect queueing, or suppress statusless unowned/load/replay content.

### 3. Enforce remote-controller ownership

**File:** `nori-rs/harness/src/remote_agent.rs`

Use reject-if-busy for remote `session/prompt` and map the typed busy result to a stable ACP server error. Forward `session/cancel` only when the active harness request is in `remote_turns`; otherwise leave the local turn untouched.

Keep local-turn prompt outcomes private because no remote JSON-RPC request exists to answer. Forward their lifecycle and content notifications normally. Do not add response synthesis or content buffering to the remote host.

### 4. Document the now-defined coexistence policy

**File:** `docs/specs/remote-acp-transport.md`

Replace the deferred simultaneous-input language with:

- the first admitted prompt owns the active turn;
- local prompts keep normal queue semantics;
- remote prompts fail while another turn is active;
- remote cancel affects only a remote-owned turn;
- observers receive `working`, content, and `idle`, but no foreign prompt response.

State that `_meta.nori.status` is an optional Nori lifecycle hint carried by standard ACP `session/update`; non-Nori clients may ignore it.

## Compatibility and Edge Cases

- No Handroll, WebSocket, HTTPS/SSE, or downstream-agent changes.
- ACP clients that ignore `_meta` continue receiving the same content stream.
- Duplicate lifecycle hints from a nested Nori agent are idempotent and must not duplicate content.
- `idle` must be emitted for cancellation and transport failure so observers cannot remain Working.
- Reconnect/load replay remains content-complete; lifecycle hints do not claim request ownership.
- A rejected remote prompt must not later start after the local turn ends.

## Verification

From `nori-rs/`:

```bash
cargo build -p mock-acp-agent
MOCK_ACP_AGENT_BIN="$PWD/target/debug/mock_acp_agent" cargo test -p nori-harness --test remote_host
cargo test -p nori-harness backend::session_reducer::tests
cargo test -p nori-tui proactive_turn
cargo build --bin nori
```

Then repeat the two-terminal LAN test: start a local turn on the agent TUI, confirm the remote TUI renders its prompt and full response, returns to idle, and receives an immediate busy error if it submits during that turn.

**Testing Details**

Tests assert public event order and user-visible TUI state, not private field choreography. The current baseline passes the focused remote-host and proactive-turn suites when `MOCK_ACP_AGENT_BIN` points to the built underscore-named binary.

**Implementation Details**

The key invariant is one shared ACP notification sequence for all frontends. The remote host remains a thin ownership/correlation boundary; admission and lifecycle truth remain in the serialized harness.

**Question**

Use the existing non-retryable Nori code `-32015` for “session already active,” or reserve a new server-defined code specifically for “turn busy”? This plan assumes `-32015` unless review finds client behavior that makes it unsafe.

---
