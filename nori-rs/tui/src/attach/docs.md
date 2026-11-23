# Noridoc: Attached host frontend

Path: @/nori-rs/tui/src/attach

### Overview

- Renders a terminal attached to a background host without owning its sessions
  or agent processes.
- Keeps conversation presentation, session selection, connection role, and
  replay cursors local to this terminal.

### How it fits into the larger codebase

- [CLI host dispatch](../../../cli/src/host/docs.md) supplies a resolved endpoint and token, plus the attaching terminal's
  configuration, whose `cwd` (the attaching directory, or `-C`) is where
  `/new` starts sessions. This frontend is separate from the
  local TUI `App`, whose shutdown owns its local sessions.
- The harness re-exports [ManagedClient](../../../acp-host/src/remote/client.rs)
  to preserve the frontend dependency boundary. The host retains mutation
  policy and broadcasts complete manager envelopes to controllers and watchers.
- Presentation reuses the TUI's [normalizer](../presentation), composer,
  Markdown, tool cells, and picker components. The
  [attachment composer](../public_widgets/composer_input.rs) submits slash
  commands literally for this frontend to interpret; the local TUI keeps its
  ordinary command handling. A terminal's selected session does not change
  another terminal's selection.

The controller uses the [host follow-up operation](../../../cli/src/host/docs.md#mid-turn-follow-ups) for a composed message during a working turn. Enter steers and Tab queues. Session events supply the expected turn ID. A pending permission dialog continues to handle its own keys and idle Enter submits an ordinary prompt.

### Core Implementation

- [Connection driving](connection.rs) separates socket ownership and reconnect
  attempts from rendering. Received JSON messages renew a liveness deadline;
  periodic session-list replies keep an idle connection alive. Reads and sends
  share that deadline, so silent traffic loss also produces the reconnecting
  state. Recovery authenticates again and compares the upgrade's host-instance
  UUID with the initial connection before initializing or resuming replay.
  Authentication rejection permits a secure local-token reread and one retry
  through the callback supplied by CLI dispatch; explicit endpoints have no
  such callback.
- [Runtime correlation](runtime.rs) tracks request IDs while processing every
  notification. Initialization supplies the role, refreshes the session list,
  and subscribes to the selected conversation using its applied cursor.
  Switching sessions replaces the subscription; listing remains independent of
  envelope delivery. Subscription errors apply only while their conversation
  remains selected. Role-change notices update both the persistent status line
  and conversation feedback.
- [AttachState](state.rs) routes envelopes by conversation identity. It applies
  ACP presentation and Nori lifecycle events before advancing each cursor,
  ignores already-applied envelopes, and rejects sequence gaps. Transcript
  restoration's `ReplayStarted` replaces presentation and permissions while
  retaining transport cursors; reconnect replay only deduplicates by sequence.
  Permissions also clear on matching `RequestResolved`, idle or cancelling
  phases, and session end.
- [Rendering](render.rs) keeps role and connectivity visible beside the
  selected conversation or session list. Watchers receive the same content and
  permissions but must take control before sending mutations.
- The identity line under the header shows the selected session as
  `<agent> · <name> · <folder> · <status>` (the folder is
  `session_table::short_folder` of the row's `cwd`, omitted when an older host
  sends none), where the name is the title's whole first
  line through `nori_protocol::session_full_name`, else
  `nori_protocol::UNTITLED_SESSION_NAME`. A top-level session shows its
  `family_status` when present, as in the session lists. Inside a delegated
  child (a row whose `source` is `session`; forks get no breadcrumb) whose
  parent is listed, it becomes the full-path breadcrumb from the top-level
  session with its own status and the key ` · <parent_session key> parent`
  (Alt+Up by default). Both forms go through the shared
  [`session_path::breadcrumb_line`](../session_path.rs) (a top-level session
  is a one-step path without a key), so long names are fitted to the width:
  the key gives way before the current name is cut, and names are cut in the
  middle. The walk follows listed delegating parents and stops at a repeat.
- A waiting line (the row under the identity line) names the next session,
  other than the selected one, whose own status is `waiting on you`: under its
  parent when the parent is listed, with `(+N more)` and the jump key
  (`AttachState.jump_binding`, set from the configured `next_waiting_session`
  binding). It uses the shared `session_path::waiting_line` without request
  text, passing whole names that it cuts in the middle by width, and the row
  stays empty when nothing waits.
- The session list uses the shared two-row session layout from
  [`session_table`](../../../tui-components/src/session_table.rs), the same as
  the local session list and `nori host ls` on a terminal (see the
  [tui-components docs](../../../tui-components/docs.md)). Row 1 holds the
  status glyph and title (Session), agent, status word, and a header-less
  context column; row 2 holds the session's short folder, ` · `, and the start
  of the agent's last reply (faint, `no reply yet` when absent; the folder
  alone when the reply is empty; see `session_table::folder_and_reply`), the model under Agent, and the time since the
  last activity under Status. A row whose own status is `waiting_on_you`
  shows what the session asks for (`waiting_request`, such as `approve:
  cargo test -p parser`) in place of the reply. These come from the additive
  `_nori/session/list` fields `reply_preview`, `waiting_request`, `model`,
  `idle_seconds`, `context_used`, and `context_size`; older hosts omit them
  and the cells stay empty (a missing `waiting_request` falls back to the
  reply). There is no ID column. The list header reads `Sessions N · <status
  summary>` through the shared `session_status::status_summary`, like the
  local list's header. The picker is built with `horizontal_margin(0)` so widths follow
  the terminal and `selection_keeps_tones(true)` so the highlighted row keeps
  its status colours; the list has no fullscreen selection rails.
