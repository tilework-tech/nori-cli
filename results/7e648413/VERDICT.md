# #757 `/screen` and Ctrl+L at `7e648413` (on #753 `bf0788f8`): changes requested

Independent verification, 2026-11-24. Real terminals (tmux) on Linux, the real `nori` and `nori-tui` binaries, the synthetic `lantern_adapter.py` agent server, and synthetic homes and folders.

## Blocking

1. **The full-screen view can miss history that arrives with no later activity (intermittent).** After `/resume` while full-screen, the replayed question and answer stayed off screen for 13 s in 8 of 18 runs (S42l). The inline control (S42n) showed them 4 of 4. The cells are in the transcript, because a resize shows them and switching inline prints them once. Cause from source: `insert_active_history_cell` (and the `InsertHistoryCell` path) only schedule a frame through `tui.insert_history_lines`, which is skipped when `print_or_hold` holds the lines. Evidence: `results/7e648413-s42l-stale/`.

## Minor

2. The `?` shortcuts list always says "ctrl + l to toggle full screen", even with `toggle_screen = "alt+j"` (S43d). The binding itself follows the config.

## Passing at `7e648413`

| Area | Result |
|---|---|
| Inline → full-screen and back, both directions during a streaming turn (S42a, nori-tui S42g) | 19/19 each. 40 rows reach the scrollback once and in order. The earlier exchange appears once. A whole turn run full-screen lands once. |
| Two sessions, Alt+, while full-screen, background turn streaming (S42b) | 15/15. Each session's scrollback holds only that session after you return inline. 70 rows once and in order. |
| Ctrl+T and resizes over full-screen, then exit with the overlay open (S42c) | 15/15. Held history is printed on exit. |
| /quit and Ctrl+C from full-screen during a turn (S42d, S42e) | 7/7 each. Normal screen, cursor visible, cooked tty, balanced alternate-screen and paste modes. |
| Resizes while full-screen and streaming (S42i), Ctrl+Z and fg (S42j), permission prompt while full-screen (S42k) | 10/10, 10/10, 13/13 |
| Draft typed **before** the switch, Ctrl+L both ways (S43a, nori-tui S43f) | 15/15 each. The exact text stays visible and unsent in both views, then Enter sends it once. |
| Draft typed during a turn, Ctrl+L both ways during the turn (S43b) | 6/6 |
| Two-line pasted draft; Ctrl+L with the slash menu open; permission prompt open (S43c) | 8/8. Switching keeps the typed text and menu. The permission prompt holds the key, as it holds Ctrl+T, and nothing is answered. |
| Overlay preservation vs a switch action (S42f) | Ctrl+T keeps the draft. At `768b2a64` no key switched with a draft in place (F11, Alt+S, Ctrl+S tried). At `7e648413` Ctrl+L does. |
| nori attach (S42h, S43e) | `/screen` greyed with "nori attach is always full-screen". Ctrl+L leaves the screen and draft alone and isn't advertised. |

Inherited, not caused by #757: the "Permission: Allow once" row is printed above the "› perm paint" row, inline and full-screen alike, and at base `bf0788f8` (S42m).

## CI and native macOS

- Linux CI 45455617170: passed and ran all 6 `screen_toggle` tests.
- Owner's native run 45455645488: passed; repeats 0 failed of 95, attach folder tests 0 of 15.
- My exact-head native run 45456861528 (`verify/pr-757-7e648413-screen-macos-load`): standard steps passed. `screen_toggle` 6 tests × 15 runs (5 plain rounds + 10 under parallel load) all passed. With `host_attach` ungated, the inherited failures recur at baseline rates: watcher permission 0/10, busy Enter 0/10, two-terminal takeover 7/10 (#751 0/0/7, #752 0/0/8, #753 0/0/5).
- Linux, repo terminal suites (`screen_toggle`, `host_startup`, `host_new_session`, `agent_switching`, `host_file_mentions`, `host_attach`, `transcript_keys`, `streaming`), 5 rounds: 325/325 (`linux-pty-5-rounds.txt`).
