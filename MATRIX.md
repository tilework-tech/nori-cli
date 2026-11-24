# Host TUI acceptance matrix

Independent acceptance checks for the shared host TUI stack (one TUI for
`nori` and `nori attach`, driven by a host). Every check runs the real
`nori` binary in real tmux PTYs and, where the behaviour lives on the host or
in the agent server, also observes it from outside the screen:

- **screen**: tmux visible text, scrollback, `#{alternate_on}`, `#{cursor_flag}`, `#{pane_title}`
- **pty**: raw output bytes (alternate screen, bracketed paste, focus and keyboard-mode pairs) and `stty -a` in the shell after exit
- **wire**: an independent observer connection to the host (`session/list`, role enforcement)
- **adapter**: the ledger of every frame each synthetic agent server received (`session/new` cwd, mode changes, prompts, ticks of a running turn, close/EOF)

Status columns are per tested commit. `pass` / `FAIL` come from `results/<sha8>/results.json`.
At main (`f9eedcac`) a `FAIL` usually means the feature has not been built yet; the
"Layer" column names the stack layer expected to turn it green.

| Requirement | Scenario / check | Observed by | Layer | f9eedcac |
|---|---|---|---|---|
| Plain `nori` stays inline | S01 inline.no_alt_screen | screen | 2 | pass |
| Plain `nori` leaves its transcript in scrollback | S01 inline.transcript_in_scrollback | screen | 2 | pass |
| Exactly one bottom hint row in plain `nori` | S01 hint_row.exactly_one / at_bottom | screen | 2/6 | FAIL (no hint row) |
| Plain `nori` exit restores the terminal | S01 exit.* (shell prompt, alt off, cursor, escape pairs, cooked tty) | screen, pty | 2 | pass |
| Plain `nori` owns its host: nothing outlives it | S01 lifetime.no_host_outlives_terminal, adapters_stopped | wire, adapter | 2 | pass |
| Quitting plain `nori` mid-turn stops the turn | S02 lifetime.turn_stopped_after_quit | adapter | 2 | pass |
| `nori attach` uses the alternate screen | S03a-c attach.alt_screen_on | screen | 6 | pass |
| Exactly one bottom hint row in attach | S03a-c hint_row.* | screen | 6 | pass |
| Ctrl+D detaches, turn keeps running | S03a detach.* | screen, pty, wire, adapter | 6 | pass |
| `/quit` detaches, turn keeps running | S03b detach.* | screen, pty, wire, adapter | 6 | pass |
| PTY hangup (EOF) detaches, turn keeps running | S03c detach.client_exits_on_hangup, turn_keeps_running | process, adapter | 6 | pass |
| Attach leaves shell scrollback untouched | S03a/b alt.scrollback_untouched | screen | 6 | pass |
| Reattach sees the live turn | S03a reattach.sees_live_turn | screen | 1/6 | pass |
| `/close` ends the session on host and adapter | S04 close.* | wire, adapter | 3 | pass |
| `/` menu in attach lists the normal commands | S05 slash.menu_opens_with_normal_commands | screen | 3 | FAIL |
| Unsupported commands greyed with a reason | S05 slash.{diff,browse,undo}_listed_with_reason | screen | 3 | FAIL |
| Unsupported commands never reach the agent as prompts | S05 slash.unavailable_not_sent_as_prompt | adapter | 3 | FAIL (sent as prompt) |
| Empty remote host: `/new` uses the host folder | S06 cwd.empty_remote_falls_back_to_host_folder | adapter | 3 | pass |
| `/new DIR` overrides | S06 cwd.explicit_override | adapter | 3 | pass |
| Plain `/new` inherits the foreground folder | S06 cwd.plain_new_inherits_foreground | adapter | 3 | FAIL (launch folder) |
| `/new DIR` Tab completes on the session filesystem | S06 cwd.dir_completion_from_session_fs | screen | 5 | FAIL (completes the client's `laptop-only/`) |
| The new-session line names the inherited folder | S06 cwd.confirmation_names_folder | screen | 3 | FAIL |
| Remote `@` offers the session's files | S07 at.alpha_files_offered | screen | 5 | FAIL (`loading...` forever) |
| Remote `@` never offers laptop files | S07 at.no_laptop_files, laptop_file_not_found | screen | 5 | pass (vacuous: no results at all) |
| Same logical path, other root, after switch | S07 at.beta_files_offered_same_logical_path | screen | 5 | FAIL |
| No stale completions after switch | S07 at.no_stale_alpha_results | screen | 5 | pass (vacuous) |
| Watcher Enter keeps draft, shows Alt+T hint once | S08 watcher.* | screen, adapter | 3 | FAIL (notice printed twice) |
| Host enforces watcher read-only | S08 host.observer_cannot_{set_mode,prompt} | wire | host | pass |
| Alt+T transfer; old controller sees watching | S08 transfer.* | screen | 1/3 | pass |
| No automatic promotion | S08 transfer.no_automatic_promotion | screen | host | pass |
| Reconnect keeps drafts | S09 reconnect.controller_draft_kept | screen | 1 | pass |
| `/approvals` offers host modes, no legacy presets | S10 approvals.* | screen, adapter | 4 | FAIL (sent as prompt) |
| Thought text, title, read ranges, search hits (Agent Home) | S11a (plain), S11b (attach) home.* | screen | regression | plain pass; attach title FAIL |
| New sessions start at `[host] default_mode` | S12 default.* | adapter | 4 | FAIL (`/new` ignores host default) |
| Mode change reaches only the foreground session | S13 approvals.change_reaches_only_foreground | adapter | 4 | FAIL |
| Ask hidden for agent servers without Ask | S13 approvals.ask_hidden_for_adapter_without_ask | screen | 4 | FAIL |
| Footer shows the foreground mode | S13 approvals.footer_shows_foreground_mode | screen | 4 | FAIL |
| Watcher sees `/approvals` greyed with Alt+T | S13 approvals.watcher_sees_greyed_with_take_control | screen | 4 | FAIL |
| Picker lists only the connected host | S14 picker.excludes_other_hosts | screen | 3 | pass |
| Resume a chosen agent server's saved sessions on the host | S14 resume.* | screen, adapter | 3 | FAIL |
| Full Access migrates to Approve all, sandbox kept, once | S15a migration.* | screen, config | 4 | FAIL (maps) / pass (sandbox, once) |
| Read Only migrates to Ask | S15b | screen, config | 4 | FAIL |
| Read Only migrates to Agent without Ask | S15c | screen, config | 4 | FAIL |

### Added 2026-11-24 (gap closure)

Columns: main `f9eedcac`, #738 `8ea6650e`.

| Requirement | Scenario / check | Observed by | Layer | f9eedcac | 8ea6650e |
|---|---|---|---|---|---|
| Watcher reconnect keeps its unsent draft and stays a watcher | S16 watcher_reconnect.* | screen, adapter | 1/3 | pass | pass |
| Controller keeps its role after a hard drop | S09 reconnect.controller_role_restored | screen | 1 | pass | pass |
| Silent loss (no FIN/RST) is noticed, reconnects, keeps draft and live turn | S17 silent.* | screen, adapter | 1 | pass (noticed at 10.1 s) | pass |
| Working indicator shows the current thought (plain) | S18a indicator.* | screen, adapter | regression | pass | pass |
| Working indicator shows the current thought (attach) | S18b indicator.thought_*_on_working_row | screen | 2/6 | FAIL (no indicator; thoughts stream inline) | FAIL (same) |
| `[tui] tool_output_max_lines` caps head/tail, diffs whole, transcript full (plain) | S19a cap.* | screen | regression | pass | pass |
| Same cap in attach | S19b cap.total_rows_at_most_cap, hidden_marker | screen | 2/6 | FAIL (cap ignored) | FAIL (same) |
| Tool output stays full by default | S19c (plain), S19d (attach) | screen | regression | pass | pass |
| Cancel an agent-started turn: one cancel, same broker turn id, cancelled stop, idle (plain, Esc) | S20a agent_turn.* | adapter, screen | regression | pass | pass |
| Same in attach (Ctrl+C); watcher cannot cancel; draft kept; watcher sees idle | S20b agent_turn.* | adapter, screen | regression | pass | pass |

## Pre-existing findings at main (`f9eedcac`)

1. `[host] default_mode` is ignored by `/new` from attach: the host sets the
   agent-mapped mode (`acceptEdits`) even with `default_mode = "ask"`
   (S12, adapter ledger). Only ACP `session/new` applies it.
2. A watcher pressing Enter prints the read-only notice twice (S08).
3. In attach, `@` opens a file popup that stays on `loading...` (S07), and `/new DIR` Tab completes from the client's folder, not the session's (S06).
4. In attach, `/approvals`, `/diff` and other slash text are sent to the agent as prompts (S05, S10).
5. The attach terminal title is not set from the agent (S11b; the pane keeps the shell's title).
7. Attach has no working indicator: thought text streams inline as a `│` block, so the
   current-thought indicator (#730) exists only in plain `nori` (S18b).
8. Attach ignores `[tui] tool_output_max_lines`: all 60 rows render with the cap at 10 (S19b).
   Plain `nori` caps correctly (4 head + 5 tail + `┄ 51 lines hidden`), leaves a 40-line diff whole,
   and keeps the Ctrl+T transcript uncapped (S19a).
9. In attach, a CONTROLLING terminal's hint row still offers `alt+t take control` (S09, S17 captures).
10. Pressing Ctrl+C a second time after an agent-started turn is already idle clears the
   attach draft (ordinary idle Ctrl+C). Recorded as information, not a failure (S20b).
6. Linux CI is green; native macOS on the main push fails only
   `requests_piped_before_stdin_closes_are_all_answered_by_the_joined_host`
   (`cli/tests/host_stdio.rs:424`), the pre-existing failure tracked by nori-cli #737
   (run 45293937282).

## Not covered by this harness (honest gaps)

- Delayed-cancel rejection (a cancel bound to an older agent-started turn is refused once a
  new turn starts) is covered only by the repository test
  `tui-pty-e2e --test attached_external_cancel` (`delayed_cancel_for_amber_does_not_interrupt_blue`),
  which drives turn activity through a fixture option rather than a real agent-started turn.
  S20 covers the end-to-end path with a real agent-started turn and broker-style
  `nori/turnId` / `nori/turnState` / `nori/stopReason`.
- Silent loss (S17) blackholes only the controller's link. A blackholed watcher, and a host
  that keeps the dead controller socket for longer than the client's 10 s liveness window,
  are not driven separately; S17 shows the reconnecting client regains CONTROLLING.
- A race that types `@` within milliseconds of a session switch is not forced;
  S07 types after the switch settles.
- Screen checks for menus and selections (S12-S15) match on visible labels
  (`Ask`, `Agent`, `Approve all`, `(current)`); they will be re-pinned to the
  implemented copy at layer 3/4 without weakening what they assert.
- Native macOS PTY runs of this harness are not available; macOS evidence comes
  from the exact-commit `verify/pr-<N>-<sha8>-native-host` CI runs.
