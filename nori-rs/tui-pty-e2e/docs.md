# Noridoc: tui-pty-e2e

Path: @/nori-rs/tui-pty-e2e

### Overview

`tui-pty-e2e` drives the real `nori` binary inside a pseudo-terminal and asserts
on the parsed terminal screen. It is the final regression boundary for behavior
that unit tests cannot prove across process, harness, event, and rendering
layers.

### How it fits into the larger codebase

Tests launch `nori` with `mock-acp-agent`, send keyboard input, and observe
screen output and process lifecycle. The path under test is:

```text
PTY -> nori-tui -> nori-harness -> nori-acp-host -> mock ACP agent
```

### Core Implementation

`portable_pty` supplies the terminal and the crate's
[`Emulator`](src/emulator.rs) maintains the virtual screen. Test helpers send
keys, type and submit composer input, wait for stable text, capture output, and
enforce exit deadlines.

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

`Key` adds `Alt(char)` (ESC prefix), `AltDown`, `BackTab`, and the terminal
focus reports `FocusIn` and `FocusOut`, used by session hotkeys and
notification tests.

Scenarios cover representative raw ACP messages, tools, plans, permissions and
responses; Nori lifecycle and failure behavior; query-driven pickers; cloud
list/resume/close/detach; transcript persistence and view-only selection; MCP
and browser workflows; runtime remote-control ownership; and ordering races
between streaming text and tool updates. Agent-switch coverage observes real
child PIDs to prove that the current and prepared candidate coexist, activation
reuses the prepared child, and cancellation or activation failure leaves the
current session usable.

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

The remote-control workflow drives startup and runtime listener management
through a real WebSocket ACP client, then switches agents. It proves both entry
paths share one listener owner, disabling listeners preserves the harness, and
the stable endpoint exposes a replacement only after its `SessionStarted`
commit.

Prepared-startup scenarios wrap `mock-acp-agent` behind the exact registered
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

Multi-session scenarios enable `[features] multi_session` through extra
config TOML and run each session on its own mock agent process, using the
mock's per-prompt markers so sessions behave differently on one binary.
[`multi_session.rs`](tests/multi_session.rs) asserts on full history, not only
the screen: a session move must reprint only the target's history and keep
unsent input, `/new` keeps the previous session running, a burst of moves
lands on the last target, configured bindings are honored, and nothing
changes without the flag. [`session_list.rs`](tests/session_list.rs) covers
the list's status words, previews, filter, and jumps, a background request
answered through `next_waiting_session`, a dead agent failing only its own
session and resuming in place, forks listed under their parent, and
notifications gated on terminal focus. [`session_transcripts.rs`](tests/session_transcripts.rs)
covers the always-on fix that `/new` then `/resume` shows each turn once,
including after a resize, and a cloud scenario lists two cloud sessions side
by side. The ignored `jump_timing_on_a_long_history` test reports how long a
jump to a long history takes; run it with `--ignored --nocapture`.

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
  `/var/folders`, and canonicalized `/private/var/folders` on macOS. Meaningful
  terminal ordering and content remain intact.
- Transcript pickers treat a v3 transcript as non-empty only when it has at
  least one user turn; lifecycle records alone do not make it resumable.
- Composer input goes through `TuiSession::type_input` and
  `TuiSession::submit_input`, never a bare `send_str` followed by a sleep. A
  string written in one PTY burst is classified as a paste, and Enter is folded
  into that buffer as a newline until the buffer flushes, so the helpers wait
  for the composer to render the text before sending the next key.
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
