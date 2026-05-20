# BTW Side Session — Current Progress

## Status: Spec Complete, Awaiting Approval

## Research Completed

### ACP Protocol Analysis
- [x] Confirmed ACP supports multiple concurrent sessions on one connection
      (every message carries `sessionId`)
- [x] Found Draft `session/fork` RFD — exact match for BTW use case, but not
      universally supported. Noted as future enhancement.
- [x] Confirmed `claude-agent-acp` natively handles multiple sessions per
      process (`sessions: { [key: string]: Session }` map, each `session/new`
      spawns independent Claude Code subprocess)
- [x] Confirmed `sprite-acp-bridge` is a transparent NDJSON pipe — no changes
      needed on the sprite side

### CLI Codebase Analysis
- [x] `SacpConnection` (`nori-rs/acp/src/connection/sacp_connection.rs`):
      already has `create_session()` and `prompt()` accepting `SessionId`.
      Per-session prompt state tracked in `HashMap<String, SessionPromptState>`.
      Gap: event routing is session-unaware (single `event_rx` channel).
- [x] `run_prompt_summary()` (`nori-rs/acp/src/backend/hooks.rs`):
      existing precedent for parallel ACP sessions, but spawns a separate
      child process. BTW adapts the pattern to the same connection.
- [x] `Op` enum (`nori-rs/protocol/src/protocol/mod.rs`):
      `#[non_exhaustive]`, safe to extend with `Op::Btw`.
- [x] `EventMsg` enum: has `PromptSummary` precedent for side-channel results.
      BTW adds `BtwStarted/Delta/Complete/Error` variants.
- [x] `ConnectionEvent` (`nori-rs/acp/src/connection/mod.rs`):
      currently `SessionUpdate` and `ApprovalRequest` — may need a wrapper to
      carry `session_id` for routing, or routing can be done in the SACP
      notification handler before dispatch.

### Context Sharing Strategy Decision
- [x] Chose **wire-level history capture** as the general approach (agent-agnostic,
      no filesystem access, works mid-turn)
- [x] Documented `session/fork` and transcript-from-disk as future enhancements

## Implementation Tasks

### Phase 1: Protocol & Types
- [ ] Add `Op::Btw { prompt: String }` to `nori-rs/protocol/src/protocol/mod.rs`
- [ ] Add `BtwStartedEvent`, `BtwDeltaEvent`, `BtwCompleteEvent`, `BtwErrorEvent`
      structs and `EventMsg` variants

### Phase 2: Backend — History Capture
- [ ] Create `nori-rs/acp/src/backend/btw_history.rs`
- [ ] `ConversationHistoryCapture` struct accumulates user/assistant turns
- [ ] Wire into `AcpBackend` event reducer to capture from primary session events

### Phase 3: Backend — Prompt Builder
- [ ] Create `nori-rs/acp/src/backend/btw_prompt.rs`
- [ ] Read-only preamble + conversation history + question formatting
- [ ] Token budget enforcement with truncation

### Phase 4: Backend — Side Session Event Routing
- [ ] Modify `SacpConnection` to support side-session registration
- [ ] Add session-aware routing in the SACP notification handler
- [ ] Side session events go to a separate channel; primary events unchanged

### Phase 5: Backend — BTW Session Handler
- [ ] Create `nori-rs/acp/src/backend/btw.rs`
- [ ] Full lifecycle: create session → build prompt → send → collect → teardown
- [ ] 5-minute timeout, concurrency guard (1 BTW at a time)
- [ ] Wire into `AcpBackend` Op dispatch

### Phase 6: TUI — Command & Rendering
- [ ] Parse `/btw <question>` as a slash command in the input composer
- [ ] Render `BtwStarted` as a labeled user cell + shimmer
- [ ] Render `BtwDelta` as streaming text in active area
- [ ] Render `BtwComplete` as committed history cell pair with "BTW" label
- [ ] Render `BtwError` as an error cell
- [ ] Snapshot tests for BTW cell rendering

### Phase 7: Testing & Polish
- [ ] Unit tests: history capture (turn extraction, tool call exclusion,
      mid-turn partial text)
- [ ] Unit tests: prompt builder (structure, truncation, preamble)
- [ ] Integration test: BTW Op round-trip against mock ACP agent
- [ ] TUI snapshot tests for BTW cells
- [ ] `just fmt` + `just fix` + full test suite

## Decisions Log

| Date | Decision | Rationale |
|------|----------|-----------|
| 2026-05-20 | Wire-capture over transcript-from-disk | General to any ACP agent; no filesystem access or agent-specific parsing needed |
| 2026-05-20 | Same-wire multiplexing (not separate process) | Cliff's explicit requirement #1; ACP protocol supports it natively |
| 2026-05-20 | Inline TUI rendering (not separate panel) | Minimal MVP; TUI has no multi-panel infrastructure |
| 2026-05-20 | 5-minute timeout | Enough for a quick question; prevents orphaned side sessions |
| 2026-05-20 | One BTW at a time per connection | Simplicity for v1; can be relaxed later |

## Open Questions

1. Should BTW history capture include thinking blocks? (Currently: yes,
   optionally. They provide useful reasoning context but are verbose.)
2. Should tool call summaries be included? (Currently: no. Could add
   one-line summaries like "used shell tool" in a future pass.)
3. Token budget: 50K tokens (~200K chars) as starting limit?
