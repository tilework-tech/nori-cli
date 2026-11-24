# #751 at `5cb13fcd`: independent verdict

`5cb13fcd` sits on `83d15494`, which is `c31726f7` restacked onto #750
`54b4ca5c` with the same patch.

## Escape with the `/` menu open during a turn

The new scenarios S35i (plain `nori`) and S35j (`nori attach`) start a turn,
open the menu by typing `/`, and press Escape once. They then press Escape
again, up to three times, until a `session/cancel` appears. An in-run control
first measures how many presses interrupt a turn when the draft is plain text
(`lamp draft`) and no menu is open.

| Check | `c31726f7` (before) | `5cb13fcd` |
|---|---|---|
| First Escape closes the menu | fail: menu stays open | pass |
| First Escape keeps the `/` draft | pass | pass |
| First Escape does not cancel, and the turn keeps streaming | pass | pass |
| The following Escapes interrupt like a plain draft (control: 1 press) | fail: no cancel after 3 more presses | pass: 1 press |
| One cancel per turn | fail | pass: 2 cancels for 2 turns |
| Draft after the interrupt matches the control (kept) | pass | pass |

- **Older checks:** S35d/e (first Escape closes, then cancels once) and
  S28a/b (normal Escape cancel, next turn completes) pass at `5cb13fcd`. S35d/e
  failed at `c31726f7`.
- **All of S35a–j at `5cb13fcd`:** 78/79. The remaining failure is
  `S35a/plain.bare_review_not_captured_by_loose_match`, a known plain-`nori`
  gap that also fails at #750.
- **Pre-fix run:** 23/31 (`results/c31726f7-s35esc/`).

Frames: `S35i/` and `S35j/` contain `menu-open`, `after-first-escape`,
`after-interrupt` and `control-cancelled`. Cursor placement in the PNGs is a
renderer artifact: the tmux captures carry no cursor position.

## Native and Linux

**Native macOS, exact `5cb13fcd` sources** (run 45406511352, verifier branch
`verify/pr-751-5cb13fcd-native-drain`; the job confirms `nori-rs matches
5cb13fcd` before it builds):

| Suite | Passed | Failed |
|---|---|---|
| tui-pty-e2e | 87 | 0 |
| nori-tui | 1840 (16 more than #750) | 0 |
| nori-harness | 501 | 0 |
| nori-acp-host | 144 | 0 |
| nori-cli | 265 | 0 |
| nori-config, nori-host-runtime, nori-auth-providers | 220, 9, 66 | 0 |
| 4 host suites plus `slash_menu_escape`, 10 rounds | 200 | 0 |

- **Coverage gap:** `host_attach` is `#![cfg(target_os = "linux")]`, so the
  attach Escape PTY test never runs on macOS. There, attach Escape is covered
  only by the `nori-tui` unit test
  `the_first_escape_closes_the_menu_and_the_next_cancels_the_turn`.
- **An earlier attempt is not counted:** run 45404121099 restored the sources
  from the wrong directory and tested #750's patched tree.

**Linux:**
- Author's CI 45399358573 passed.
- Locally, `host_attach`, `slash_menu_escape`, `host_startup` and
  `host_new_session` passed 5 rounds × 27, all passing.
- S35i/j and S28a/b passed through tmux.

All homes, adapters and prompts are synthetic.
