# Noridoc: tui-pty-e2e

Path: @/nori-rs/tui-pty-e2e

### Overview

`tui-pty-e2e` drives the real `nori` binary inside a pseudo-terminal and asserts
on the parsed terminal screen. It is the final regression boundary for behavior
that unit tests cannot prove across process, harness, event, and rendering
layers.

### How it fits into the larger codebase

Tests launch `nori` by default. The [foreground launcher test](tests/prepared_startup.rs)
selects each built executable in turn and checks the rendered response, inline
screen, adapter process, and quit status. Build both binaries from `nori-rs/`
with `cargo build --bin nori --bin nori-tui` before running the suite.

```text
PTY -> nori or nori-tui -> foreground host -> nori-harness -> nori-acp-host -> mock ACP agent
```

### Core Implementation

`portable_pty` supplies the terminal and the crate's
[`Emulator`](src/emulator.rs) maintains the virtual screen. Test helpers send
keys, type and submit composer input, wait for stable text, capture output, and
enforce exit deadlines.

At startup the synthetic terminal reports a cursor at row 1, column 1.
Its primary device attributes select legacy keyboard input, and its palette
replies report white text on black. The responder recognizes queries across
PTY reads, including a read that ends halfway through an escape sequence.

A timed-out wait reports the child PID and status, elapsed wait and launch
times, poll count, total PTY bytes, and when the fixture first read output.
Text waits include the expected text. On panic, the fixture prints the final
screen, the last 512 bytes of combined stdout and stderr with control bytes
escaped, and the ACP log tail when available. The launched `nori` logs
`nori_host_runtime` at debug level, so that log also records every ACP request
the owned host receives.

The `Emulator` wraps a `vt100` parser with `SCROLLBACK_ROWS` lines of
history. `vt100` ignores `ESC[3J`, which the TUI sends whenever it reprints a
session, so the emulator applies it itself: the visible screen stays and the
history is dropped. It holds back a trailing partial `ESC[3J` so a sequence
split across PTY reads still applies. On top of it, `TuiSession` offers:

| Helper | Purpose |
| --- | --- |
| `history_contents()` | Every scrollback line, oldest first, then the visible screen |
| `raw_output()` | Every byte the TUI wrote, including sequences the emulator drops, such as notifications |
| `resize(rows, cols)` | Resizes the PTY and emulator, as a user dragging the window would |
| `settle(duration)` | Keeps reading output for a fixed time so the emulator catches up |
| `wait_out_answer_grace()` | Settles for `ANSWER_GRACE`, longer than the TUI's request answer grace period, so the next option key answers a request that just appeared |

By default each `TuiSession` gets a disposable home and generated
`config.toml`. `SessionConfig::with_nori_home` instead points `NORI_HOME` and
`CODEX_HOME` at a caller-owned directory that can be reused across launches;
the harness neither writes nor deletes its `config.toml`, so a test can observe
what one launch saved and how the next launch reads it. `with_cwd` starts Nori
in a chosen directory.

`Key` adds `Alt(char)` (ESC prefix), `AltDown`, `BackTab`, and the terminal
focus reports `FocusIn` and `FocusOut`, used by session hotkeys and
notification tests.