- The list is tree-ordered: [AttachState](state.rs) runs the listed
  `(session_id, parent)` pairs through the shared picker `tree_order` and gives
  each item its depth, and the picker's tree column is the Session column.
  Tree guides (and their continuation on row 2) and the `(current)` badge
  therefore sit inside Session while the other columns stay aligned at every
  depth. Status words take the shared `session_status::status_tone` colors
  (waiting on you amber, working cyan, ready green, failed red, done in the
  default color; starting and closed muted). A row at depth 0 shows the host's
  `family_status` when present, so a parent reads `waiting on you` while
  anything under it does; older hosts omit the field and the row shows its own
  status. A row without a title reads `UNTITLED_SESSION_NAME`. Titles are cut
  once in the middle, keeping the ends that tell sibling sessions apart. The
  list ignores the host's `depth` field and derives depth from parent links.
- List keys come from the shared
  [`picker_keys::navigation_action`](../bottom_pane/picker_keys.rs), the same
  mapping as the local session list and other pickers: arrows and `j`/`k`
  move, PageUp/PageDown/Home/End page, Enter opens, `/`, `f`, or Ctrl+F start
  a search, typing filters only while searching, Backspace edits the query,
  and Esc leaves the search. The list shares the screen with the composer, so
  while the composer holds text (and no search is active) only the arrows,
  PageUp/PageDown, and Ctrl-modified keys act on the list; every other key edits the draft. The
  footer reads `↑↓/j/k move · enter open · ctrl+r resume · / search · esc
  back`.

- Delegation uses the shared [command parser and presentation](../delegation.rs):
  `/delegate <agent> <task>` sends the selected parent to the manager's remote
  delegate method. A different-kind request shows the shared `approval_lines`
  prompt (question, `asked by` / `runs on` / `task` rows) with
  `1 Yes, start it · 2 No, decline`; watchers must take control before either
  operation. Session lists carry pending delegation state so reconnecting
  terminals can recover the approval view without resending a mutation.
- Every other `DelegationChanged` becomes a `Part::Delegation` in the parent's
  conversation, rendered by the shared `DelegationCell`. Attached clients have
  no transcript view or local session-list key here, so the cell is built with
  empty `DelegationHints` and shows no key hints. An `awaiting_approval` change
  adds no part; the approval prompt covers it.
- A permission prompt or delegation approval on the on-screen session
  (`AttachState::request_on_screen`) is modal while the session list is
  closed, like the local approval overlay. The composer and its cursor are
  hidden until the request is answered; the draft is kept untouched. Request
  keys answer it:

  | Key | Permission | Delegation |
  | --- | --- | --- |
  | Up/Down, plain Enter, 1-9 | Up/Down move the choice; Enter answers it; a digit answers that option (out-of-range digits do nothing) | 1 or plain Enter approves, 2 declines |
  | Esc | `reject_once` when offered, else `reject_always`, else `cancelled` (shared `answer_guard::esc_reject_option`) | declines |
  | Ctrl+C (once) | outcome `cancelled` | declines |

  PageUp/PageDown still scroll; every other key is swallowed. The hint rows
  read `↑↓ choose · enter answer · number selects · esc reject · ctrl+c
  cancel` and `· esc or ctrl+c declines`. Earlier hotkeys and pastes are
  unaffected; off-screen requests capture nothing.
- Option keys (digits, Enter, and plain letters other than `j`/`k`) are
  dropped until a request has been on screen for the shared answer grace
  period ([`answer_guard`](../answer_guard.rs), `REQUEST_ANSWER_GRACE`), so a
  key typed into the composer just as a request appears cannot answer it.
  Esc, Ctrl+C, arrows, and `j`/`k` always act. The [Runtime](runtime.rs)
  records the on-screen request identity (`AttachState::request_on_screen_id`,
  the session plus its awaiting delegation, else its first permission, the
  order in which request keys answer them; `None` while the list is open)
  after each host event (even one that fails to apply), session switch, and
  key, and restarts
  the grace whenever that identity changes. `Runtime::with_clock` injects the
  time source so tests step time instead of sleeping.
