# Independent acceptance: #784 at c8f3ee3b (nori attach on the shared terminal)

All data is synthetic. No feature branch was edited. Release builds (LTO off, 16 codegen units) of `c8f3ee3b` and `main` 0466dddf, in separate target dirs.

## Verdict: two-terminal behavior passes on Linux and native macOS. Two medium findings (M1, M2) and three low ones.

### Two terminals: pass

| | Linux | macOS 15.7 arm64 ([run](https://github.com/tilework-tech/nori-cli/actions/runs/45694129387), branch `verify/pr-784-c8f3ee3b-native-host`) |
|---|---|---|
| J01–J06: restored and live once, watcher Enter keeps the draft, Alt+T takeover both ways, relay drop and reconnect replays only the missed turn, Alt+S switching, permission answered once by the controller, Ctrl+D, `/quit` and hangup keep the host session | 62/62 (`joined/`) | included in 78/78 (`macos/`) |
| R01: replies without a final newline, restored and live, once and in order, also for a late joiner | 7/7 (`restack/`) | pass |
| R02: empty host opens the picker on the alternate screen; `/new` starts a session on the host; Ctrl+D detaches; a second `nori attach` lists 2 sessions and Enter opens one | 7/7 | pass |
| R03: typeahead 50 ms after launch, Enter at 450 ms, delivered once | 3/3 (main 3/3) | pass |
| Attach PTY suites, 3 rounds, Linux gate removed | not rerun (PR CI green) | 98/102. `watcher_cannot_answer_a_permission_request_but_controller_can` fails 3/3 and `same_port_host_restart…` fails 1/3, as on the owner's `c9b383ca` run (4/99) and on `main` (5cb13fcd verdict). J05 passes the same behavior on real terminals. |
| Full PTY suite | | 1,956 passed, 0 failed |

Workflow note: the `! grep -q FAILED` guard in the shared macOS template does not fail the step under `bash -e` (a negated command is exempt). The owner's `c9b383ca` macOS run is marked successful while 4 of 99 attach rounds failed.

### M1 (medium): the newest reply takes 0.5–2.3 s to appear in a long session

`perf/joined_open.json`, Linux, median of 3, ms. "First" = first history row on screen, "newest" = the last reply on screen.

| sections | main first / newest | #784 first / newest |
|---|---|---|
| 64 | 54.5 / 64.0 | 82.7 / 473.2 |
| 150 | 55.0 / 88.8 | 84.3 / 1,674.3 |
| 170 | 54.6 / 109.3 | 89.2 / 2,261.2 |

- macOS: 64.1 / 308.4 (64 sections), 61.8 / 1,369.1 (170).
- The first rows arrive fast (the 1 s wait from #777 F2 is gone). The full-screen view then scrolls through the whole history before it settles on the newest turn, which is what the user came back to. The cost grows with history length; main shows the newest turn within about 110 ms.
- First key is unchanged: 59–62 ms vs 54–59 ms on main.

### M2 (medium): a reply cut by a plan update shows twice after switching inline

`perf/dup-150-run1.txt`, `dup-150-run2.txt`, `perf/dup_probe.py`.
- In a 150-section history, Ctrl+L (full screen to inline) writes some replies twice: first only their opening rows (`## KilnMark0055`), then the plan cell, then the whole reply again.
- It hits only sections whose reply is followed by a plan update (every 5th), in 1 to 13 of 150 sections per run. On macOS it hit 2–7 sections per run.
- The #777 inline joined App had no duplicates (9/9 samples), so the alternate-screen start is involved. Not traced further.

### Low

- **L1: a refused command loses the typed draft.** A refused `/new` (watcher or reconnecting), `/compact` or `/agent` clears the composer, although the footer says "draft kept". The old attach kept the text (S35b, S35c, S35h; 2/2 isolated reruns, main 0 failures).
- **L2: agent command `/review` is rejected.** An agent-advertised `/review` prints "Unrecognized command '/review'" and is not sent; `/lanternize` from the same agent goes through. Main sends `/review` once (S35b).
- **L3: `@` uses the wrong folder in two cases.** With no session selected, `@lan` lists the client's own files (`docs/laptop_notes.md` from the client folder), not the host folder (S41c). After Alt+S to another session, `@` keeps offering the first session's files (S41a `s.at_follows_switched_session`). Both pass on main.
- **Copy:** the empty picker says "No options available / The caller has not supplied any rows."

### Behavior changes (no action unless the owner disagrees)

- A live line with no newline yet (`tick 1 tick 2 …`) stays hidden until its newline or the turn end. That matches plain `nori` on main (R12, `r12-*`); the old attach showed it.
- `/screen` and Ctrl+L now work in attach (S42h, S43e expected the old refusal).
- `/resume` reads "not available in nori attach yet".

## Matrix vs main (`matrix/`)

Full S/K matrix: main 767/815, #784 688/758. Every check that changed was rerun alone (`matrix/isolated-*`). After harness fixes (the new footer role, the startup picker), the only #784-caused items are L1, L2 and L3 above. Scenarios that stopped early did so for harness reasons: old header text, no `nori-tui` binary in this build, the update popup, or `/proc` permissions.