Scenarios cover representative raw ACP messages, tools, plans, permissions and
responses; Nori lifecycle and failure behavior; query-driven pickers; cloud
list/resume/close/detach; transcript persistence and view-only selection; MCP and browser workflows; and ordering races
between streaming text and tool updates. Agent-switch coverage observes real
child PIDs to prove that the current and prepared candidate coexist, activation
reuses the prepared child, and cancellation or activation failure leaves the
current session usable. [Host agent-switch scenarios](tests/host_agent_switch.rs)
drive `/agent`, and [host session-picker scenarios](tests/host_session_picker.rs)
drive `/resume` and the picker after the last `/close`, and
[host new-session scenarios](tests/host_new_session.rs) drive `/new`,
[host file-mention scenarios](tests/host_file_mentions.rs) check that `@`
lists the files of the session on screen across Alt+, / Alt+. switches and
that `/new DIR` starts there (the fixture traces into its working folder) or
refuses a missing folder, and that Tab completes a `/new` folder from the
session on screen as it changes, and
[host startup scenarios](tests/host_startup.rs) drive plain startup, through
the owned host. They share [`support/two_agents.rs`](tests/support/two_agents.rs), which launches
plain `nori` (optionally with arguments, and with or without a first prompt)
with two synthetic adapters (Amber, the default, and Birch) on the
[standard ACP fixture](../cli/tests/standard_acp/fixture.py), whose per-agent
`fixture-traffic-<slug>` gate files trace each child's PID and methods, and
whose `fixture-hold-init` gate writes `fixture-init-held` and blocks
`initialize` until the gate is removed. The support module also reads the
host's request log, so a test can wait for a host method and check its order
(for example that `_nori/host/prepare_agent` precedes `session/new`). They
check that a saved session is restored on a freshly prepared server, and that
dismissing the picker or a refused restore (`fixture-refuse-load`) keeps the
current session, that `/new` starts on a second freshly prepared child while
the first keeps running, and that a refused `/new` (`fixture-refuse-new`)
keeps the current session in front. Startup scenarios check that the first
session is prepared and started by the host on one server, that a prompt typed
while `initialize` is held or a positional prompt reaches it exactly once, that
a refused startup session is not retried until the next prompt (which also
goes through the host), and that `/resume` during preparation restores a saved
session on the startup server with no `session/new`.

[Context scenarios](tests/context.rs) drive `/context` before, between, and
during actual prompts using the mock agent's
[usage fixtures](../mock-acp-agent/src/context_usage.rs). They assert reported
usage, alternating grid segments, checkpoint boundaries, and unchanged turn
counts. Each scenario waits for the panel's final row before asserting, because
the panel arrives as one history insert that macOS PTYs deliver in ~1 KB reads;
waiting for an early row returns before the rows below it are parsed. A
release-file barrier keeps a prompt active while the command runs,
then proves that the original prompt still completes. This covers the local
query and asynchronous rendering path without replacing Harness state.
The [instruction inventory scenario](tests/context_instructions.rs) opts into
real instruction discovery in an isolated home, warms tokenizer initialization,
then edits a file and verifies repeated `/context` commands display fresh
counts within the interactive latency budget. Unchanged files retain their
counts; the test does not substitute a prebuilt inventory.

Authenticated analytics scenarios pair the real PTY workflow with a local HTTP
sink. They prove interactive and cloud launches remain silent while merely
connected, submit the first user prompt through the real ACP agent, observe the
exact public `session_mode`, and verify a later prompt on the same logical
session does not create another activity.

[Host approvals scenarios](tests/host_approvals.rs) drive `/approvals` in plain
`nori`: the footer follows the session on screen and `/new` starts in the host
default rather than the last choice. [Preset migration
scenarios](tests/approval_presets_migration.rs) launch on a home config holding
a retired `/approvals` preset and check that the first launch shows the notice
and writes `[host] default_mode`, that sessions on every launch start in the
migrated mode, and that a config without presets is never written.

