# Bug Report: `session.cancelled` flag reset race in `@zed-industries/claude-agent-acp@0.23.1`

## Summary

After pressing Escape to cancel a running prompt, the next user message is silently swallowed. The agent returns `stopReason: "end_turn"` with zero tokens and no content. A second message after that works normally.

The root cause is a race condition inside `@zed-industries/claude-agent-acp` where the incoming prompt's `prompt()` handler resets `session.cancelled = false` (line 226 of `acp-agent.js`) synchronously before yielding, clearing the flag before the still-running prompt loop can observe it.

## Expected behavior

1. User submits a prompt; agent begins streaming (e.g. a tool call).
2. User presses Escape. Client sends `session/cancel`.
3. User submits a new prompt. Client sends `session/prompt`.
4. Agent returns `stopReason: "cancelled"` for the first prompt.
5. Agent processes the second prompt normally and streams a response.

## Actual behavior

Steps 1-3 are the same. Then:

4. Agent returns `stopReason: "end_turn"` for the second prompt with **zero tokens** (inputTokens: 0, outputTokens: 0).
5. No thoughts, no message chunks, no tool calls — nothing is streamed for the second prompt.
6. A third prompt works normally.

## Wire evidence (ACP capture log)

Captured via `sacp-tee` between Nori CLI and `claude-agent-acp@0.23.1`. Session ID `49e83953`. Irrelevant fields (available_commands, etc.) elided.

### Cancel and immediate resubmit

```
→ session/cancel  {"sessionId":"49e83953-..."}
← usage_update    (session 49e83953)
← {"id":"2cb22ee9-...","result":{"stopReason":"cancelled"}}
→ session/prompt   {"sessionId":"49e83953-...","prompt":[{"text":"testing again? part 2"}],"id":"427769e8-..."}
← {"id":"427769e8-...","result":{"stopReason":"end_turn","usage":{"inputTokens":0,"outputTokens":0,"cachedReadTokens":0,"cachedWriteTokens":0,"totalTokens":0}}}
```

Key observations:

- The cancelled response for prompt 1 (`2cb22ee9`) arrives **before** prompt 2 (`427769e8`) is sent. The prompts are serialized on the wire — there is no overlap.
- Despite correct serialization, prompt 2 returns `end_turn` with **all-zero usage**.
- No `session/update` notifications (thoughts, message chunks) appear between the prompt 2 request and response.

### Third prompt works normally

```
→ session/prompt  {"sessionId":"49e83953-...","prompt":[{"text":"still no message..."}],"id":"8d255182-..."}
← agent_thought_chunk  "The user interrupted the previous sleep command..."
← agent_message_chunk  "Running the sleep again."
← tool_call            sleep 30
← tool_call_update     completed
← agent_message_chunk  "Done!"
← usage_update
← {"id":"8d255182-...","result":{"stopReason":"end_turn","usage":{"inputTokens":4,"outputTokens":196,...}}}
```

The third prompt is processed normally with full streaming output.

### TUI screen capture

```
› testing again? part 2

                                          ← no assistant response visible

› still no message...

• Running the sleep again.
• Ran sleep 30
  └ (Bash completed with no output)
• Done!
```

## Reproduction

