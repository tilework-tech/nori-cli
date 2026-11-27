# nori-tui

Path: `nori-rs/tui`

## Purpose

`nori-tui` is the Ratatui frontend of the Nori CLI. It owns terminal input and
rendering, presentation state, pickers and overlays, approvals, and the
terminal lifecycle. One `App` serves two entries. `run_main` serves plain
`nori`, `nori resume`, and `nori cloud`; it owns a private host for its whole
run. `run_joined_app` serves `nori attach`; it joins sessions on another host
and never owns them. The crate also builds
the standalone `nori-tui` binary, which runs the same `run_main`. Using the
TUI (sessions, keys, slash commands) is owned by
[`docs/tui.md`](../../docs/tui.md). Terms follow
[`GLOSSARY.md`](../../GLOSSARY.md). The UI lists **sessions** (ACP sessions);
UI strings keep that word.

## Main modules

| Module | Role |
|---|---|
| `lib.rs` | `run_main`: config, onboarding, automatic worktree, `nori_host_runtime::launch`, `App::run`; `run_joined_app`: `App::run_joined` on a running host |
| `app/` | `App`: main `select!` loop (`run_loop.rs`), session deck, session lifecycle and resume, delegation, `host_link.rs` (owned or joined host link), joined session view (`joined_sessions.rs`, `joined_feed.rs`) |
| `chatwidget/` | `ChatWidget`: one session's view, composer submit, approvals, goals, pickers, queue list and recall (a host session's queue is resumed and reclaimed through the host, `queue_host.rs`; its prompt history is recorded, recalled and searched on the host, `history_host.rs`; its custom prompts are listed from the host and their scripts run there, `prompts_host.rs`; its goal is read when the session comes on screen and changed on the host, `goal_host.rs`; `/compact` asks its host to compact it, `compact_host.rs`; `!` commands run on its host when this terminal is local to it, a remote terminal gets the gray `-32018` hint instead, and every `user_shell` event, live or replayed, shows as a `Ran` command cell with its output, `shell_host.rs`; its `/context` report and footer usage come from the host, `context_host.rs`) |
| `bottom_pane/` | Composer, footer, popups, approval overlay |
| `presentation/` | Private projection of ACP updates into display items |
| `history_cell/`, `exec_cell/`, `client_tool_cell.rs`, `markdown*`, `diff_render.rs` | History cells and their rendering |
| `host_client/` | Client of a `nori host`: connection, reconnect, reply matching; `drive` (WebSocket) and `drive_pipe` (owned host); `AttachOptions`; `HostEndpoint` keeps the local token and presents it on every reconnect; `HostError` carries the JSON-RPC `code`; `HostFeatures` and the host's agent list read from its `initialize` capabilities |
| `program_status/` | OSC 7501 terminal status: `ProgramStatusTracker` (fold over one session's events) and `ProgramStatusReporter` (encode and write) |
| `nori/` | Nori pickers (agent, config, fork, resume, skillset, hotkey, MCP), onboarding, session header, exit card, update prompt |
| `resume_picker/`, `session_*.rs`, `viewonly_transcript.rs` | Resume picker, session preview and stats, view-only transcripts |
| `tui.rs`, `frame_scheduler.rs`, `custom_terminal.rs`, `insert_history.rs`, `render/` | Terminal setup, frame scheduling, inline viewport, scrollback insertion |
| `style.rs`, `color.rs`, `terminal_palette.rs`, `shimmer.rs` | Styles and terminal colors |
| `storybook` (feature `storybook`) | Production view specimens for `examples/` |

```text
nori / nori-tui                          nori attach
      |                                       |
 run_main                                 run_joined_app
      |  nori_host_runtime::launch            |  ManagedClient (WebSocket, token)
      v                                       v
 App::run --HostLink::start--> OwnedHost  App::run_joined --HostLink::join-->
   |  manager  -> ChatWidgets               host_client::drive --> running nori host
   |  pipe     -> host_client::drive_pipe     |  JoinedFeed -> parked ChatWidgets
   v                                          v
 SessionEvent::{Acp, Nori} --presentation--> history cells, bottom pane
```

## Dependency direction

```text
nori-cli --> nori-tui --> nori-host-runtime --> nori-harness --> nori-acp-host
                 \------> nori-harness, nori-config, nori-protocol,
                          nori-tui-components, nori-skillsets, nori-installed,
                          nori-auth-providers, codex-* support crates
```

`nori-tui` must not depend on `nori-cli`. Wire mechanics stay in
`nori-acp-host`; the TUI adds no protocol vocabulary of its own.

## Invariants

- Never construct a `SessionManager` here. Sessions belong to the owned host,
  so they end when it stops.
- Only `run_main` builds an `OwnedHost`. A joined App never holds the
  in-process half and never ends a session when its terminal closes.
- The App has two entries that share one loop: `App::run` on an owned host
  (plain `nori`, `SessionManager` in-process) and `App::run_joined`
  (`run_joined_app` in `lib.rs`) on a host it joined, with no
  `SessionManager`, `HarnessHandle`, or `ForegroundConfig`
  (`session_manager` and `foreground_config` are `Option`).
- `HostLink` either owns its host (`start`, over the in-process pipe) or
  joins a running one (`join`, over `host_client::drive`). For an owned host,
  the App sends `initialize` before it reads UI events; a refused initialize
  or a stopped pipe ends the loop with an error, and `stop_or_leave` stops
  every session after the loop, even on error.
- A joined link re-sends `initialize` on every connection, takes its role
  from the reply, and lists sessions every second while connected as the
  heartbeat. A session event that is a delegation change, a notice, or an
  agent request also lists at once (unless a list is pending), so Alt+Down
  reaches a child as soon as the parent's notice names it. Disconnects, role
  changes, stops, and a refused initialize become `HostUpdate`s that never
  end the loop; a stop or refused initialize ends the connection for good. Calls made while
  disconnected or after a stop are refused at once; calls unanswered when
  the connection drops fail as possibly applied. Leaving a joined host only
  closes the connection; its sessions keep running.
- Joined session view: every listed session gets a parked widget
  (`ChatWidget::from_host_feed`). `JoinedFeed` applies `_nori/session/event`
  envelopes once each, in seq order per conversation, and follows forks. A
  gap is reported once; that conversation then applies nothing more until it
  is resubscribed from its cursor. The connection's single subscription
  follows the session on screen and is re-sent after every reconnect and
  every switch, so it replays exactly what this terminal has not applied.
  Parked sessions take status, title, and preview from the 1 s list. Feed
  events take the same delivery path as `AppEvent::SessionEvent`.
- In joined mode, leaving (Ctrl+D, `/detach`, `/quit`) sends no `Shutdown`
  and closes no session; prompts and session actions go through host calls
  (`_nori/session/*`). Actions that need an in-process session show "This
  needs a session running in this terminal's own host." A host session that
  ends with reason Shutdown is treated as closed, not as the end of the
  terminal; the next session or a placeholder comes on screen, and only
  "Session ended…" prints. Esc-backtrack is off: the joined transcript pager
  (PgUp in `app/joined_pager.rs`, or Ctrl+T) opens with
  `TranscriptOverlay::without_edits`, offers only "q/esc to quit", and Esc
  closes it.
- A joined App starts on the alternate screen; `/screen` and Ctrl+L switch to
  the inline view and back. Leaving from the alternate screen prints no
  transcript and no exit summary.
- `ChatWidget` holds the joined link's `HostConnection` (none for an owned
  host) and picks its command `Surface` from it: `Owned` without one,
  `Joined` with one. `Joined` refuses host commands it has no screen for
  with "not available in nori attach yet" and in-process-only commands with
  "not available on this host yet". `HostConnection.features` gates what the
  host serves, such as folders for `/new` and `find_paths` completion.
  In-process commands the host serves a method for run joined once the host
  advertises it (`command_availability::served_by_host`): `/goal` with
  `goal`, `/context` with `context`, `/compact` with `compact`, `/agent` with
  `agents`. Older hosts keep the refusal. Joined `/agent` (`chatwidget/host_agents.rs`)
  lists `HostConnection.agents`, read from the host's `initialize`
  `nori/preparedAgent` `agentOption` (`host_client/agents.rs`); a row whose
  description is not "Installed" cannot start. It starts a session with the
  agent (`_nori/session/new` with `agent`), or, before the first prompt,
  changes the session's agent through `session/set_config_option` `agent`
  and ignores the old agent's `Shutdown` it asked for (`AgentChange`).
  `slash_command::builtin_named` resolves aliases such as `/agents`; the
  command popup matches them but lists the builtin once.
  A refused builtin command stays in the composer as `/command`; a command
  the composer does not know goes to the agent as a prompt. With no session
  on screen, `@` asks a joined host that serves `find_paths` to search its own
  folder (`app/file_mentions.rs`); otherwise it searches the session on screen.
  Both searchers (`host_client/file_search.rs`, `file_search.rs`) skip a query
  only while the same one is still in flight or scheduled; once it lands, the
  same text asked again is searched again, so a popup reopened on unchanged
  text gets an answer. The composer reports a query only when its file popup
  needs one (a new popup or changed text), not on Up/Down through matches. A
  joined watcher's submission is refused with the draft kept, and Alt+T sends
  `AppEvent::TakeHostControl`. Ctrl+C goes to `ChatWidget::joined_ctrl_c`
  first: only a connected controller cancels, including a turn another
  terminal started, and never resends a pending cancel (an empty prompt then
  detaches). A watcher or a reconnecting terminal sends no cancel on Ctrl+C
  or Esc; during a turn either key prints the refusal and keeps the draft. On
  a stopped host Ctrl+C clears the draft or detaches. A `/new` session refused while another was being set up is retried
  at the next list.
