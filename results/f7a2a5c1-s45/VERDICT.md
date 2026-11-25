# #772 terminal program status (OSC 7501) at `f7a2a5c1`: approve, no blocking findings

Independent verification, 2026-11-25. Linux: real tmux terminals, the real `nori` debug build at `f7a2a5c1f277cdefb2d6910c1041ad32c88a2dd6` (base `main` `80992049`), the synthetic `lantern_adapter.py` agent server, and synthetic homes and folders. This run did not use the PR's own mock agent. Each pane's raw output was recorded with `pipe-pane`, and every OSC 7501 report was read from those bytes. tmux 3.3 drops OSC 7501, so nothing here shows a terminal emulator rendering the status. The checks prove the bytes nori writes. Requirements come from the Nori design thread and the PR description. External request #754 was not opened.

## Requirements → result

| Requirement | Scenario | Result (3 rounds) |
|---|---|---|
| Off by default: no OSC 7501 from plain nori, attach controller, or watcher through an approval turn | S45a | 3/3 pass, zero `]7501` bytes |
| Opt-in plain nori follows the foreground state | S45b | `idle → working → done → working → blocked → working → done → clear`, deduplicated, 3/3 |
| Approval text is generic by default | S45b, S45g | `msg` decodes to `Approval needed`. "Paint the boathouse" never appears on the wire. |
| Opt-in approval title | S45c | `msg=UGFpbnQgdGhlIGJvYXRob3VzZQ==` = "Paint the boathouse", 3/3 |
| Tab title unchanged | S45b, S45g, regression lanes | No report carries `title` or `id`. The OSC 0 titles at base and at PR head (status on) are identical in all 23 regression terminals. |
| Typing and `/screen` both ways send no report | S45b | 3/3 |
| Error / cancel / recovery | S45d | failed prompt → `error`; Esc → `idle`; next turn → `working → done`; Ctrl+C exit → `clear`. 3/3 |
| Agent-started turn | S45k | `working`, then `idle` after cancel. 3/3 |
| Plain foreground switch updates at once; background stays silent | S45f | Alt+, / Alt+. report the shown session by the first poll after the key (≤ about 120 ms). A background session that keeps streaming, or that opens a permission request, writes nothing. 3/3 |
| Attach controller and watcher both track one session | S45g | Both report `blocked` with the generic text and end on `done`. 3/3 |
| Attach foreground switch through the session list | S45h | Switching to the working session reports `working` 0.12 s after Enter. The background stays silent. 3/3 |
| Exit/detach clears | S45b `/quit`, S45d Ctrl+C, S45g Ctrl+D watcher then controller, S45h Ctrl+D | Each ends with exactly one `state=clear`. The session and its turn survive detach. The terminal is restored (normal screen, cursor, cooked tty, balanced modes). |
| Stated limit: Ctrl+Z | S45i | Confirmed. No clear while suspended; the terminal keeps `working`. After `fg`, reporting resumes. |
| Stated limit: host stops under an attached terminal | S45j | Confirmed. The last `done` stays after `nori host stop`. Ctrl+D then exits and clears. |

## Regressions

- Existing matrix subset (S01 S02 S03a S03b S08 S20a S21b S25a S25b S28a S28b S37c S42a S42b S42d S42e S42j S42k S43a S43e): base `80992049` 168/171; PR off 168/171; PR with `program_status = true` 168/171. The same 3 checks fail on all three (S01 hint-row expectations, S08 watcher notice count), so they are pre-existing harness drift.
- The repo's own PTY tests on Linux (`program_status` 7, the `host_attach` controller/watcher test, and `screen_toggle` 8): 3/3 rounds pass (`linux-repo-pty-3-rounds.txt`).

## Non-blocking findings

1. **Pre-existing, not caused by #772: plain nori cannot exit after the agent server dies mid-prompt.** `/quit` stops at "Exiting…" for over 45 s, and Ctrl+C doesn't help. This reproduces at base `80992049` (`results/80992049-base-control/S45e0`) and at the PR head with program status off (S45e0). With program status on, the terminal keeps `error` because the clear path never runs. Worth its own bug; it doesn't block #772.
2. **Design behavior: attach coalesces bursts.** `nori attach` reports after a burst of host events lands. A fast turn can go `idle → done`, or `working → blocked`, without the state in between. Plain nori writes after every event and showed every state. Final states were always correct.
3. No terminal detection and no `/settings` toggle, as the PR states.

## Native macOS (Darwin arm64, macOS 15.7.4)

- Run 45576386761 (`verify/pr-772-f7a2a5c1-native-host`, pinned to `f7a2a5c1`): `nori-tui` program_status unit tests 35/35. `nori-config` passed. The PR's PTY tests with their Linux gate removed: `program_status` (7) plus the `host_attach` controller/watcher test, 5 rounds, **0 failed of 40**. `nori-tui --lib` 1926 passed. The full `tui-pty-e2e` suite passed except 2 of 20 in `host_attach`, which `main` gates to Linux.
- Run 45577851323 (`verify/pr-772-f7a2a5c1-native-attach-ab`): those two tests (watcher permission answer, busy Enter fallback) fail **8/8 at base `80992049` and 8/8 at `f7a2a5c1`**. They are inherited, matching the rates recorded for #751–#757.
- The tmux/raw-byte S45 scenarios ran on Linux only. Coverage on native macOS is the repo's PTY tests above.
