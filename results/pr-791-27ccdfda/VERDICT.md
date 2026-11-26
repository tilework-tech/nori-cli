# Independent acceptance: #791 at 27ccdfda

All data is synthetic. No feature branch was edited. `27ccdfda` sits on #795 (`c6cef012`) in the stack main → #789 → #790 → #792 → #793 → #794 → #795 → #791 and adds two commits: `b035761f` (the earlier `fc7324c2` change, rebased) and `27ccdfda` (reopened `@` and per-tick streaming). Release builds (LTO off, 16 codegen units) on one Linux machine: `27ccdfda`; its base `c6cef012` (whole stack below #791); landed `main` 85da3524; pre-cutover `dfd9b4d4`. Hashes are in `joined_open.json`. #792 and #793 keep their verdicts at `3350ba87` and `f59783d9`; R06, R09 and R09b also pass here at the stack head.

## Verdict (Linux and native macOS): pass for M2, the reopened `@`, single delivery of streamed replies across Ctrl+L, and L1–L3. M1 is better than at `fc7324c2`, but the newest reply is still slower than `dfd9b4d4`: 2.1–2.4× on Linux, 1.2–2.0× on macOS. Plan updates are out of order on every build, including `main` and plain `nori`.

### M1: long-history open (`perf/joined_open.py`, 100×30, median of 3, ms)

| Sections | First content: `dfd9b4d4` / `main` / base / **`27ccdfda`** | Newest reply: `dfd9b4d4` / `main` / base / **`27ccdfda`** | Typed key shows | Bytes |
|---|---|---|---|---|
| 64 | 54 / 82 / 80 / **87** | 55 / 462 / 463 / **132** | 54 / 60 / 59 / 70 | 185k / 217k / 211k / 13k |
| 150 | 56 / 93 / 86 / **96** | 96 / 1,707 / 1,758 / **211** | 56 / 61 / 60 / 69 | 83k / 494k / 494k / 24k |
| 170 | 57 / 92 / 86 / **94** | 105 / 2,130 / 2,139 / **219** | 61 / 61 / 61 / 67 | 94k / 555k / 555k / 26k |

- Newest reply: 3.5× faster than `main` at 64 sections, 8× at 150, 9.7× at 170. Down from 159 / 267 / 302 at `fc7324c2` (an earlier run on the same machine).
- Still 2.4×, 2.2× and 2.1× slower than `dfd9b4d4` (+77, +115, +114 ms). The gap no longer grows between 150 and 170 sections.
- First content is 30–40 ms behind `dfd9b4d4`, as on `main`. A typed key shows about 8 ms later than on `main`.
- The base `c6cef012` matches `main`, so the stack below #791 does not move these numbers.

### M2 and single delivery after Ctrl+L

| Check | `main` | base `c6cef012` | `27ccdfda` |
|---|---|---|---|
| Long-history replay: replies written twice after Ctrl+L, 9 runs | 1, 2, 0, 5, 5, 5, 9, 3, 9 | 2, 2, 0, 9, 5, 8, 4, 4, 4 | **0 in all 9** |
| S50 live: a 40-row reply streamed in mid-line chunks with a plan every 5 rows; Ctrl+L in the controller mid-stream | 75 rows (35 twice) | 75 rows (35 twice) | **40, each once, in order** |
| S50 watcher, Ctrl+L after the turn | 75 rows (35 twice) | 75 rows (35 twice) | **40, once** |
| S50 fresh attach (replay), and plain `nori` live | 40, once | 40, once | 40, once |
| Each plan update shown once; the closing line once | yes | yes | yes |

### Plan order (medium; same on `main`, the base and plain `nori`)

- **Live:** a plan update appears about four rows before the point where it arrived. The stream sends `row 30 → plan 6`; every terminal shows `row 26 → plan 6 → rows 27–30` (`S50/controller-done.txt`, `watcher-inline.txt`, `plain-done.txt`). The finished lines held for the commit tick are drawn after a plan that arrived later.
- **Replay:** a plan is drawn above the reply it followed. In the 40-row reply all eight plans sit above it. In the long-history fixture, `plan 5` sits above reply 5 (`joined_open_history/`). The host replays `reply → plan → tool` (`harness/perf/replay_order.py`).
- #791 does not cause this. It is the same on `main` and the base, and plain `nori` shows the same live offset.

### Reopened `@` (S41i)

The same query typed again: after Ctrl+U, after Backspace back to `@`, and after Esc. Each case runs in a remote attach with no session, a remote attach in a session, and plain `nori`.

| | `main` | base | `27ccdfda` |
|---|---|---|---|
| First query answered | 2 of 3 places (no-session fails: L3) | same as `main` | 3 of 3, ~155 ms |
| Same query again after Ctrl+U or Esc | **stuck on `loading...`**, all 3 places | same | answered, ~155 ms |
| After Backspace to `@` and retyping | answered, except with no session (L3) | same as `main` | answered |
| A client-only file offered | never | never | never |

S41i: `27ccdfda` 12/12 answers, `main` and base 4/12.

### L1–L3 and regression guard (`regression/`)

77/79 on `27ccdfda`: S35h, S35r, S41c, S41h, R01, R02, R03, R05a, R05b, R06, R09, R09b, R14a and R14b pass. R12 fails in attach and in plain `nori`: a live line still waits for its newline, as the PR states.

## Native macOS

macOS 15.7.4 arm64, [run 45737449515](https://github.com/tilework-tech/nori-cli/actions/runs/45737449515), branch `verify/pr-791-27ccdfda-native-acceptance`, checked out at `27ccdfda` (the `nori-rs` tree matches the head). `dfd9b4d4` and `main` were built on the same runner. Harness `caf5f4fa`. Evidence in `macos/`.

| Sections | First content: `dfd9b4d4` / `main` / `27ccdfda` | Newest reply: `dfd9b4d4` / `main` / **`27ccdfda`** | Replies written twice after Ctrl+L, `main` / `27ccdfda` |
|---|---|---|---|
| 64 | – ¹ / 65 / 68 | 65 / 260 / **76** | 0, 0, 0 / 0, 0, 0 |
| 150 | 60 / 61 / 64 | 66 / 1,104 / **129** | 2, 3, 4 / 0, 0, 0 |
| 170 | 68 / 65 / 62 | 77 / 1,305 / **154** | 4, 8, 3 / 0, 0, 0 |

¹ `dfd9b4d4` opens on the newest reply, so no earlier heading appears on screen.

- Scenarios S50, S41i, S41c, S41h, S35r, R01 and R14a: 54/58 on `27ccdfda`. All four failures are S50's plan-order check, which `main` fails the same way. On macOS some live plans land right after their row and others before it. `main`: 38/58.
- S50 live on `main`: the controller shows 80 rows for 40 (all 40 twice) and the watcher 79. On `27ccdfda` both show 40, each once.
- S41i on `27ccdfda`: 12/12 answered. On `main`, the repeated query stays on `loading...`, as on Linux.
- `nori-tui` unit tests: 1,902 passed, 0 failed.