The [host attachment scenarios](tests/host_attach.rs) run a detached host and
independent attached PTYs sharing a disposable Nori home. They cover the flat
script-facing list, session creation (including `/new` in the attaching
terminal's folder or a Tab-completed one), `@` mentions and `/new` Tab
completion answered from the session's folder on the host, a `--host` attach
whose host and terminal have different `HOME`s naming and completing `~` and
relative folders from the host's home and folder, controller/watcher handoff, refused
watcher prompts and permission answers, greyed `/` menu rows refused with
the menu's reason, `/approvals` changing a session's mode from the controller
and showing it in every terminal, detach/reattach, and host survival
after its last session closes. A local TCP proxy either drops one terminal's
connection or silently stops forwarding traffic while the controller continues,
then checks the reconnecting marker and replay of missed output without
duplicates. Cleanup owns both clients and the host so a failed assertion does
not leave background agents running.

Prepared-startup scenarios (startup now prepares through the owned host) wrap `mock-acp-agent` behind the exact registered
`nori-handroll acp --type remote` argv while recording process identity. They
prove that ordinary and remote-adapter startup initialize, optionally list, and
then activate exactly one session on their own without any input, reusing the
single prepared child; that `/new`, `/resume`, positional input, and a first
typed prompt reuse that same child; and that deferred text and images cross the
configured session boundary exactly once. Local slash and shell commands still
render or report their status without spawning a second agent, and preparation
timeout and exit cover the reaping paths. Plain-local coverage in `startup.rs`
additionally proves that a session activates without input (the resolved model
name appears), that `/model` lists the agent's advertised models before any
prompt, and that per-session skillsets auto-activate once a skillset is applied.

`startup_draft.rs` gates agent preparation on a file, renders the first draft
character, releases preparation, and types the remaining text after activation.
This deterministically checks cursor preservation across the widget handoff and
then verifies that Ctrl-C clears the correctly ordered draft.

Cloud activation scenarios hold `session/new` behind a release-file barrier
after the picker selection. They verify that connecting feedback remains in
history, typing stays available, Enter preserves rather than queues the draft
before `SessionStarted`, and a later retry crosses ACP exactly once. A
structured new-session failure also proves that the terminal shows the ACP
message and string `data.detail` while excluding unrelated diagnostic fields.

Cloud startup scenarios enable the local onboarding/trust, automatic-worktree,
and per-session-skillset policies independently. Each must still reach the
Handroll session picker without showing a local gate, creating a local
worktree, or delaying agent preparation.

Multi-session scenarios need no configuration (several sessions per TUI is
always on) and run each session on its own mock agent process, using the
mock's per-prompt markers so sessions behave differently on one binary.
[`multi_session.rs`](tests/multi_session.rs) asserts on full history, not only
the screen: a session move must reprint only the target's history and keep
unsent input, `/new` keeps the previous session running, a burst of moves
lands on the last target, and configured bindings are honored. [`session_list.rs`](tests/session_list.rs) covers
the list's status words, agent-sent titles, reply and context rows, the
single status summary while the list is open, filter, and jumps, a background request
answered through `next_waiting_session`, a dead agent failing only its own
session and resuming in place, forks listed under their parent, and
notifications gated on terminal focus. [`session_transcripts.rs`](tests/session_transcripts.rs)
covers the always-on fix that `/new` then `/resume` shows each turn once,
including after a resize, and a cloud scenario lists two cloud sessions side
by side. [`session_lifecycle.rs`](tests/session_lifecycle.rs) covers the
`[tui] max_sessions` refusal across new, resume, and new-process fork paths,
closing from the list with Ctrl+D (including the
second press a busy session needs) and from `/close`, starting a session beside
a busy one, a background loop staying in its own row, a restarted session
keeping its title and conversation ID through manager resume, `/agent`
preparation leaving no line in the old session, and the background-request line
fitting one row. A cloud-mode
scenario checks that `/close` beside another open session lists the released one
as closed. The ignored `jump_timing_on_a_long_history` test reports how long a
jump to a long history takes; run it with `--ignored --nocapture`.

[Tool output preference scenarios](tests/tool_output_preference.rs) use a
shared home and the mock agent's `MOCK_AGENT_SEND_COMMAND_OUTPUT` command to
check that inline tool output is full by default, that the post-trust compact
question saves the chosen value only for that project (and applies in its
subdirectories), that Esc saves nothing and keeps inheriting the global
setting, that a one-off `-c` override still shows the question without being
saved, and that an unrelated `-c tui={...}` table keeps the project's choice.
Line cap scenarios set a global `[tui] tool_output_max_lines` and a smaller
project value, then check that the project cap keeps the head and tail of the
output with a `┄ N lines hidden` row, and that `-c
tui.tool_output_max_lines=0` lifts the project cap.

[Queue recall scenarios](tests/queue_recall.rs) press Tab to queue a follow-up
during a running turn, then Up+Enter (focus the queue list, edit the selection)
to move it back into the composer. After an edit, Tab appends the
revised text to the queue and Enter steers the active turn. The mock agent's
prompt log checks that the withdrawn text never runs as a separate prompt.
Coverage includes identical messages, Down from the newest selected message
returning to the composer, and Alt+Up at the root session leaving the queue, an empty composer,
and a typed draft untouched.

[Queue pause scenarios](tests/queue_pause.rs) queue follow-ups behind a slow
turn and assert on the mock agent's prompt log. They cover editing a selected
duplicate (delivered once), removing a selected message (never delivered), and
a queue held after an interrupt or after a turn that fails via
`mock:prompt-fail-after-delay`, which sends nothing until Enter on an empty
composer resumes it and then delivers each held prompt once, in order.
Two scenarios select the head of two identical queued rows and wait for it
to start through the asynchronous queue update, then press x or Enter; the
prompt log (`MOCK_AGENT_PROMPT_LOG`) shows every message delivered once, proving the key did not
remove or edit the identical neighbour.

The protocol hard cut did not introduce a test-only compatibility path. PTY
tests exercise source-first `SessionEvent::{Acp, Nori}` dispatch and the same
typed `HarnessHandle` methods available to headless embedders.

### Things to Know

- Build the mock agent before local runs when CI has not supplied its path.
- The crate uses its own `vt100`-based emulator and does not require the TUI
  `vt100-tests` feature; build the real `nori` binary before running it.
- Assertions about reprinting must read `history_contents()`. The screen alone
  cannot show a turn printed twice once the duplicate has scrolled off.
- Snapshot normalization removes dynamic session IDs, timestamps, status
  text, and disposable temp roots: `/tmp` on Linux plus `/private/tmp`,
  `/var/folders`, and canonicalized `/private/var/folders` on macOS. The
  session lists' last-activity times (`12s ago`, `3m ago`) all become
  `N ago`, so snapshots do not depend on how long a test took. Meaningful terminal ordering and
  content remain intact.
- Transcript pickers treat a v3 transcript as non-empty only when it has at
  least one user turn; lifecycle records alone do not make it resumable.
- Composer input goes through `TuiSession::type_input` and
  `TuiSession::submit_input`, never a bare `send_str` followed by a sleep. A
  string written in one PTY burst is classified as a paste, and Enter is folded
  into that buffer as a newline until the buffer flushes, so the helpers wait
  for the composer to render the text before sending the next key. That
  render check ignores slash-popup rows: a leading `/` or `!` counts only
  when it renders as the composer's mode sigil followed by whitespace
  (`/ context`), because the popup lists `/context` as soon as it opens on a
  prefix, before the rest of the input has reached the composer.
- Timing-sensitive scenarios use bounded waits, but assertions target visible
  behavior rather than private reducer calls.
- Prepared-lifecycle scenarios assert process ownership as well as terminal
  output because a correct-looking picker can otherwise hide a disposable
  inspection subprocess. Shutdown checks require the direct adapter child to be
  reaped and its previously running descendants to stop. Descendant checks use
  process state: an orphan zombie awaits its new parent's reap and does not
  count as a running subprocess.
- Prompt-fidelity assertions use mock-agent validation rather than rendered
  output, so a duplicated or transformed deferred payload fails at the ACP
  boundary.
- Analytics assertions use an explicit `NORI_ANALYTICS_URL` and disposable
  Firebase-shaped user configuration. They never contact the production ingress
  or PostHog.

Created and maintained by Nori.
