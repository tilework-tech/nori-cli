# Noridoc: nori-tui

Path: @/nori-rs/tui

### Overview

`nori-tui` is the Ratatui frontend for the headless Nori harness. It owns input,
rendering, presentation-only state, pickers, approvals, and terminal lifecycle.
It does not own ACP transport or expose a second protocol vocabulary.

### How it fits into the larger codebase

```text
nori-tui
   |  typed HarnessHandle methods
   |  SessionEvent::{Acp, Nori}
   v
nori-harness -> nori-acp-host -> ACP agent
   ^
   |
nori-config
```

The TUI depends on `nori-harness`, `nori-config`, and `nori-protocol`; it does
not import the ACP host or ACP schema crate directly. Its ACP types arrive
through `nori_protocol::acp`.

### Core Implementation

#### Source-first event dispatch

The application event loop matches `SessionEvent::Acp` and
`SessionEvent::Nori` before projecting either branch into UI state.

- ACP notifications drive private message, thought, plan, tool, usage, mode,
  live config, capability, and available-command presentation.
- ACP requests drive the permission overlay and retain their raw `RequestId`.
- Initialize responses and prompt responses matching the active request update
  UI lifecycle. For prompt errors, the correlated `NoriEvent::RequestFailed`
  drives completion, loop disposition, and user-visible display; the matching
  raw ACP error remains observable but is not rendered or completed a second
  time. Failures unrelated to the active prompt do not complete it. Other typed
  operations complete through their
  `HarnessHandle` return values while their raw responses remain observable on
  the stream.
- Nori events drive lifecycle, queue, replay, compaction, goals, undo,
  user-shell output, hooks, summaries, notices, and classified failures.

The private modules under `tui/src/presentation/` assemble ACP streaming values
into display cells and friendly labels. These view models are allowed to be
lossy and UI-specific; they are not fed back into the harness or exported from
`nori-protocol`.

#### Commands and approvals

User actions call typed `HarnessHandle` methods for prompting, cancellation,
history, prompt discovery, compaction, undo, shell, goals, session config,
session listing, close, and shutdown. The old generic operation bus is gone.

When an ACP permission request arrives, the overlay renders the ACP subject and
options, then calls `respond_to_agent(request_id, response)` with the exact ID
from the request. Filesystem requests do not enter this path because the host
handles them. Terminal and extension request families are not advertised by
the current host.

#### Lifecycle behavior

An orderly ACP close completes the typed close call, leaves the raw close
response observable on the stream, observes `SessionEnded(Closed)`, and then
handles stream closure. The TUI does not render a successful close-response
message. Explicit application shutdown uses `SessionEnded(Shutdown)`.

Events entering the application are tagged with their session generation. When
a session is replaced, events from older generations are discarded.
Replacement shutdown is based on the live harness handle rather than transcript
recorder or conversation-ID availability.

Unexpected child or transport loss emits a request failure when work was in
flight followed by `SessionEnded(ConnectionLost)`. The TUI stays open so the
user can read the failure and choose the next action; connection loss is not
treated as a successful quit.

Cloud sessions use standard ACP `session/list`, `session/resume`, and
`session/close`. Quitting detaches through connection teardown; `/close` is the
explicit agent-side release action.

#### Footer configuration

`[tui.footer_layout]` accepts the same layout entries in `footer_left`,
`footer_right`, and all four `textarea_*` corners. An entry is either a built-in
segment name or an inline custom chunk:

```toml
[tui.footer_layout]
footer_left = [
  "git_branch",
  { format = "{context_used_percent} / {context_window_tokens}" },
  "approval_mode",
]
```

Built-in names are `prompt_summary`, `vim_mode`, `git_branch`,
`worktree_name`, `git_stats`, `context`, `context_used_percent`,
`context_remaining_percent`, `context_used_tokens`,
`context_remaining_tokens`, `context_window_tokens`, `approval_mode`,
`skillset`, `nori_version`, `token_usage`, `mode_indicator`, and
`cloud_session`. The default `context` segment renders used percentage and
maximum window size, such as `44% / 272k`. The five atomic context segments are
off as standalone entries by default so custom chunks can compose only the
values they need.

Custom formats are deliberately limited to literal text and built-in
placeholders. `{{` and `}}` emit literal braces. Unknown placeholders,
unbalanced braces, expressions, conditions, and format specifiers are rejected
when configuration loads. Referenced segments keep their styles and ignore
their standalone `[tui.footer_segments]` toggle; if any referenced runtime value
is unavailable, the complete custom chunk is hidden.

`FooterLayoutItem` and the load-time parser live in
`@/nori-rs/nori-config/src/types/mod.rs`. Composition and context formatting
live in `@/nori-rs/tui/src/bottom_pane/footer.rs`; resolved ACP or transcript
usage reaches it through
`@/nori-rs/tui/src/bottom_pane/chat_composer/rendering.rs`.

#### Transcripts and view-only mode

Between `ReplayStarted` and `ReplayFinished`, replayed user and assistant
messages are assembled in event order and rendered as static conversation
history with turn boundaries. They are not handled as live output streams.

Transcript schema v3 contains lifecycle events even before
a prompt, so session pickers determine whether a transcript is empty by its
exact user-turn count rather than total record count. The loader retains private
v2 compatibility.

Goodbye-card statistics are accumulated from the TUI's private projection of
raw ACP updates and from Nori lifecycle state. They are presentation data, not
part of the public harness protocol.

### Things to Know

- Never add a public normalized ACP enum to make rendering convenient. Keep
  display inference private in `presentation/`.
- Query results arrive as typed method returns; do not add correlated Nori
  response events.
- The PTY suite in `@/nori-rs/tui-pty-e2e/` is the regression boundary for real
  terminal behavior, event ordering, approvals, lifecycle, and transcript
  selection.
- `nori-config` is the source of approval and sandbox policy; ACP session config
  options remain ACP schema values.

Created and maintained by Nori.