- Composer hints (`bottom_pane/composer_hints.rs`) are a footer segment,
  drawn by `bottom_pane/footer.rs`. `ChatWidget` pushes its session facts
  (watcher, own turn, paused queue, host shell and path search, guarded
  steering from the agent's `initialize` reply) into `ChatComposer` on every render and
  height query, so hints never go stale. Popups, Ctrl+C and Esc reminders, and
  footer hint overrides replace the row. A watcher or reconnecting terminal
  gets no send keys. While hints show, the composer draws no placeholder tip.
  On a narrow row, hints drop whole from the end before the right group sheds.
- Match `SessionEvent::Acp` and `SessionEvent::Nori` first, then project. Keep
  display inference private in `presentation/`. Never add a public normalized
  ACP enum for rendering.
- Replace the active widget only through `shutdown_current_conversation` and
  then `install_active_widget`. A direct assignment to `App.chat_widget` loses
  or leaks a session.
- Session-scoped `AppEvent`s that can arrive late carry a `SessionGeneration`.
  Act on that generation, not on "the active widget".
- A `Closed` or `Done` session stays in the deck. Filter with
  `SessionStatus::is_closed()`; a check for `Closed` alone misses finished
  delegated children.
- History from a widget reaches `App` through its session-scoped sender. Do not
  print a parked session's history directly.
- The startup welcome card (`ChatWidget::take_welcome_card`) is not written
  to history at once. `StartupHeader` (`app/startup_header.rs`) holds it at
  the top of the inline viewport, outside `transcript_cells`, while
  `StartupFace` (`nori/session_header/startup_face.rs`, a pure timeline)
  animates its title. Readiness is the `SessionStarted` branch for
  `PickerPurpose::Startup` in `app/event_handling.rs`; the composer, config,
  and input never wait for the face. The card is written through
  `append_history_cell` in the same draw that shrinks the viewport, so rows do
  not move. Other history before readiness freezes the face open and writes
  the card first, so a failed start never winks. History during the finale
  is shown under the card and written with it, unless it does not fit. A
  consolidated stream, the full-screen view, exit, or a header taller than the
  free rows settle it at once; an overlay only pauses it.
- Only that startup card has the face. Cards written at `SessionStarted`
  (resume, agent switch, cloud) and `tui.animations = false` show the static
  wordmark `〉nori CLI vX`; `/status` and the post-compaction header keep
  `› Nori CLI vX`.
- `ProgramStatusTracker` is a pure fold over one session's `SessionEvent`s.
  It settles a turn from the prompt's own response, matched by request ID.
  Every `ChatWidget`, parked ones included, folds its events, so a session
  brought forward already knows its state. `App` reports only `chat_widget`'s
  status after each main-loop event; background sessions never write. A
  joined App reports the same way.
- A host operation's error reaches its widget as a typed `HostError`, so a
  widget can branch on the JSON-RPC code. `shell_host.rs` does: `-32018`
  shows `history_cell::new_hint_event` (dark gray) rather than a red error.
  Either way the refused `!cmd` goes back to the prompt.
- `ProgramStatusReporter` writes only when the bytes change, never when
  stdout is not a TTY, and clears once if it ever reported.
- `frame_scheduler.rs` keeps at least 8 ms between draws: a lone request
  draws at once, a burst of requests draws a few frames, not one per request.
- The full-screen view (`TranscriptRows` in `app/full_screen_frame.rs`)
  renders each cell once, again only on a width change or a generation bump. Consolidating a finished agent reply into one
  Markdown cell goes through `TranscriptCells::replace_tail`, so only the
  replaced cells re-wrap; bumping the transcript generation instead re-wraps
  the whole history and made a long `nori attach` replay crawl.
- `streaming/controller.rs` renders streamed Markdown only on a commit tick
  or at finalize, where lines become visible; a chunk with a newline just
  marks a line finished. Live output is unchanged (one line per tick, a
  partial line waits for its newline), and a replay burst that finishes a
  reply between ticks renders it once instead of once per chunk.
- `transcript_reflow::agent_message_consolidation` replaces a reply's cells
  only when the trailing run reaches the reply's first cell. If another cell
  (such as a plan update) cut into the stream, the cells stay as streamed, so
  the inline reprint after Ctrl+L or `/screen` repeats no text.
- The session list view asks for a redraw only when its rows change, not on
  every frame while it is open; otherwise the full-screen view repaints
  continuously.
- Approval and sandbox policy come from `nori-config`. ACP session config
  options stay ACP schema values.
- "Fork" means ACP fork; going back to an earlier message is rewind. "Queue"
  and "pause" in user-facing text mean the prompt queue only.
- `-p` belongs to the top-level CLI as `--print`; the TUI flag set must not
  claim it.
- The library denies `print_stdout`, `print_stderr`, and the methods listed in
  `nori-rs/clippy.toml`.

## Styles

- Headers: bold. Primary text: default foreground. Secondary text: `dim`.
- Use ANSI colors only. `Color::Rgb` and `Color::Indexed` are disallowed by
  `clippy.toml`; `shimmer.rs` is the exception, because it only adjusts the
  default colors.
- Green: pointers and focus (row pointer, shortcut key, focused border,
  overlay rail), success, and additions. Keep selected text in the default
  foreground.
- Cyan: targeted information accents (a marker, key, value, status). Do not
  color whole paragraphs.
- Red: errors, failures, and deletions.
- Agent identity colors only on explicitly typed category tabs or cells:
  yellow or orange for Claude, white for Codex, blue for Gemini and
  Antigravity, green for Nori. The status block uses `#ff9e64` on the Claude
  agent name only.
- Avoid `black`, general-purpose `white`, `blue`, and `yellow` foregrounds.
  Use `reset` for the default. Identity, warning, and contrast over a fixed
  background are the exceptions.
- Interactive screens request terminal foreground and background colors while
  they own the input stream. Rendering uses the terminal default until each
  reply arrives. Release the input reader before exposing the shell or editor.
- Shared pickers get their styles from `nori-tui-components` through
  `component_theme` in `style.rs`.
- Every large list marks its focused row the same way: green `▏ ▕` rails at
  both edges, a green `›` pointer in its own column, and a shaded background
  when the terminal reports its background and true color. Without shading,
  the pointer and rails are bold. Row numbers and the `○`/`●` multiple-choice
  dots sit in their own columns after the pointer and never replace it. The
  shared `Picker` and `OverlayMenu` draw this; see `tui-components/src/focus.rs`.
  Short prompts in `ListSelectionView` (tool permission, delegation,
  approval and API key confirmations) use the same marks through
  `render_focused_rows`. Completion popups (`/`, `@`, `$`) keep their own
  highlight.
- `(current)` marks the chosen value in a list and is the only spelling.
  `/sessions` and the attach list mark the session on screen with
  `(on screen)`. On/off settings show only `(on)` or `(off)`.

## Tests

| What | Where | Run |
|---|---|---|
| Unit and render tests, `insta` snapshots | `#[cfg(test)]` modules, `src/**/snapshots/` | `cargo test -p nori-tui` |
| vt100 emulator suite | `tests/all.rs`, `tests/suite/` | `cargo test -p nori-tui --features vt100-tests` |
| Real binary in a PTY | `nori-rs/tui-pty-e2e` | `just test e2e` (repository root) |
| Visual specimens | `examples/` | `cargo run -p nori-tui --features storybook --example <name>_storybook` (`context`, `status_card`, `exit_card`) |

Run commands from `nori-rs/` unless noted. Review snapshot changes with
`cargo insta pending-snapshots -p nori-tui`, then
`cargo insta accept -p nori-tui`. Any user-visible UI change needs snapshot
coverage.

## Where `main` differs

- c06: Approve all in `/approvals` still opens the full-access
  confirmation behind `notices.hide_full_access_warning`, and
  `codex_common::approval_presets` remains for the Windows sandbox prompt.
- c03: the picker titled "Fork conversation" (`nori/fork_picker.rs`) offers
  both ACP fork ("Branch from current point") and rewind to a message.
  Internal rewind code is named "backtrack" (`app_backtrack.rs`).
- c04: goal commands and status use "pause" (`/goal pause`, "paused").
