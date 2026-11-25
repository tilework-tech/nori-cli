# Independent acceptance: #777 at 20051fcf (joined session view from replay and live events)

All data is synthetic. No feature branch was edited.

| | SHA |
|---|---|
| #777 head | `20051fcf` (base #776 `b2562f5e`) |
| Baseline | `main` 0466dddf |
| Verifier entry | `harness/verifier/verifier-joined-app.patch`: `nori attach` calls `run_joined_app` when `NORI_VERIFY_JOINED_APP` is set. Verifier tree only. |

## Verdict: one blocker (F1)

### F1 (blocker): agent text that doesn't end in a newline is never shown, or shows late and out of order

- **Symptom on a real host:** the user prompt prints but the reply doesn't, for both restored and live turns (`proof/joined-live.png`). Today's attach shows the reply (`proof/oldattach-restored.png`).
- **Symptom in a live turn:** streamed text with no newline yet (e.g. `tick 1 tick 2 …`) never appears; only the working indicator shows.
- **Ordering effect:** when a later turn streams, the previous tail is flushed into it. The two lines below were joined into one on screen:
  - `one relay finished.- hid row 01`
  - `dawn relay finished.` printed after the next prompt `› perm paint it`
- **Root cause, reproduced in a unit test:** `harness/verifier/host_feed_verifier.rs` feeds the 13 envelopes a real host replays (`fixtures/host-replay-first-light.jsonl`).
  - Over JSON, the prompt response `{"stopReason":"end_turn"}` deserializes as `AgentResponse::AuthenticateResponse`, not `PromptResponse`.
  - The widget's `PromptResponse` arm (matched on `owned_prompt_request_id`) never fires, so the turn never completes and the final partial line is never flushed.
  - Control: the same replay with the reply ending in `\n` prints it.
- **Why the PR tests pass:** they feed in-memory events whose replies end in `\n`.
- **Possibly also affected (from reading, unverified):** other untagged responses such as `CloseSessionResponse`, which gates `release_held_close_errors`.

### F2 (medium): first content waits about 1 s

- Opening a saved session through the joined App shows its first history after about 1,076 ms (median, n=3 at each of 64, 150 and 170 kiln sections). Today's attach takes 55 ms.
- Likely cause (code reading): the joined link's first list tick fires before `initialize` completes and is skipped, so the first list, and the subscription, wait for the 1 s heartbeat.
- A key typed 50 ms after launch is handled at 55–62 ms on both paths, so first-key latency did not regress.

### F3 (low, scope note for the launcher): `/new` is offered but fails

The joined controller's menu enables `/new`. Running it prints "This needs a session running in this terminal's own host." The attach matrix scenarios that start with `/new` therefore can't run on the joined App yet (`joined-preview-attach-matrix/`, informational).

## What passes

J scenarios: 2 terminals plus an observer, sessions seeded with `nori host new`, markers that end in a newline to avoid F1, and scrollback audited from tmux history. Results are in `joined/`.

| | Result |
|---|---|
| J01 restored history once, own turn sent once and shown once | pass |
| J02 watcher Enter keeps the draft and names Alt+T; Alt+T takeover; both drafts kept; promoted prompt delivered once; both terminals show it once | pass |
| J03 watcher behind a dropped relay: missed turn replayed once, history not repeated, draft kept, notice shown | pass |
| J04 switch with Alt+S: hidden turn not shown on the other session, shown once on return; drafts kept per session | pass |
| J05 permission: watcher sees it, can't answer; controller answers once (`allow`); both prompts dismissed | pass |
| J06a/b/d Ctrl+D, /quit, hangup mid-turn: host turn keeps streaming, nothing closed or cancelled, rejoin shows history and stream once, terminal restored | pass |

- **Long history:** kiln 64, 150 and 170 sections had every mark exactly once and in order, with no disconnect rows (`perf/joined_open_150_170.json`).
- **Library layer:** `joined_feed_verifier.rs` (5 tests: shuffled duplicates and gaps over 3,000 events, switching, gap reported once, unknown sessions, reconnect cursor) and the 5 HostLink tests pass.
- **Suites:**
  - `cargo test -p nori-tui`: 1,962 passed, 0 failed (with the verifier modules).
  - `nori-cli --test host_lifecycle --test managed_recovery`: 147 + 23 passed.
  - `tui-pty-e2e --test host_attach --test attached_external_cancel`: 5 + 20 passed.
- **Regression matrix** (real `nori attach` and plain `nori`): no check regressed against `main`.

Not covered: native macOS (deferred to the launcher head), fork continuation on a real host (no fork-capable synthetic adapter in this harness), and journal-expiry gaps (> 4,096 envelopes) in the joined App.