- Child permissions stay in the child's conversation. The configured
  `next_waiting_session` binding (Alt+Down by default) navigates there, even
  with a draft in the composer, as in the local TUI. With nothing waiting in
  the cached list, a draft lets the key pass to the composer; an empty
  composer on a connected terminal retains the navigation action while
  refreshing the list, then selects a newly discovered waiting session from
  the response. Later input or navigation, list errors, and disconnection
  cancel that pending action, and the response jumps only if the composer is
  still empty, including after paste.
- Unsent drafts are kept per session. Every switch (next waiting, parent,
  picker Enter, and the session `/new` creates) goes through
  `Runtime::open_session`, which parks the whole composer edit (text, large
  pastes, images, cursor; the composer's own draft hand-off) with the session
  it was typed in and restores the target's own draft, so text never follows
  the user into another session. Text typed while no session is open goes
  with the session opened next. Drafts live only in this terminal's memory.
  Parent notices name the child and parent readably (see the
  [harness](../../../harness/docs.md#delegation-between-managed-sessions)).
  The `parent_session` binding moves the selection from a delegated child
  (`source` `session`, not a fork) to its parent when that parent is in the
  cached list, closing the list and resubscribing; elsewhere it does nothing.
  Results return through the parent's ordinary event stream without a terminal
  review step. The echoed result prompt (a user message that
  `nori_protocol::delegation_result_parts` recognizes) renders as one
  `› delegation result from <agent> · <name> · <status> · delivered to this
  session` line through the shared `delegation::delivered_result_spans`
  (the delivered suffix gives way first, then the name is cut in the middle),
  since the result block already shows the text.
- A session end is stored as `Part::Ended` with the host's message, and its
  line is worded at draw time from the row's current status, because the host
  may report `done` after the end event. A host message is shown as sent.
  Otherwise a delegated child reads `This child finished its task · <parent
  key> to its parent` when done and `This child session ended · …` otherwise
  (just a period when the key is unbound); a top-level session reads
  `Session ended. Use /new to start another session.`

- [`/new [DIR] [AGENT]`](new_session.rs) starts a host session, and like
  other mutations requires control in this terminal. Plain `/new` starts in
  the attaching terminal's directory, not the host's; on a host named with
  `--host`, which may be another machine, it starts in the host's folder. A lone argument is a
  folder when it starts with `/`, `~`, or `.`, contains `/`, or names an
  existing folder under the attaching directory; otherwise it is an agent, so
  `/new <agent>` keeps working. Folders are made absolute (`~` expanded,
  relative ones joined to the attaching directory) before sending. `cwd` is
  sent only when the host's `initialize` advertised
  `capabilities.nori_sessions.session_cwd`; otherwise plain `/new` sends no
  folder and a named folder is refused with a notice, keeping the draft.
- Tab on a `/new <partial>` draft completes the folder like a shell, against
  this machine's filesystem (so on a remote `--host` it sees local folders):
  it fills the longest prefix the matching folders share, appends `/` to a
  single match, expands `~`, hides dot-folders unless the typed prefix starts
  with `.`, and lists several matches in the notice line.

### Things to Know

- [Input routing](input.rs) honors `hotkeys.open_session_list`, also opens the
  list with Left on an empty composer or `/sessions`, and resumes a highlighted
  failed session with Ctrl+R while retaining its conversation identity.
- Detach closes only the client. Session close, cancellation, and resume are
  explicit host commands, and a role handoff never closes the old controller.
  For agent-started work, Ctrl+C preserves the draft and uses the same
  controller-only request as `/cancel`, `_nori/session/cancel_observed`.
  The host checks the observation ID against the active turn. Attach shows
  delivery or confirmation failures and leaves any retry to the controller.
  For a host-admitted prompt, `/cancel` sends the standard `session/cancel`
  notification. `/close` sends ACP `session/close`, and `/resume` and Ctrl+R
  send `session/resume` with the row's
  folder (`_nori/session/resume` for an older host whose rows carry no
  folder). A resumed session is attached to this connection, so the host
  also sends it plain `session/update` notifications and
  `session/request_permission` requests. Attach ignores both: it renders the
  subscription and answers permissions through it. A request from the host is
  never matched against attach's own pending request IDs.
- Unacknowledged mutations are not replayed after reconnect because the host
  may already have applied them. Recovery clears pending correlations and
  warns when an outcome is uncertain; only the event subscription resumes.
- Replay failure preserves existing conversation content and displays the
  error. It does not silently substitute a new live cursor. Cursors are local
  memory, and a fresh attachment starts from the host journal's beginning.
- A changed or unavailable host identity, an unrecoverable authentication
  failure, or rejected initialization stops recovery. The terminal retains
  conversation history and the composer draft, displays a persistent reason
  with instructions to rerun `nori attach`, and remains open for Ctrl+D detach.
  No initialization or replay reaches a replacement host after identity
  comparison rejects it; token equality alone cannot establish continuity.
- Snapshot and reducer [tests](tests.rs) cover roles, read-only behavior, and
  sequence application. The [PTY scenarios](../../../tui-pty-e2e/tests/host_attach.rs)
  exercise the complete host, multiple terminals, and dropped or silently
  stalled TCP connections.

Created and maintained by Nori.
