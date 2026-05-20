# BTW Side Session — Application Spec

## Goal

Add a `/btw <question>` command to the Nori CLI that creates a secondary,
read-only ACP session on the **same agent connection** (same NDJSON stdio
wire), shares conversation context from the primary session, and answers a
one-off question without interrupting the primary session — even mid-turn.

## Core Requirements

1. **Same wire.** The BTW session uses a separate ACP `sessionId` but
   multiplexes on the same stdin/stdout NDJSON stream to the same agent
   subprocess. No second process is spawned.
2. **Non-interrupting.** The primary session's prompt turn is never touched,
   cancelled, or blocked. The BTW session runs concurrently on the same
   connection.
3. **Shared context.** The BTW session receives the full conversation prefix
   (user messages + assistant text responses) accumulated so far — including
   partially-streamed text from an in-flight primary turn.
4. **General to any ACP agent.** The context-sharing mechanism operates at the
   ACP wire/client layer. It does not depend on agent-specific transcript
   formats, filesystem paths, or the Draft `session/fork` RFD.

## Architecture

```
┌─────────────────────────────────────────────────────┐
│  Nori CLI (TUI)                                     │
│                                                     │
│  ┌───────────────┐   ┌──────────────────────────┐   │
│  │ Primary Prompt │   │ /btw question            │   │
│  │  (session A)   │   │  (session B — ephemeral) │   │
│  └───────┬───────┘   └─────────┬────────────────┘   │
│          │                     │                     │
│          ▼                     ▼                     │
│  ┌─────────────────────────────────────────────┐    │
│  │           SacpConnection                     │    │
│  │  (single agent subprocess, shared stdin/out) │    │
│  │  routes by sessionId                         │    │
│  └─────────────────┬───────────────────────────┘    │
└────────────────────┼────────────────────────────────┘
                     │ NDJSON over stdio
                     ▼
            ┌────────────────┐
            │  ACP Agent     │
            │  (e.g. claude- │
            │   agent-acp)   │
            │  handles N     │
            │  sessions per  │
            │  process       │
            └────────────────┘
```

### Context Sharing Strategy

**Wire-level history capture** (chosen — agent-agnostic):

The CLI already sees every ACP message flowing through `SacpConnection`. A
`ConversationHistoryCapture` module listens to:
- `prompt()` calls → captures user text
- `ConnectionEvent::SessionUpdate(AgentMessageChunk)` → accumulates assistant
  text
- `ConnectionEvent::SessionUpdate(AgentThoughtChunk)` → optionally captures
  thinking

Tool calls and results are excluded. The result is a chronological list of
`{ role, text }` turns. Mid-turn, the partial assistant text accumulated so
far is included.

This history is serialized into the BTW session's prompt as context. No
filesystem transcript discovery, no agent-specific parsing, no protocol
extensions required.

**Future enhancements:**
- `session/fork` (ACP Draft RFD) — when agents advertise the capability, fork
  the primary session instead of injecting history. The forked session inherits
  full conversation state natively.
- Transcript-from-disk — richer context via per-agent transcript discovery
  (existing `TranscriptRefresher` infrastructure).

## Minimal MVP Scope

### User Interaction

1. User types `/btw <question>` in the TUI input composer.
2. The question and a shimmer/spinner appear in the active history area.
3. The BTW response streams in. While streaming, the question + partial
   response are shown as "active" cells (not yet committed to scrollback).
4. On completion, the final question/answer pair is committed to the chat
   history as a distinct cell pair with a visual "BTW" label.
5. The primary session continues unaffected throughout.

### Protocol Changes

**New `Op` variant** in `nori-rs/protocol/src/protocol/mod.rs`:
```rust
Op::Btw { prompt: String }
```

**New `EventMsg` variants:**
```rust
EventMsg::BtwStarted(BtwStartedEvent)       // question submitted
EventMsg::BtwDelta(BtwDeltaEvent)            // streaming text chunk
EventMsg::BtwComplete(BtwCompleteEvent)      // final answer
EventMsg::BtwError(BtwErrorEvent)            // error / timeout
```

### Backend Changes (`nori-rs/acp/`)

#### Multi-session event routing in `SacpConnection`

Today, `SacpConnection` has a single `event_rx` channel. All
`ConnectionEvent` items flow into it. For BTW, the connection needs to route
events by `sessionId`:

- Primary session events → existing `event_rx` (consumed by the main reducer)
- BTW session events → a separate `btw_event_rx` channel (consumed by the BTW
  collector)

The `SessionUpdate` variants from the ACP schema carry a `session_id` field.
The SACP notification handler in `sacp_connection.rs` (the `on_notification`
closure) already receives typed `acp::SessionNotification` which includes
`session_id`. The routing adds a check: if the notification's `session_id`
matches a registered side session, send to the side channel; otherwise send to
the primary channel.

**No changes to `prompt()` method.** The BTW handler calls `create_session()`
and `prompt()` on the same `SacpConnection`. The prompt method already works
with any `SessionId`. The only constraint is that the agent must support
concurrent sessions (ACP spec says it should).

#### `ConversationHistoryCapture`

New module: `nori-rs/acp/src/backend/btw_history.rs`

Accumulates turns from the primary session's events. The `AcpBackend` event
reducer already processes every `ConnectionEvent`; the capture hooks into the
same stream.

```rust
struct ConversationHistoryCapture {
    turns: Vec<ConversationTurn>,
    current_assistant_text: String, // partial accumulation mid-turn
}

struct ConversationTurn {
    role: TurnRole,  // User | Assistant
    text: String,
}
```

#### BTW prompt builder

New module: `nori-rs/acp/src/backend/btw_prompt.rs`

Serializes conversation history + a read-only preamble + the user's question
into a single prompt string.

