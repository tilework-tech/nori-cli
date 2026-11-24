# #757 `/screen` recheck at `17a955c0` (on `9106afd8`, `7e648413`): approve, one minor open

Independent verification, 2026-11-24. Tested head: `17a955c0942d16831aee0363e3af4e588bf22841` (the #757 head, checked before posting). Control: `9106afd8`. Original failure: `7e648413`. Real terminals (tmux) on Linux, the real `nori` and `nori-tui` debug builds, the synthetic `lantern_adapter.py` agent server, synthetic homes and folders.

## Resume while full-screen (the blocking finding)

S44a/S44b (`nori` / `nori-tui`): `/resume` from full-screen, pick the saved session, then **no key, resize or turn** for 13 s. Both restored rows (question and answer) must appear and stay. Then `/screen` back inline before any new turn: each row in the scrollback once, question before answer, and the previous session's rows gone. Then one turn: still once, in order. S44e/S44f: the same resume, then `/quit` straight from full-screen: the normal terminal scrollback holds each row once, in order. S44c/S44d are the inline controls. S42l is the original scenario.

| Scenario | `7e648413` | `9106afd8` | `17a955c0` |
|---|---|---|---|
| S44a `nori`, full-screen, no input; back inline once and in order | 10/12 (2 never showed the rows) | 12/12 | 12/12 |
| S44b `nori-tui`, same | 9/12 (3 never showed the rows) | 12/12 | 11/11 |
| S42l original (`nori`) | 8/12 | 10/10 | 5/5 |
| S44e `nori`, `/quit` from full-screen, scrollback once | — | **0/4** (history lost) | 11/11 |
| S44f `nori-tui`, same | — | **0/4** (history lost) | 11/11 |
| S44c / S44d inline controls | — | 5/5, 5/5 | 5/5, 5/5 |

At `9106afd8` the quit case left only the earlier session ("lagoon line") in the scrollback; `17a955c0` fixes it. Restored rows appeared without input at 0.0–0.2 s in every fixed run.

Evidence: `results/7e648413-resume-repro/`, `results/9106afd8-resume/{new1,new2,new3,cq}/`, `results/17a955c0-resume/{f1,f2}/`, counts in `results/17a955c0-resume/aggregate.txt`. Scenarios S44a–f are in `harness/run.py`.

## Minor, still open: the help hint ignores the configured key

With `[tui.hotkeys] toggle_screen = "alt+j"`, Alt+J switches and Ctrl+L doesn't, but `?` still lists "ctrl + l to toggle full screen" (S43d, 5/5 at `9106afd8` and `17a955c0`). `bottom_pane/footer.rs` `ShortcutId::ToggleScreen` hard-codes `key_hint::ctrl('l')`. Severity: minor (wrong help text for rebinding users only; the binding works). Not a blocker.

## CI and native macOS at `17a955c0`

PR-triggered CI did not start for the API-published `9106afd8`/`17a955c0`. My exact-head run `45471793243` (`verify/pr-757-17a955c0-screen-recheck`, the PR's own workflow pinned to the head, source revision checked):
- Linux checks: fmt, build, all package tests (3429 passed, 0 failed, 15 ignored), clippy. `screen_toggle` 15 rounds (5 under parallel load): 120/120, both resume tests 15/15.
- macOS checks (Darwin arm64): standard steps passed (3222 passed, 0 failed, 8 ignored; many PTY suites are Linux-gated and run 0 tests on macOS). `screen_toggle` ungated 15 rounds: 120/120, resume and quit tests 15/15 each.
- `host_attach` ungated on macOS, 10 rounds: the inherited failures at baseline rates: busy Enter 0/10, watcher permission 0/10, two-terminal takeover 8/10. The other 16 tests 160/160. Same at `9106afd8` (run `45469971488`), and in line with #751–#753 and `7e648413`.
- Author runs agree: Linux `45468597276`, native macOS `45468586459`.

Not caused by #757: my `9106afd8` Linux run hit `startup::test_startup_shows_nori_banner` once (the card already showed the session's approvals and model). #757 doesn't touch startup; it passed at `17a955c0` in CI and 10/10 locally. Treat it as an intermittent startup snapshot race.

Still inherited, unchanged: "Permission: Allow once" prints above its prompt row (S42m, at base `bf0788f8` too).
