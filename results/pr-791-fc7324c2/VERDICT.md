# Independent acceptance: #791 at fc7324c2 (provisional: the owner is still changing replay and file search)

All data is synthetic. No feature branch was edited. `fc7324c2` is one commit on `main` 66264b5d. Release builds (LTO off, 16 codegen units), same Linux machine, same session: `fc7324c2`, landed `main` 85da3524, and the pre-cutover baseline `dfd9b4d4`. Binary hashes are in `joined_open.json`.

## Verdict: M2, L1, L2 and L3 pass on Linux and native macOS. M1 is much better than `main` but still slower than `dfd9b4d4` to the newest reply: 2.4–2.8× on Linux, 1.3–2.7× on macOS. Provisional until the final head.

### M1: long-history open (`perf/joined_open.py`, 100×30, median of 3)

ms from launch. "First content" = first section heading on screen. "Newest reply" = the last reply's end marker on screen.

| Sections | First content: `dfd9b4d4` / `main` / `fc7324c2` | Newest reply: `dfd9b4d4` / `main` / `fc7324c2` | Typed key shows | Bytes written |
|---|---|---|---|---|
| 64 | – ¹ / 87 / 93 | **66** / 451 / **159** | 54 / 60 / 68 | 185k / 211k / 16k |
| 150 | 55 / 86 / 91 | **95** / 1,632 / **267** | 55 / 61 / 70 | 84k / 481k / 34k |
| 170 | 56 / 93 / 95 | **107** / 1,993 / **302** | 61 / 60 / 65 | 93k / 545k / 40k |

¹ At 64 sections `dfd9b4d4` opens on the newest reply; no earlier heading is on screen.

- Newest reply: 2.8× faster than `main` at 64 sections, 6× at 150, 6.6× at 170. It is still 2.4×, 2.8× and 2.8× slower than `dfd9b4d4`, and the gap grows with history (+93, +172, +195 ms).
- First content is unchanged from `main` (+35–38 ms over `dfd9b4d4`). The first key shows about 8 ms later than on `main`.
- The PR's own numbers (108 / 168 / 181) are lower than these but follow the same shape. The machines differ.

### M2: rows written twice after Ctrl+L

| | `main` | `fc7324c2` |
|---|---|---|
| Duplicated replies across 9 runs (64/150/170 × 3) | 2, 2, 0, 4, 4, 4, 8, 9, 6 | 0 in all 9 |
| Every reply once and in order | 1 of 9 runs | 9 of 9 |
| Plan updates (every 5th section) shown exactly once | all | all |

**New finding (medium, already on `main`, not caused by #791):** each plan update is shown **above** the reply it followed. The host replays `reply 5 → plan 5 → tool 5` (`plan-order/host-replay-order-8-sections.txt`, from `harness/perf/replay_order.py BIN harness/perf`). The terminal shows `prompt 5 → plan 5 → reply 5 → tool 5`, on `main` and on `fc7324c2` (`plan-order/*-64.txt`, lines 98–105). Untested: whether a live turn (not a replay) and plain `nori` reorder the same way.

### L1–L3 (`scenarios/`, controls in `main-85da3524-control/`)

| Check | `fc7324c2` | `main` |
|---|---|---|
| L1: a refused `/agent` keeps `/agent` in the prompt (S35b); refused `/new` (watcher, S35c) and during a reconnect (S35h) keep the draft | pass | **fail** |
| L2 (S35r, alone): `/review the parser`, `/review` and `/lanternize now` each reach the agent once | 3/3 | **0/3** (stay in the prompt) |
| L3 (S41c): with no session, `@lan` lists the host folder's files, not the client's | pass | **fail** |
| L3 (new S41h): `@` typed as soon as the composer appears, no settle delay, no session: host file shown in 154 ms, the client-only name `decoy_laptop` never offered | pass | **fail** (client file offered) |
| S41h right after `/new <beta>`: `@harb` shows beta's file, alpha's name gives nothing | pass | pass |
| S41h after Alt+S, both directions: `@` follows the session on screen, the other folder's file is never offered | pass | pass |

- S41h: `fc7324c2` 7/7, `main` 5/7. PNGs: `scenarios/S41h/0-at-no-session-immediate.png`, `0b-at-client-only-name.png` and the same under the control.
- Stale harness checks, not product findings: S35b's `/compact` checks expect a refusal, but `/compact` runs on the host since #783 (it fails on `main` too). Its `/review the parser` check then lands during the compaction turn; S35r runs the same commands alone and passes. S35c and S41a look for `CONTROLLING`/`WATCHING` in capitals. The Alt+S loop in S41a never moves off the current row (the picker opens on it, and Down at the last row stays put), so it visits one session three times. S41h replaces it.
- Regression guard (`regression/`): R01, R02, R03, R05a, R05b, R14a and R14b pass. R12 (a live line without a newline) fails in attach and plain `nori`, as expected: the PR leaves it to a product decision.
- Linux CI on the head passes.

## Native macOS

macOS 15.7.4 arm64, [run 45726848563](https://github.com/tilework-tech/nori-cli/actions/runs/45726848563), branch `verify/pr-791-fc7324c2-native-acceptance`, checked out at `fc7324c2` (the `nori-rs` tree matches the head, `340127ce`), with `dfd9b4d4` and `main` 85da3524 built on the same runner in their own target dirs. Harness `7503ab40`. Evidence in `macos/`.

| Sections | First content: `dfd9b4d4` / `main` / `fc7324c2` | Newest reply: `dfd9b4d4` / `main` / `fc7324c2` | Duplicated replies after Ctrl+L, `main` / `fc7324c2` |
|---|---|---|---|
| 64 | – / 60 / 69 | **71** / 251 / **95** | 1, 0, 1 / 0, 0, 0 |
| 150 | 61 ¹ / 66 / 66 | **69** / 1,057 / **181** | 7, 0, 6 / 0, 0, 0 |
| 170 | 62 ¹ / 68 / 67 | **70** / 1,336 / **189** | 3, 4, 4 / 0, 0, 0 |

¹ Median of the samples where an earlier heading was on screen at all; `dfd9b4d4` opens on the newest reply, so `joined_open.py` records a timeout for that marker only. Its newest-reply times are complete.

- Scenarios S41c, S41h, S35r, R01 and R14a: 27/27. `main` fails S41c, S41h's two no-session checks and all three S35r checks (21/27).
- `nori-tui` unit tests: 1,850 passed, 0 failed.
- Plan updates show once in every run; the plan-above-reply order is the same as on Linux.