**Preamble:**
```
You are answering a brief side question while another agent session is
actively working on this machine.

IMPORTANT CONSTRAINTS:
- This is a one-shot, read-only environment
- DO NOT make file changes, run shell commands, or mutate the system
- DO NOT use any tools that modify files or execute commands
- The conversation history between the user and the working agent is
  shown below, but tool calls and their results have been omitted
- Answer the question concisely and directly
- If you don't have enough context to answer, say so
```

**Token budget:** ~50K tokens (~200K chars). Truncate oldest turns if
exceeded, keeping at least the last 10 exchanges. Add an
`[earlier conversation truncated]` marker.

#### BTW session handler

New module: `nori-rs/acp/src/backend/btw.rs`

Orchestrates the full lifecycle:
1. Create side session via `SacpConnection::create_session()`
2. Register the side session ID for event routing
3. Build prompt from `ConversationHistoryCapture` + preamble + question
4. Call `SacpConnection::prompt()` with the side session ID
5. Collect `AgentMessageChunk` events from the side channel, emit as
   `BtwDelta` events to the TUI
6. On completion, emit `BtwComplete`
7. Tear down: unregister side session (events revert to primary channel)

**Timeout:** 5 minutes, after which the side session is cancelled and a
`BtwError` with a timeout message is emitted.

**Concurrency guard:** One BTW session per connection at a time. A second
`/btw` while one is in-flight returns an error.

### TUI Changes (`nori-rs/tui/`)

**Minimal rendering approach:**

- `/btw` recognized as a slash command in the input composer
- On `BtwStarted`: render the question as a "BTW" labeled user cell, show a
  shimmer/spinner in the active area (same pattern as primary prompt thinking
  indicator)
- On `BtwDelta`: accumulate and render streaming text in the active area
- On `BtwComplete`: commit the question + answer as a pair of history cells
  with a distinct visual label ("BTW" prefix or dimmed separator)
- On `BtwError`: commit an error cell

The BTW cells are inline in the main chat history — no separate panel needed.

## Key Technical Findings

### `claude-agent-acp` supports multiple sessions per process

`ClaudeAcpAgent` maintains `sessions: { [key: string]: Session }`. Each
`session/new` call spawns an independent Claude Code subprocess internally.
Prompts across sessions run concurrently. The sprite-acp-bridge is a
transparent NDJSON pipe. No changes needed on the agent side.

### `SacpConnection` is structurally ready

The connection already:
- Uses `ConnectionTo<Agent>` which is `Send + Sync`
- Has `create_session()` and `prompt()` that accept `SessionId` parameters
- Tracks per-session `prompt_state` in a `HashMap<String, SessionPromptState>`
- Has a channel-based event system

The main gap: event routing is session-unaware (all events go to one channel).

### ACP protocol supports session multiplexing natively

Every ACP message carries `sessionId`. Multiple `session/new` calls on one
connection are protocol-legal. `SessionNotification` includes `session_id` for
demuxing. The protocol was designed for this.

### Existing precedent: `run_prompt_summary()`

`nori-rs/acp/src/backend/hooks.rs` already spawns a parallel ACP connection
for prompt summarization. It demonstrates the pattern (spawn, create session,
prompt, collect, tear down) — but uses a **separate** child process. BTW
adapts this pattern to the **same** connection.

## Edge Cases

1. **BTW while primary is mid-turn:** Works — different sessionIds, concurrent
   prompts. Agent handles them independently.
2. **BTW when no session exists:** Return error immediately.
3. **Concurrent BTW requests:** Reject with "BTW already in progress."
4. **BTW session crashes:** Teardown in cleanup. Error propagates as
   `BtwError`.
5. **Primary disconnects during BTW:** Side session cleanup hooks into
   connection close handler.
6. **Very long conversation history:** Truncate oldest turns, keep last 10
   exchanges.
7. **Agent doesn't support multiple sessions:** `session/new` fails, return
   error. No fallback to separate process (violates "same wire").

## Files to Change

| File | Change |
|------|--------|
| `nori-rs/protocol/src/protocol/mod.rs` | Add `Op::Btw`, `BtwStarted/Delta/Complete/Error` event variants |
| `nori-rs/acp/src/connection/sacp_connection.rs` | Add side-session event routing |
| `nori-rs/acp/src/connection/mod.rs` | Extend `ConnectionEvent` if needed |
| `nori-rs/acp/src/backend/btw_history.rs` | New — conversation history capture |
| `nori-rs/acp/src/backend/btw_prompt.rs` | New — prompt builder with preamble |
| `nori-rs/acp/src/backend/btw.rs` | New — BTW session lifecycle handler |
| `nori-rs/acp/src/backend/mod.rs` | Wire up BTW handler to Op dispatch |
| `nori-rs/tui/src/` | `/btw` command parsing, BTW cell rendering |

## Testing Strategy

1. **Unit: history capture** — feed `ConnectionEvent` items, assert correct
   turn extraction (user/assistant text captured, tool calls excluded,
   mid-turn partial text included).
2. **Unit: prompt builder** — given turns + question, assert prompt structure
   (preamble present, history formatted, question at end, truncation works).
3. **Integration: BTW Op round-trip** — submit `Op::Btw`, assert
   `session/new` + `session/prompt` sent on same connection, assert
   `BtwComplete` event emitted, assert side session torn down.
4. **Snapshot: TUI rendering** — BTW cells render with correct visual label.

All tests written before implementation (TDD).

## Out of Scope (v1)

- `session/fork` support (future enhancement)
- Transcript-from-disk context (future enhancement)
- Multiple concurrent BTW sessions
- BTW in broker/Slack integration (separate PR in sessions repo)
- Configurable model for BTW session
- Tool restrictions (prompt-based instruction only for v1)
