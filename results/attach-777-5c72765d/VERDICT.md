# Independent acceptance: #777 at 5c72765d (joined view fix)

All data is synthetic. No feature branch was edited. This re-checks the earlier verdict at `20051fcf` (`../attach-777-20051fcf/VERDICT.md`).

| | SHA |
|---|---|
| #777 head | `5c72765d` (base #776 `b2562f5e`) |
| Baseline | `main` 0466dddf |
| Entry | `harness/verifier/verifier-joined-app.patch` (`NORI_VERIFY_JOINED_APP=1`), the same as before. From #784 on, real `nori attach` runs this App. |

## Verdict: F1 fixed. F2 is fixed in #784, not here.

### F1 (was blocker): replies without a final newline. Fixed.

R01 passes 7 of 7 at this head (`restack/R01`). It also passes on real `nori attach` at #784 `c8f3ee3b` and at the restacked #785, on Linux and native macOS.
- The restored reply `dawn relay finished.` shows once.
- Two live replies without a final newline (`noon`, `dusk`) and a plain reply each show once.
- Order holds: each prompt, its rows, then its last line, with no last line glued to the next row.
- A terminal that joins afterwards replays all of it once, in the same order.
- Each live prompt reaches the agent once.

### F2 (was medium): first content about 1 s late. Still present here; #784 fixes it.

`joined_open.py`, Linux, 3 samples each, median ms (`../attach-784-c8f3ee3b/perf/joined_open.json`):

| sections | main: first / newest | #777 5c72765d: first / newest |
|---|---|---|
| 64 | 54.5 / 64.0 | 1080.8 / 1171.1 |
| 150 | 55.0 / 88.8 | 1081.6 / 1290.0 |
| 170 | 54.6 / 109.3 | 1088.8 / 1323.7 |

- Scrollback audit: every mark once and in order, 9 of 9 samples.
- The fix is #784's `c8f3ee3b` ("open the picker at once"), where first content arrives in 83–89 ms. #777 is not usable alone, so this is acceptable only if #777 and #784 land together.

### Typeahead at launch: also fixed by #784

R03: text typed 50 ms after `nori attach <id>` and sent 400 ms later is lost here (the session opens after about 1 s). #784 `c8f3ee3b` delivers it once, 3 of 3.

## Passes

- J01–J06 (controller/watcher, Alt+T takeover, relay reconnect, switching, permissions, Ctrl+D, `/quit`, hangup): 62 of 62 (`joined/`).
- Linux PR CI for `5c72765d` is green.
