# #752 per-session approvals: repaired head `d4c1353e`, 2026-11-24

Base: #751 `5cb13fcd`. Previous verdict: changes requested at `b959a6ea`
(`results/b959a6ea/VERDICT.md`, `results/mixed/NOTE.md`). Binaries built from a
clean worktree at `d4c1353e` (nori sha256 191e7ddf…); old client `5cb13fcd`
(8610b2a9…), the same binary as the `b959a6ea` mixed-version run.

## Verdict: pass

Every finding from `b959a6ea` is fixed and observed fixed on a real PTY. The
three macOS `host_attach` failures are the same at the #751 baseline, so #752
does not cause them. No regressions in the full Linux matrix.

## Findings from `b959a6ea`, rechecked

| Finding | `b959a6ea` | `d4c1353e` | Evidence |
|---|---|---|---|
| Plain `nori` `[ … ]` badge stale after a mode change and on Alt+, | 3 fail | pass | S40a: badge equals the agent server's last mode at each step: start `acceptEdits`, Ask `default`, `/new` `acceptEdits`, Alt+, `default` with no mode request |
| Startup card guesses "Agent approvals" (host default Ask; migrated Read Only) | fail ×3 | pass | S40a–c: the card shows the folder only; footer Ask; badge `default` matches the agent server |
| Ask offered for an agent without Ask | fail | pass | S39a/b, S13: both menus offer exactly the host's list `[agent, approve-all]`; Ask is listed as not available with the reason; choosing it leaves the host in Agent, the agent server unchanged, and the host log records no `session/set_mode` (positive control: S38c logs one when Ask is sent) |
| Old `nori attach` on the new host unusable | 8/24 | 22/24 | S38a–c new ← old: history drawn, `Approvals: Agent` / `Approvals: Ask` notices, prompt reaches the agent server once and the reply is drawn, after another terminal changes the mode. The 2 failures are the old client's own missing `/approvals` (same as the old ← old control) |
| macOS attach mode-menu test race | 4/10 under load | 10/10 under load | run 45429424837, next to `host_startup` + `host_new_session` |

## Mixed versions (Linux, tmux PTY, synthetic agent servers)

| Host ← attach client | Checks | Outcome |
|---|---|---|
| `d4c1353e` ← `d4c1353e` | 40/40 | control, S38a–c + S39a/b |
| `5cb13fcd` ← `5cb13fcd` | 19/24 | control; failures are the old attach's missing `/approvals` |
| `d4c1353e` ← `5cb13fcd` | 22/24 | works; mode shown as a notice; failures as in the control |
| `5cb13fcd` ← `d4c1353e` | 20/24 | unchanged from `b959a6ea`: Ask applies on the old host, but the new client shows no label and no `(current)` row, because the old host never announces modes. Minor, not blocking |

## Approvals matrix and full matrix (Linux)

- S10, S12, S13, S15a–c, S37a–d: 43/43 (S13's Ask check now expects the
  coordinator default, Ask listed as not available, instead of hidden).
- S40a–c (new): 11/11 at `d4c1353e`; 6/11 at `b959a6ea`.
- Full matrix: 511/531 against 510/531 at `e4728d92` on the same 531 checks.
  The only difference is S13's Ask check. The 20 remaining failures are the
  same checks as before (known plain-`nori` gaps outside #752).
- Repo PTY suites, local, 5 rounds, `host_attach` (Linux), `host_approvals`,
  `approval_presets_migration`: 115/115.
- CI 45429382403 at `d4c1353e`: pass.

## Native macOS: run 45429424837 (`d4c1353e`) against baseline 45433284870 (`5cb13fcd`)

Both pin and verify their head; each branch differs from its PR head only in
`rust-ci.yml`, and the load step only removes the Linux gate from `host_attach`.
The load loop is identical except that the baseline runs `host_attach` alone
(the approvals suites do not exist at `5cb13fcd`).

Standard steps at `d4c1353e`: all pass (`nori-tui` 1853, host and harness
suites, full PTY list, startup/new-session/agent-switching repeat 0/50 fails).

| Test (ungated, 10 rounds under parallel load) | `5cb13fcd` | `d4c1353e` | Class |
|---|---|---|---|
| `watcher_cannot_answer_a_permission_request_but_controller_can` | 0/10 | 0/10 | inherited (also fails on `main`, run 45415081456) |
| `attached_busy_enter_reports_fallback_and_tab_explicitly_queues` | 0/10 | 0/10 | inherited |
| `two_terminals_watch_take_control_and_refuse_the_previous_controllers_prompt` | 7/10 | 8/10 | inherited, intermittent |
| 13 other `host_attach` tests | 10/10 each | 10/10 each | pass |
| `only_the_controller_changes_a_session_mode_and_every_terminal_shows_it` | — | 10/10 | new, pass |
| `host_approvals` (2), `approval_presets_migration` (5) | — | 10/10 each | new, pass |

Unresolved, not #752: the three inherited macOS attach failures are still
untraced and hidden from CI by the Linux-only gate.

## Not covered

- Mixed versions ran on Linux only.
- The doubled notice line in attach (transcript + status) is pre-existing
  presentation and also appears at `5cb13fcd`.
