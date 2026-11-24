# #752 per-session approvals: `e4728d92`, then `b959a6ea`

`b959a6ea` adds one commit on `e4728d92`: the attach mode menu's name column goes
from 24 to 26 characters (plus its snapshot). Native evidence from `e4728d92`
carries over for everything except that padding. The padding is covered by the
Linux reruns at `b959a6ea` below and by the author's macOS run 45414139758.

## Verdict: changes requested (two places where plain `nori` disagrees with the host)

Per-session behaviour is right everywhere I checked, and the host and attach
terminals always agree. In plain `nori`, two older mode labels contradict the
host-reported mode.

## Acceptance matrix (tmux PTY, synthetic agent server, independent host observer)

Host state comes from an observer connection (`session/load` on the host,
`modes.currentModeId`) and from the agent server's frame ledger. UI state is read
from each terminal's `Approvals: …` label.

| Requirement | Scenario | `5cb13fcd` (baseline) | `e4728d92` | `b959a6ea` |
|---|---|---|---|---|
| `/approvals` offers Ask / Agent / Approve all, no presets, never sent as a prompt | S10 | 2/3 (no menu in attach) | 3/3 | 3/3 |
| New sessions start at `[host] default_mode` and never copy the foreground mode | S12 | 0/3 | 3/3 | 3/3 |
| Change reaches only the foreground session; footer follows it; watcher sees `/approvals` greyed | S13 | 0/3 | 3/3 | 3/3 |
| Ask hidden for an agent server without Ask | S13 | fail | fail (decision 2 pending Cliff) | fail (same) |
| Retired presets migrate once (Full Access, Read Only, Read Only without Ask); sandbox kept | S15a–c | 6/9 (vacuous passes) | 9/9 | 9/9 |
| Two sessions, three terminals, Alt+T handoff; host and every terminal agree | S37a | 1/11 | 11/11 | 11/11 |
| Current config read as-is on repeat launch; mode change keeps sandbox settings | S37b | 4/7 | 7/7 | 7/7 |
| Plain `nori`: mode stays with its session across `/new` and Alt+, | S37c | n/a | 4/4 | 4/4 |
| Approve all enforced by the host (the agent server's permission request is answered `allow`, no prompt) | S37d | n/a | 2/2 | 2/2 |

Full matrix at `e4728d92` (Linux, 3 lanes): 510/531. On the 489 checks shared
with the parent's full run (`c31726f7`): 468 against 456, no regressions,
12 newly passing; 42 new checks (S35i/j, S37) all pass.

## Native macOS (`e4728d92`)

- Run 45415232002: full suites pass for `nori-harness`, `nori-host-runtime`,
  `nori-acp-host`, `nori-config`, `nori-tui` (1851), `nori-cli` and
  `nori-auth-providers`. `tui-pty-e2e` passes apart from three `host_attach`
  tests that are inherited from `main` (see `results/5cb13fcd-macos-attach`).
  With the Linux-only gate removed, the new attach test passed in the full run.
- Repeats, 10 rounds: `host_approvals`, `approval_presets_migration`,
  `host_startup`, `host_new_session`, `slash_menu_escape` and the attach Escape
  test pass in every round. `only_the_controller_changes_a_session_mode_and_every_terminal_shows_it`
  failed 4/10 when it ran in parallel with the Escape tests.
- A/B, run 45418024952 (same build, run alone): the original test passed 12/12.
  A copy that pauses before each Enter passed 11/12. Its single failure
  (`native/r12-*.txt`) is a partial frame: the menu title is drawn, the rows are
  blank, and the header already shows the right mode. `open_mode_menu` returns
  on the title and the test then asserts on one snapshot. That is a test race,
  not wrong state. I did not keep the panic text from the 4 parallel failures,
  so I can't say they are the same race.

## Linux

- CI 45414113959 at `b959a6ea`: pass.
- Local at `b959a6ea`: `host_attach` (gate removed, 17/17), `host_approvals` 2/2,
  `approval_presets_migration` 3/3; approvals scenarios 42/43 (the one failure is
  decision 2).

## Findings for the owner

1. **The plain-`nori` mode badge goes stale.** The footer's `[ … ]` badge shows the
   agent server's own mode and does not follow host changes. In S37c the session
   is in Ask (the agent server was set to `default`, and the footer says
   `Approvals: Ask`), but the badge still reads `[ acceptEdits ]`. After `/new`
   and Alt+, back, it still shows the other session's value
   (`S37c/first-session-ask.png`, `back-to-first.png`). The host replaces the
   agent server's mode updates with its own, so the badge only ever sees the
   first value.
2. **The startup card ignores the host mode.** The card prints the TUI config's
   preset label: `Agent approvals` when the session actually starts in Ask from
   `[host] default_mode = "ask"` (S37b) or a migrated Read Only (S15b). With Full
   Access it reads `Approve all approvals`.
3. **The new attach test races on macOS** (see above). It is hidden by the
   `host_attach` Linux-only gate. Waiting for `"Agent (current)"` /
   `"Approve all (current)"` instead of asserting on one snapshot would close it.
4. **Decision 2 (Cliff).** The TUI lists Ask for every agent and lets the host
   refuse it. Reading the code (not observed): the host's own advertised modes
   (`offered()` in `host/acp/config.rs`) already omit Ask for such agents, so
   the attach menu and the host's mode list differ for those agents.

All data is synthetic: generated project roots, throwaway homes and scripted
agent servers.
