# Independent acceptance: #794 at b0f10ec7 (composer hint row)

All data is synthetic (an `orchard` project with `pear_ledger.rs` / `plum_rows.rs`, throwaway homes under `/tmp/v794`, the repo's `mock_acp_agent`). No feature branch was edited. Scripts: `harness/pr794/` (`plain.py`, `attach.py`, `reconnect.py`, `enhanced.py`, `busybang.py`, `kittywrap.py`).

## Builds under test

| Label | Commit | What it is |
|---|---|---|
| `794` | `b0f10ec7` | PR head alone (base `main` 66264b5d) |
| `combo` | local merge `23e2be8c` = `b0f10ec7` + #789 `d98c3e1b` | merged cleanly, local only, never pushed |

Debug builds, Linux x86_64, tmux 3.3a. macOS was not run.

## Verdict: pass on Linux for both builds, with no discrepancies between the hints and key handling

| Area | `794` | `combo` |
|---|---|---|
| Plain `nori` P1–P7 (49 checks): bold keys / dim labels (SGR 1 / 2), empty → typed → erased, `/` and `!` → `Enter run`, Ctrl+J inserts a newline, `?` lists `! for shell commands` and `$ for skills`, `@` popup, steer/Tab queue, nonsteerable `Enter queue`, paused queue, 40/60/80 cols, four layout overrides | 49/49 | P2, P3 14/14 |
| P9: Tab with an agent that cannot steer queues and delivers once | pass | – |
| P8: `!` during own turn: `! shell` still shown, `Enter run`, runs, nothing sent to agent | pass | – |
| Attach A1–A6 (33 checks): controller, watcher, takeover, nonsteerable + paused, remote, narrow, enhanced keys | 33/33 | 33/33 |
| A7 reconnecting via a dropped relay: only `Ctrl+J newline`, Enter sends nothing, `Enter send` returns after reconnect | 3/3 | 3/3 |
| E1: kitty keyboard protocol (PTY wrapper answers `CSI ? u`; nori pushes `CSI > 7 u`): `Shift+Enter newline`, Shift+Enter (`CSI 13;2 u`) inserts a newline, Ctrl+J still works, Enter sends all three lines once | 6/6 | – |

### Remote `! shell` (A4), per build

| Build | Empty hint (remote controller after Alt+T) | `!touch …` typed | Enter |
|---|---|---|---|
| `794` alone | `/ commands · ! shell · @ files · ? help` | `Enter run` | ran on the host (canary created in the session folder). This is correct for its base: on `main`, `!` runs remotely too |
| `combo` | `/ commands · @ files · ? help` | no hint | gray "Remote shell commands are not yet supported.", draft kept, nothing ran |

Local `nori attach` keeps `! shell` in both builds. #794 reads `features.shell`, so it follows #789 with no extra change.

### Agreement with key handling (observed outcomes)

- `Enter steer · Tab queue`: Tab queued without sending. Enter was injected into the running turn ("Survey correction received: …"). The queued prompt was delivered once afterwards.
- `Enter queue` (agent without steering): Enter showed "Steering unavailable. Added to the next-turn queue." and was delivered once on the next turn. No Tab hint is shown, though Tab also queues (P9: queued, then delivered once).
- A2, after Alt+T takeover during another terminal's turn: the new controller sees `Enter steer · Tab queue`, and Enter does steer that turn.
- `Enter resume queue` shows only for a controller with an empty prompt. Empty Enter resumed it once. A watcher sees no resume hint, and its Enter resumed nothing.
- Watcher: no `! shell`, no Enter key, and only `Ctrl+J newline` once text is typed. Enter refused with the draft kept, and `alt+t take control` stayed in the status.

## Observations for the owners (not failures)

1. Narrow attach (A5): status wins over hints, as specified. At 100 cols only `/ commands` (or `Enter send`) fits. At 80 cols and below, no hints show. At 40 and 60 cols the status itself is cut mid-word (`… · r`, `Approvals: Ag`). I did not compare this cut with `main`.
2. The paused-queue header still says `enter resumes` beside the new `Enter resume queue` hint. This is already flagged in the PR.
3. Casing differs: the hint row says `Ctrl+J` / `Shift+Enter`, while the `?` overlay says `ctrl + j for newline`.
4. A watcher pressing Enter on a paused queue gets no visible feedback. This behavior predates #794.

## Not covered

- Native macOS. The hint strings do not depend on the platform (`composer_hints.rs` picks Shift+Enter or Ctrl+J only from keyboard-enhancement detection). The `⌥` prefix in `key_hint.rs` is used only by the `?` overlay.
- A real kitty, Ghostty or WezTerm window. Enhanced mode was emulated with `kittywrap.py`.
- `cargo test` suites were not rerun. Linux CI and cargo-deny are green at `b0f10ec7`.
