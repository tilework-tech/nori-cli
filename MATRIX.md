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

### Added 2026-11-24 (stack #739-#746)

Columns: main `f9eedcac` (`results/f9eedcac-ext`), then the first head where the check passes.
Every head from #739 to #746 also re-ran S01-S20 with no regression against its base.

| Requirement | Scenario / check | Observed by | main | First green |
|---|---|---|---|---|
| Plain `nori` owns a private host: no socket, discovery file or `host ls` entry; adapters are its children | S21a/b owned.* | process, adapter | pass | main (kept through #746) |
| Hangup and SIGTERM end every session and restore the terminal | S21a/b owned.nori_exits, no_adapter_outlives_nori, turn_stopped, terminal_restored_after_sigterm | process, adapter, screen | pass | kept |
| A background host never sees plain `nori`'s live session and survives its quit | S21c | wire | pass | kept |
| `/agent` switches agent servers in the same folder; old server gets no prompt; all stop on quit | S22 agent.* | adapter, screen | pass | kept |
| Client-local commands work and never reach the agent | S23 local.* | screen, adapter | pass except `!` | kept |
| `!cmd` shows its output | S23 local.echo_works | screen | FAIL | not yet (baseline gap) |
| `nori resume` shows its picker before any session; Esc starts one in the launch folder | S24 picker_first.* | adapter, process | pass | kept |
| Permission answered once with the chosen option; repeat request answered separately (plain) | S25a perm.* | adapter | pass | kept |
| Watcher Enter cannot answer; controller answer reaches the adapter once; watcher sees outcome | S25b perm.* | adapter, screen | pass | kept |
| Queued follow-up delivered exactly once after the turn | S26a queue.* | adapter, screen | pass | kept |
| Reclaimed follow-up (Up, Enter) returns to the composer and is never sent | S26b queue.* | adapter, screen | pass | kept |
| Interrupted queue pauses; Enter resumes and delivers exactly once | S26c queue.* | adapter, screen | pass | kept |
| Standalone `nori-tui` binary: inline, one session, private host, clean exit | S27 standalone.* | screen, pty, adapter | absent | #741 |
| Escape cancels the observed turn once; the next turn is not cancelled (plain) | S28a cancel.* | adapter | pass | kept |
| Same in attach | S28b cancel.* | adapter | FAIL | #743 |
| Refused `/close` keeps the session working; retry closes; no cancel | S29a/b close_refusal.* | adapter, wire | FAIL | #744 |
| Refused `/close` shown once (plain) | S29a close_refusal.shown_once | screen | FAIL | #746 |
| Refused `/close` shown once (attach) | S29b close_refusal.shown_once | screen | FAIL | **still FAIL at #746** (three lines) |
| Prepared agent: capability, catalog rows, activation on the same adapter process, single use, discard and disconnect stop unused adapters | S30 prepared.* | wire, adapter, process | FAIL | #745 |

Further pre-existing findings:

11. `!cmd` runs but nothing is shown: the TUI drops `NoriEvent::UserShell` (`tui/src/chatwidget/event_handlers.rs`), at main and every head.
12. `nori resume` lists no saved rows for the synthetic agent server at main and every head (information; the picker-first UI layer is expected to use #745).

Not covered: Escape pressed before host admission returns (#743) is covered only by the repository PTY test with its admission hook.

Run several commits at once with `ACC_LANE=<name>` (own work dir and tmux socket); re-run part of a commit with `--merge --only ...`.

### Added 2026-11-24 (#747, #748)

| Requirement | Scenario / check | Observed by | #746 `c94867b1` | First green |
|---|---|---|---|---|
| `/resume` lists the agent server's own saved row and restores it on a separately prepared server process; history replays; prompt reaches it; one load | S31a restore.* | adapter, process, screen | FAIL (loads on the live adapter) | #748 |
| `/agent` lists and restores another server's saved row | S31b restore.* | adapter, process, screen | pass | kept |
| Refused `/resume` restore keeps the current session and shows the agent's message | S31c | adapter, screen | FAIL (session replaced) | #748 |
| Refused `/agent` restore shows the agent's own message | S31d restore.refusal_message_shown | screen | FAIL (generic "Failed to load ACP session") | #747 |
| Esc on `/resume` rows reaps the prepared server; current session keeps working | S32a | process, adapter | FAIL (no prepared server) | #748 |
| Esc on `/agent` rows reaps the prepared server | S32b | process | pass | kept |

Honest gap: in plain `nori` the owned host shares the process with the TUI, so whether `/agent`
goes through the host or in-process is only visible through S31d's message and the repository tests.

### Added 2026-11-24 (#749)

| Requirement | Scenario / check | Observed by | #748 `b3a4e8df` | #749 `5fa7be45` |
|---|---|---|---|---|
| `/new` starts on a fresh server process; prompt reaches it; previous server keeps running | S33a new.* | adapter, process | pass | pass |
| Refused `/new` shows the agent's message and keeps the current session answering | S33b new.refusal_* | adapter, screen | FAIL (current session lost) | pass |
| `/new DIR` sets the new session's folder in plain `nori` | S33a/b new.dir_argument_sets_session_folder | adapter | FAIL | FAIL (later cwd layer) |

`new.uses_foreground_session_folder` is recorded only when the foreground session is in a different folder
from the launch folder; plain `nori` cannot reach that state until `/new DIR` works, so it is not yet evaluated.
### Added 2026-11-24 (#749 early input, #746 repair)

The synthetic server takes one-shot delays from its control directory: `init-delay` holds the next
server's `initialize` reply and `new-delay` holds the next `session/new` reply, each for 6 s. The scenario
types into the terminal only after the adapter records the start of the delay and checks that the Enter
landed inside it, so the race is exercised on every run instead of depending on timing.

| Requirement | Scenario / check | Observed by | #749 `5fa7be45` | #749 `3b2010d4` |
|---|---|---|---|---|
| Input typed while the `/new` server initializes reaches only the new session, once, after its `session/new` | S33c early.* | adapter (pid, timestamps), screen | FAIL (sent to the previous session before the new one existed) | pass |
| Same, while `session/new` is pending | S33e early.* | adapter, screen | FAIL (sent to the previous session) | pass |
| Refused `/new` during initialize: input stays unsent, returns to the composer, Enter sends it once to the previous session | S33d early.* | adapter, screen | FAIL (sent at once; composer empty) | pass |
| Same, refused while `session/new` is pending | S33f early.* | adapter, screen | FAIL (sent at once; composer empty) | pass |

| Requirement | Scenario / check | Observed by | #746 `c94867b1` | #746 `36947010` |
|---|---|---|---|---|
| Refused `/close` shown once (attach) | S29b close_refusal.shown_once | screen | FAIL (three lines) | pass |
| Two attached terminals, controller's `/close` refused: one `session/close` and one refusal at the adapter; controller shows it once, watcher at most once; session keeps answering in both | S29c close_refusal2.* | adapter, wire, screen | FAIL (controller three lines) | pass |

Full matrix at `36947010`: 322/369. Against `c94867b1`, S29b and S29c now pass and no other check changed;
the remaining S33 failures are `/new` features that arrive in #749. Linux CI and exact-commit native macOS
(`verify/pr-746-36947010-native-host`) passed.

| Second `/close` while the refused first is still in flight (adapter holds the close 5 s): one adapter close, refusal shown once, session keeps answering | S29d close_twice.* | adapter, wire, screen | not run | FAIL at `36947010` (refusal shown twice, plus "already in progress"); pass at `25c42dba` |

### Restacked heads, 2026-11-24

| PR | Head | Run | Result |
|---|---|---|---|
| #746 | `25c42dba` | S29a-d | 28/28 pass |
| #747 | `735847f9` | S29a-d, S31a-d, S32a-b | 51/56; S31a, S31c and S32a fail as expected, first green at #748 |
| #748 | `4c9e2407` | S29a-d, S31a-d, S32a-b | 56/56 pass |
| #749 | `e86f3803` | full matrix | 317/353; no check regressed against `3b2010d4`; S29b-d and S33c-f pass; S33a/b `/new DIR` still fails (later cwd layer) |

Native macOS at each exact head passed. Linux CI passed for #746, #748 and #749. For #747, Linux CI failed once in
`nori-acp-host` `test_cancelled_spawn_reaps_child_during_initialization` (5 s pid-file timeout). #747 does not change `acp-host`.
The test passed 8 of 8 runs locally, and the full lib suite passed 105/105.

Later heads (09:00): #746 `35297393` adds only a test fix (script agents run through their interpreter, which avoids
ETXTBSY). #747 `5fbb7cf6`, #748 `2fb8b41a` and #749 `c476d07d` have the same patches as the heads above, by `git patch-id`.
The results above carry over, and native macOS passed at all four. Linux #747 then failed
`managed_recovery::disconnected_machine_output_is_saved_and_its_live_lane_can_reattach` (`_nori/host/replay` -> -32016
watcher is read-only). That failure predates the stack change (also seen at #743 `6ed85ffb` and on a #739 macOS run) and was routed on #747.

Observation, not a finding: the watcher shows the adapter's bare stream error (`close declined by …`) without the
`Failed to close the session:` prefix, because only the terminal that sent the close knows it is a close refusal.

