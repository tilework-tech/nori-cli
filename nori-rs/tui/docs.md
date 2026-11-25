# nori-tui

Path: `nori-rs/tui`

## Purpose

`nori-tui` is the Ratatui frontend of the Nori CLI. It owns terminal input and
rendering, presentation state, pickers and overlays, approvals, and the
terminal lifecycle. It has two frontends. The foreground TUI (`run_main`)
serves plain `nori`, `nori resume`, and `nori cloud`; it owns a private host
for its whole run. The attach frontend (`run_attach`) serves `nori attach`; it
observes sessions on another host and never owns them. The crate also builds
the standalone `nori-tui` binary, which runs the same `run_main`. Using the
TUI (sessions, keys, slash commands) is owned by
[`docs/tui.md`](../../docs/tui.md). Terms follow
[`GLOSSARY.md`](../../GLOSSARY.md). The UI lists **sessions** (ACP sessions);
UI strings keep that word.

## Main modules

| Module | Role |
|---|---|
| `lib.rs` | `run_main`: config, onboarding, automatic worktree, `nori_host_runtime::launch`, `App::run` |
| `app/` | `App`: main `select!` loop, session deck, session lifecycle and resume, delegation, `host_link.rs` (owned-host pipe and stop) |
| `chatwidget/` | `ChatWidget`: one session's view, composer submit, approvals, goals, pickers, queue list and recall (a host session's queue is resumed and reclaimed through the host, `queue_host.rs`; its prompt history is recorded, recalled and searched on the host, `history_host.rs`; its custom prompts are listed from the host and their scripts run there, `prompts_host.rs`; its goal is read when the session comes on screen and changed on the host, `goal_host.rs`; `/compact` asks its host to compact it, `compact_host.rs`; `!` commands run on its host when this terminal is local to it, a remote terminal gets the gray `-32018` hint instead, and every `user_shell` event, live or replayed, shows as a `Ran` command cell with its output, `shell_host.rs`; its `/context` report and footer usage come from the host, `context_host.rs`) |
| `bottom_pane/` | Composer, footer, popups, approval overlay |
| `presentation/` | Private projection of ACP updates into display items |
| `history_cell/`, `exec_cell/`, `client_tool_cell.rs`, `markdown*`, `diff_render.rs` | History cells and their rendering |
| `host_client/` | Client of a `nori host`: connection, reconnect, reply matching; `drive` (WebSocket) and `drive_pipe` (owned host); `HostEndpoint` keeps the local token and presents it on every reconnect; `HostError` carries the JSON-RPC `code` |
| `attach/` | `run_attach`, `AttachOptions`: the `nori attach` frontend |
| `program_status/` | OSC 7501 terminal status: `ProgramStatusTracker` (fold over one session's events) and `ProgramStatusReporter` (encode and write) |
| `nori/` | Nori pickers (agent, config, fork, resume, skillset, hotkey, MCP), onboarding, session header, update prompt |
| `resume_picker/`, `session_*.rs`, `viewonly_transcript.rs` | Resume picker, session preview and stats, view-only transcripts |
| `tui.rs`, `custom_terminal.rs`, `insert_history.rs`, `render/` | Terminal setup, inline viewport, scrollback insertion |
| `style.rs`, `color.rs`, `terminal_palette.rs`, `shimmer.rs` | Styles and terminal colors |
| `storybook` (feature `storybook`) | Production view specimens for `examples/` |

```text
nori / nori-tui                          nori attach
      |                                       |
 run_main                                 run_attach
      |  nori_host_runtime::launch            |  ManagedClient (WebSocket, token)
      v                                       v
 App::run --HostLink--> OwnedHost         host_client::drive --> remote nori host
   |  manager  -> ChatWidgets (in-process features)
   |  pipe     -> host_client::drive_pipe (controller)
   v
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
- Only `run_main` holds an `OwnedHost`. The attach frontend never holds the
  in-process half and never ends a session when its terminal closes.
- The App sends `initialize` on the owned pipe before it reads UI events. A
  failed initialize or a stopped pipe ends the loop with an error. The App
  stops the host after the loop, even on error.
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
- `ProgramStatusTracker` is a pure fold over one session's `SessionEvent`s.
  It settles a turn from the prompt's own response, matched by request ID.
  Every `ChatWidget`, parked ones included, folds its events, so a session
  brought forward already knows its state. `App` reports only `chat_widget`'s
  status after each main-loop event; background sessions never write.
  `nori attach` reports the selected `Conversation` once its event channel is
  drained, so a replay burst writes one report.
- A host operation's error reaches its widget as a typed `HostError`, so a
  widget can branch on the JSON-RPC code. `shell_host.rs` does: `-32018`
  shows `history_cell::new_hint_event` (dark gray) rather than a red error.
  Either way the refused `!cmd` goes back to the prompt.
- `ProgramStatusReporter` writes only when the bytes change, never when
  stdout is not a TTY, and clears once if it ever reported.
- During catch-up, the attach event loop applies queued host events in
  arrival order and draws once per bounded batch, handling keys and list
  refreshes between batches. The attach renderer formats transcript parts
  from newest to oldest only until they cover the scroll offset and the
  visible rows, then shows those rows in reading order. Scrolling past the
  start keeps the first page on screen. An idle conversation with no
  transcript text shows the ready prompt.
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

## Tests

| What | Where | Run |
|---|---|---|
| Unit and render tests, `insta` snapshots | `#[cfg(test)]` modules, `src/**/snapshots/` | `cargo test -p nori-tui` |
| Attach frontend | `src/attach/tests*.rs` | `cargo test -p nori-tui attach` |
| vt100 emulator suite | `tests/all.rs`, `tests/suite/` | `cargo test -p nori-tui --features vt100-tests` |
| Real binary in a PTY | `nori-rs/tui-pty-e2e` | `just test e2e` (repository root) |
| Visual specimens | `examples/` | `cargo run -p nori-tui --features storybook --example context_storybook` |

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
