# #751 `5cb13fcd`: attached-terminal Escape on native macOS

Run: https://github.com/tilework-tech/nori-cli/actions/runs/45411751044 (branch
`verify/pr-751-5cb13fcd-native-attach`, commit `9430a5ca`). The job restores
`nori-rs/` from `5cb13fcd` exactly, then applies one verifier-only patch
(`.verifier/host_attach.patch`): it drops the `#![cfg(target_os = "linux")]`
gate from `tui-pty-e2e/tests/host_attach.rs` and appends
`verifier_attach_escape_trace`. The step asserts that this file is the only
difference from `5cb13fcd`.

## Entry points

Each test runs the real `nori` binary in a PTY (120x40): `nori host --port 0`
starts a detached host, `nori attach` opens the picker, `/new` starts a
session, and `nori host ls` reads the host's own session status. The
synthetic agent server is `mock-acp-agent`; its stderr lands in the host home,
where the trace counts `Mock agent: cancel` lines.

## Result: pass

| Step (trace frames) | Screen | `nori host ls` | Cancels at the adapter |
|---|---|---|---|
| 1. Turn running | streaming | working | 0 |
| 2. `/` typed | menu open | working | 0 |
| 3. First Escape | menu closed, `/` kept in the composer | working | 0 |
| 4. Second Escape | turn ends | ready | 1 |

- Full ungated run: the three Escape tests pass (`attached_escape_closes_the_menu_first_and_then_interrupts_the_turn`,
  `verifier_attach_escape_trace`, plain `slash_menu_escape`).
- Repeat: 10 rounds x 3 Escape tests = 30/30.
- Linux, same patch: 2/2 locally.

The "attach Escape is unit-tested only on macOS" gap is closed for this head.
The gate itself is not a platform dependency: every host_attach test builds and
runs on macOS.

## Other host_attach tests on macOS (not Escape)

Removing the gate exposed three tests that fail on macOS. Linux passes them.

| Test | `5cb13fcd` macOS (1 full + 5 rounds) | `main f9eedcac` macOS (5 rounds) |
|---|---|---|
| `attached_busy_enter_reports_fallback_and_tab_explicitly_queues` | 6/6 fail | 5/5 fail |
| `two_terminals_watch_take_control_and_refuse_the_previous_controllers_prompt` | 3/6 fail | 2/5 fail |
| `watcher_cannot_answer_a_permission_request_but_controller_can` | 6/6 fail | 5/5 fail |

Classification: inherited. `main` fails the same three tests at the same rates
(run https://github.com/tilework-tech/nori-cli/actions/runs/45415081456, gate
removed by `sed`, tree restored exactly from `f9eedcac`), so #751 did not cause
them. I have not traced the cause. Two of them wait on one terminal while a
second terminal's PTY goes unread, which matches the macOS small-buffer
pattern documented for #750, but that is a hypothesis, not a finding. They stay
hidden while the Linux-only gate remains.

Frames: `frames/*.png` rendered from the raw PTY stream of round 1
(`frames/*.ansi`), with the screen text, `nori host ls` output and cancel count
for each step. All data is synthetic.