- **Reproducible with:** `@zed-industries/claude-agent-acp@0.23.1` (Claude Code agent)
- **Not reproducible with:** `mock-acp-agent` (Nori's mock agent for testing)

The mock agent does not implement prompt queuing. Its `prompt()` handler processes each request independently, so the flag-reset race does not exist.

## Root cause analysis

### Relevant code

All line numbers reference:
`@zed-industries/claude-agent-acp@0.23.1` — `dist/acp-agent.js`

### Shared mutable state

The `session` object (defined at line 21 of `acp-agent.d.ts`) contains:

```ts
cancelled: boolean;       // session-wide cancel flag
promptRunning: boolean;    // true while a prompt loop is active
pendingMessages: Map<string, { resolve: (cancelled: boolean) => void; order: number }>;
```

All concurrent `prompt()` invocations for the same session share this state.

### The race sequence

**Step 1 — Prompt 1 is running.**

`session.promptRunning = true`. The message loop (line 265) is suspended at:

```js
// line 266
const { value: message, done } = await session.query.next();
```

**Step 2 — `cancel()` is called (line 552).**

```js
session.cancelled = true;                      // line 557 — set flag
for (const [, pending] of session.pendingMessages) {
    pending.resolve(true);                     // line 559 — resolve existing pending prompts as cancelled
}
session.pendingMessages.clear();               // line 561
await session.query.interrupt();               // line 562 — interrupt SDK query
```

At this point `session.cancelled === true` and `session.query` has been interrupted.

**Step 3 — `prompt()` is called for Prompt 2 (line 221).**

In JavaScript's single-threaded event loop, the `prompt()` handler runs **synchronously** until its first `await`. The synchronous preamble:

```js
session.cancelled = false;                     // line 226 — RESETS THE FLAG
session.accumulatedUsage = { ... };            // line 227-232 — zeroes usage
// ...
if (session.promptRunning) {                   // line 246 — true (Prompt 1 still active)
    session.input.push(userMessage);           // line 247 — push to input queue
    const cancelled = await new Promise(...)   // line 249 — YIELDS HERE
```

**Line 226 resets `session.cancelled` to `false` before yielding.** Prompt 2's pending promise is created at line 250 and was not present when `cancel()` cleared `pendingMessages` in step 2, so it is not resolved by the cancel.

**Step 4 — Prompt 1's message loop resumes.**

The `session.query.interrupt()` from step 2 causes `session.query.next()` to resolve. The loop checks:

```js
// line 267-271
if (done || !message) {
    if (session.cancelled) {                   // FALSE — Prompt 2 reset it in step 3
        return { stopReason: "cancelled" };
    }
    break;                                     // exits the while loop
}
```

Because `session.cancelled` is now `false`, the loop **does not return `"cancelled"`**. It breaks out of the `while(true)` loop and falls through to:

```js
// line 516
throw new Error("Session did not end in result");
```

Alternatively, if `session.query.next()` yields a `system` → `session_state_changed` → `idle` message before returning `done`, the handler returns:

```js
// line 316-317
if (message.state === "idle") {
    return { stopReason: "end_turn", usage: sessionUsage(session) };
}
```

This matches the observed wire behavior: `end_turn` with zero usage (the accumulatedUsage was zeroed by Prompt 2 at line 227-232).

**Step 5 — The `finally` block executes (line 536).**

```js
finally {
    if (!handedOff) {
        session.promptRunning = false;         // line 538
        if (session.pendingMessages.size > 0) {
            const next = [...session.pendingMessages.entries()]
                .sort((a, b) => a[1].order - b[1].order)[0];
            if (next) {
                next[1].resolve(false);        // line 545 — resolves Prompt 2's pending promise
                session.pendingMessages.delete(next[0]);
            }
        }
    }
}
```

Prompt 2's pending promise is resolved with `false` (not cancelled), so Prompt 2's `prompt()` resumes at line 252:

```js
if (cancelled) {                               // false
    return { stopReason: "cancelled" };
}
promptReplayed = true;                         // line 257
```

**Step 6 — Prompt 2 takes over the message loop.**

Prompt 2 enters the `while(true)` loop at line 265, calling `session.query.next()`. But the SDK query was already interrupted in step 2 and Prompt 1 consumed whatever final messages it produced. The query may immediately return `done: true`, or produce `session_state_changed → idle`, causing Prompt 2 to return `end_turn` with the zeroed usage from line 227-232.

This is the swallowed prompt.

## Version history: this is a regression in 0.23.x

npm publish dates:

| Version | Published | Prompt queuing | `session_state_changed` handler | Bug present? |
|---------|-----------|----------------|----------------------------------|-------------|
| 0.18.0 | 2026-02-18 | No | No | No |
| 0.22.2 | 2026-03-18 | Yes | No | Likely no (see below) |
| 0.23.0 | 2026-03-25 | Yes | **Yes (new)** | **Yes** |
| 0.23.1 | 2026-03-26 | Yes | **Yes** | **Yes** |

### 0.18.0 — No prompt queuing, no race

`prompt()` (line 340) is a simple loop with no `promptRunning`, no `pendingMessages`, no queuing logic. `cancel()` (line 475) just sets `session.cancelled = true` and calls `query.interrupt()`. There is no mechanism for two prompt handlers to run concurrently within the agent, so the flag-reset race cannot occur.

### 0.22.2 — Prompt queuing added, but no `session_state_changed`

Prompt queuing infrastructure appears: `promptRunning`, `pendingMessages`, `nextPendingOrder`, and the `session.cancelled = false` reset at line 223. The `cancel()` handler (line 538) now resolves pending messages.

The flag-reset race EXISTS in this version at the code level. However, without the `session_state_changed → idle` handler, the race has a narrower impact. When `session.cancelled` is cleared by the incoming prompt, the running loop's `done` check (line 256-260) falls through to `throw new Error("Session did not end in result")`. This error is caught and re-thrown, which the ACP framework reports as an error response — not a silent `end_turn` with zero tokens. The user would see an error, not a swallowed prompt.

### 0.23.0/0.23.1 — `session_state_changed → idle` makes the race silent

Two changes in the 0.22.2 → 0.23.x diff introduce the silent failure:

**1. `CLAUDE_CODE_EMIT_SESSION_STATE_EVENTS` env var (line 961-962 of 0.23.1):**

```js
// Opt-in to session state events like when the agent is idle
CLAUDE_CODE_EMIT_SESSION_STATE_EVENTS: "1",
```

This causes the Claude Agent SDK to emit `session_state_changed` events, including `idle` after an interrupt.

**2. New `session_state_changed` handler (lines 315-319 of 0.23.1):**

```js
case "session_state_changed": {
    if (message.state === "idle") {
        return { stopReason: "end_turn", usage: sessionUsage(session) };
    }
    break;
}
```

Neither of these exist in 0.22.2 (confirmed: `grep -n 'session_state_changed\|EMIT_SESSION_STATE' 0.22.2/acp-agent.js` returns no matches).

**How this converts the race from an error into a silent swallow:**

After cancel + interrupt, the SDK transitions to `idle` and emits `session_state_changed → idle`. In 0.22.2, this event type was unhandled (would fall through to `default: unreachable()`). In 0.23.x, the handler catches it and returns `{ stopReason: "end_turn" }` — a clean-looking response with zero usage (because `accumulatedUsage` was zeroed by the incoming prompt at line 227-232).

The stale `idle` event from the cancelled prompt's interrupt may also linger in the query stream. When the next prompt takes over the loop, it immediately encounters this leftover `idle` event and returns `end_turn` with zeroed usage — the swallowed prompt.

**3. Cancel check reordering (lines 361-366 of 0.23.1):**

The cancel check inside the `result` handler was moved from after `if (!promptReplayed)` (0.22.2 line 350) to before it (0.23.1 line 361), with a comment referencing issue #442. This change itself doesn't cause the bug, but it was part of the same release that introduced `session_state_changed`, and the comment confirms the developers were aware of cancel/prompt race conditions in this area.

### Timeline correlation

The bug was first observed in late March / early April 2026. The `session_state_changed` handler was introduced in 0.23.0 (2026-03-25), one week before the bug was reported. This matches a regression introduced by the idle-state handling feature.

## Why other agents are unaffected

The mock agent (`mock-acp-agent`) handles each `session/prompt` request as an independent function call. There is no shared `cancelled` flag, no `promptRunning` gate, and no prompt queuing. Each prompt is processed in its own scope, so no flag-reset race can occur.

## Potential fixes (in `claude-agent-acp`)

1. **Don't reset `session.cancelled` when queuing.** Move `session.cancelled = false` to after the pending promise resolves (after line 254), so only the prompt that actually takes over the loop clears the flag.

2. **Use a per-prompt cancel token** instead of a shared session-level boolean. Each prompt loop checks its own token, unaffected by concurrent prompt entries.

3. **Guard the reset with `!session.promptRunning`.** Only reset the flag when this prompt will be the active loop owner:
   ```js
   if (!session.promptRunning) {
       session.cancelled = false;
   }
   ```

Any of these would prevent the incoming prompt from clearing the flag before the running loop can observe it.
