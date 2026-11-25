# Independent acceptance: #790 at f8082cbf (editing follows the current controller)

All data is synthetic. No feature branch was edited. The criterion follows Cliff's rule: editing a queued prompt follows the current session controller. It replaces the earlier origin-only criterion.

`f8082cbf` adds one test-only commit (`joined_queue.rs` and a support file) to `main` 85da3524, so the runtime behavior is main's. Builds: `f8082cbf`, `main` 66264b5d as the control, and the earlier origin-only head `39fd5283` as a negative control.

## Verdict: pass

R14 checks. A terminal queues `relay 1 lamp`, `relay 1 second` and `relay 1 third` during a turn, then interrupts it.
- **R14a:** that terminal detaches with Ctrl+D, and a fresh `nori attach` opens the session.
- **R14b:** a second terminal joins as a watcher. Its Up+Enter is refused. It then takes control with Alt+T.
- In both, the new controller presses Up+Enter and edits the taken text to `relay 1 thirdfix`.
- Queue removal is told apart from history recall by the queue count. A take drops `3 queued` to `2 queued`; a recall puts the text in the composer and leaves `3 queued`.

| Check | `f8082cbf` | `main` | `39fd5283` (origin-only) |
|---|---|---|---|
| Watcher Up+Enter leaves the queue at 3 in both terminals and sends nothing | pass | pass | pass |
| Up+Enter takes from the queue (3 → 2), not a history recall | pass | pass | **fail**: Up recalls history, queue stays at 3 |
| `relay 1 thirdfix` delivered once | pass | pass | pass (typed by hand) |
| Original `relay 1 third` delivered 0 times | pass | pass | **fail**: delivered once on resume |
| `relay 1 lamp` and `relay 1 second` delivered once each, in order | pass | pass | pass |
| R05a/R05b: own take-back once, also after reconnect | pass | – | – |

- Scenarios: `f8082cbf` 22/22 (`r14/`), `main` 13/13 (`r14-main-66264b5d/`), `39fd5283` 7/13 (`r14-39fd5283/`).
- Wire probes, `f8082cbf` 22/22 and `main` 22/22:
  - A client without `composer` (broker-style) queues prompts that stay non-editable, and `take` returns `null`.
  - A composer terminal's prompt is taken once. The same `requestKey` returns the same answer; another key gets `null`.
  - A terminal that took control takes another terminal's prompt.
  - Watchers get `-32016`.
