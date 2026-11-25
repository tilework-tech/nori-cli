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
  local TUI `App`, which owns an in-process host and stops it, with its
  sessions, when it quits.
- All wire traffic goes through the [host session client](../host_client/docs.md):
  connection driving, reconnect, JSON-RPC ids, and reply matching live there.
  The host retains mutation policy and broadcasts complete manager envelopes
  to controllers and watchers.
- Presentation reuses the TUI's [normalizer](../presentation), composer,
  Markdown, tool cells, and picker components. The
  [attachment composer](../public_widgets/composer_input.rs) shows the local
  TUI's `/` menu and returns builtin commands for this frontend to run;
  unknown slash names (agent commands, custom prompts) are submitted as text
  for the agent rather than refused. Bare `exit`/`quit` detach, as in plain
  `nori`. A terminal's selected session does not change another terminal's
  selection.

[Commands](commands.rs) take their verdict from the shared
[command availability](../command_availability.rs) table (see the
[TUI docs](../../docs.md#commands-and-approvals)) with
`Surface::Attached { watching, connected }`:

- `sync_command_menu` greys every builtin with its reason. It runs when the
  runtime is built, before each key and after each connection event, so an
  open menu follows role changes and reconnects.
- Attach cannot list the agent's own commands, so Enter runs a highlighted
  builtin only when the typed name is the start of its name. `/review` stays
  a prompt even though the menu's loose match highlights `/resume-viewonly`.
- With a menu open, the first Escape closes it and keeps the draft and the turn. The menu stays closed for that text, so the next Escape cancels as usual. Plain `nori` behaves the same.
- Builtin names win over same-named agent commands, as in plain `nori`:
  `/compact` or `/model` typed in attach is refused with its reason instead
  of reaching the agent as text.
- `run_command` executes popup selections and typed builtins. A refused
  command restores the typed text and shows `/<name>: <reason>`. While
  reconnecting, stopped, or after a replay gap, the existing
  `require_mutation` messages apply instead. Commands the host could run but
  attach has no screen for read `not available in nori attach yet`; commands
  that need plain `nori`'s in-process session read `not available on this
  host yet`; a watcher's controller-only commands read `watching · Alt+T to
  take control`.
- `/approvals` is controller-only, so a watcher sees it greyed with
  `watching · Alt+T to take control`; the host would also refuse a watcher's
  `session/set_mode` with `-32016`.
- `/detach` and `/cancel` remain attach-only aliases. `/close` and `/resume`
  with trailing text go to the agent as a prompt.
- Attach's `/resume` resumes the on-screen failed session; plain `nori`'s
  opens the saved-session picker. Watcher prompt refusals keep their
  read-only wording.

The controller uses the [host follow-up operation](../../../cli/src/host/docs.md#mid-turn-follow-ups) for a composed message during a working turn. Enter steers and Tab queues. Session events supply the expected turn ID. A pending permission dialog continues to handle its own keys and idle Enter submits an ordinary prompt.

### Core Implementation

- [Entry](mod.rs) packs the endpoint, token, and local-token refresh callback
  into a `HostEndpoint`, spawns the host client's
  [connection driver](../host_client/connection.rs), and refreshes the session
  list every second. That refresh is also what keeps an idle connection inside
  the client's liveness deadline.
- The [Runtime](runtime.rs) keeps UI state and a `Pending` tag per call; the
  host client allocates ids and hands each reply back with its tag. Every
  `Connected` event sends `initialize`. Initialization supplies the role, refreshes the session list,
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
- With `[tui] program_status` on, each `Conversation` folds its envelopes into
  its own [`ProgramStatusTracker`](../program_status/tracker.rs), and the
  [entry](mod.rs) reports the selected session's status to the terminal as
  OSC 7501 (see [terminal program status](../../docs.md#terminal-program-status)).
  It reports after a loop iteration only once the incoming event channel is
  drained, so a replay burst produces one report. A selected session with no
  events yet reports nothing, leaving the previous report in place; with no
  session open it reports idle. `run_attach` clears the status on detach or
  exit. Controller and watcher terminals both report.
- [Rendering](render.rs) keeps role and connectivity visible beside the
  selected conversation or session list. Watchers receive the same content and
  permissions but must take control before sending mutations.
- Consecutive thought chunks join, as sent, into one part whatever their
  message ids, and render as the chat view's `ThoughtCell` block before the
  answer that ends them. `[tui] thoughts_in_history = false` hides them.
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
- [`/approvals`](approvals.rs) shows and sets the host's permission mode
  for the selected session; the host owns the mode, and this terminal only
  displays what the host announced and asks for changes. `AttachState`
  keeps each conversation's latest `ApprovalModeChanged` and falls back to
  the list row's `approval_mode` once the announcement has left the replay
  window. Without arguments the command opens a `ModeMenu` pinned to that
  session; `ask`, `agent` or `approve-all` sets the mode directly, and an
  unknown word restores the draft with a notice. Every change is one
  `HostCall::SetMode` (`session/set_mode`) mutation; Approve all always waits
  for a second Enter, whether picked from the menu or typed. The menu is drawn
  in place of the composer and takes keys only while no request is on screen,
  and only unmodified Up/Down, `j`/`k`, Enter and Esc, so Alt+T and
  session-move bindings keep working. Losing control or selecting another
  session closes it. The identity line appends `· Approvals: <mode>` once a
  mode is known. Nothing here writes configuration: new sessions start in the
  host default. The modes the host announces for the session
  (`approval_mode_changed.available`, or the list row's `approval_modes`) are
  the authority: a mode left out, such as Ask for an agent with no mode that
  asks every time, shows as not available and is refused here without a
  request. A host refusal still arrives as an ordinary mutation error.
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
  the folder of the session on screen (its list row's `cwd`, or the folder
  its `session_started` event reported). With no session on screen it starts
  in the attaching terminal's directory, or on a host named with `--host`,
  which may be another machine, in the host's own folder. A lone argument is a folder when it starts
  with `/`, `~`, or `.`, or contains `/`; on a local host it is also a folder
  when it names an existing folder under the base. Otherwise it is an agent,
  so `/new <agent>` keeps working. A `--host` host never consults this
  machine's disk for that rule. Who resolves the folder depends on the host:

  | Typed folder | Host advertises `find_paths` | Older host |
  | --- | --- | --- |
  | `~` or `~/…` | Sent as typed; the host uses its own home | Expanded with this machine's home |
  | Relative, session on screen | Joined to the session's folder here (already a host path) | Same |
  | Relative, no session, `--host` host | Sent as typed; the host uses its own folder | Joined to the terminal's directory |
  | Relative, no session, local host | Joined to the terminal's directory | Same |

  A client cannot know a remote host's home, which is why a host with
  `find_paths` names `~` itself. An older host accepts only absolute
  folders, so on a remote older host whose home differs, name the folder
  another way. Every other folder is made absolute here before sending.
  `cwd` is sent only when the host's `initialize` advertised `capabilities.nori_sessions.session_cwd`;
  otherwise plain `/new` sends no folder and a named folder is refused with a
  notice, keeping the draft.
- Tab on a `/new <partial>` draft completes the folder like a shell. When
  the host advertised `capabilities.nori_sessions.find_paths`, Tab sends
  `_nori/session/find_paths` (`kind: "folders"`, the session on screen, at
  most 50 matches) and completes from the host's disk once it answers; a
  reply that arrives after the draft changed is dropped. An older host is
  completed against this machine's filesystem. Either way Tab fills the
  longest prefix the matching folders share (a host list of 50, which may
  have been cut short, is only listed), appends `/` to a single match,
  hides dot-folders unless the typed prefix starts with `.`, and lists
  several matches in the notice line. The completion helpers live in
  [`session_folder.rs`](../session_folder.rs), shared with plain `nori`.
- `@` file mentions use the same call with `kind: "files"` for the session
  on screen (the host's folder with none), limit 20. The composer reports each
  edit of its `@` token; one search is in flight at a time and only the newest
  query typed meanwhile is sent when it lands
  ([`HostFileSearch`](../host_client/file_search.rs)). Matches for a session
  no longer on screen are dropped. A host without `find_paths` gets no
  search and the popup stays empty.

### Things to Know

- `nori attach` does not check the host contract version. It adapts to an
  older host by the capabilities that host advertises (`session_cwd`,
  `find_paths`), as with folder naming above.
- Older `nori attach` builds decode `_nori/session/event` envelopes strictly.
  This build declares `capabilities.nori_sessions.approval_modes` in its
  `initialize`. The host sends `approval_mode_changed` only to clients that
  declare it. Any other client gets a `notice` under the same `seq`, live and
  in replay, so an older client keeps its replay order and shows the change
  as a notice.
- [Input routing](input.rs) honors `hotkeys.open_session_list`, also opens the
  list with Left on an empty composer or `/sessions`, and resumes a highlighted
  failed session with Ctrl+R while retaining its conversation identity.
- Detach closes only the client. Session close, cancellation, and resume are
  explicit host commands, and a role handoff never closes the old controller.
  For agent-started work, Ctrl+C preserves the draft and uses the same
  controller-only request as `/cancel`, `_nori/session/cancel_observed`.
  The host checks the observation ID against the active turn. Attach shows
  delivery or confirmation failures and leaves any retry to the controller.
  For an admitted prompt the same request carries its ACP request UUID.
  Before admission, attach remembers Escape for that submission and waits
  for its receipt. Losing control discards the pending interrupt.
- [`/close`](close.rs) sends ACP `session/close` tagged `Pending::Close` with
  the targeted session. While that tag is outstanding, another `/close` for
  the same session is not sent and shows "This session is already closing."
  The adapter's refusal reaches attach twice, as an unowned error on the
  session stream and as the host's error reply, so the conversation's
  [`CloseRefusal`](../close_refusal.rs) holds stream errors while the close
  is in flight. The ledger is armed only once `Runtime::send` reports the
  request reached the connection, so a close that never left holds nothing.
  On refusal, held errors the host text already contains are dropped, others
  are shown first, and one `Failed to close the session: …` notice lands in
  the targeted session's transcript; the session stays open for a retry.
  When the terminal is showing another session, the notice row names the
  refused one; otherwise the row is left as it was. A refusal reported before
  its stream copy waits only until the next non-error stream event for that
  session or a disconnect, so it cannot swallow a later unrelated error. The
  host's success reply, or a disconnect before any reply, releases held
  errors into the transcript. Attach does not settle on the stream's close
  response: its ACP body is `{}` and deserializes as an ambiguous untagged
  variant.
- `/resume` and Ctrl+R send `session/resume` with the row's
  folder (`_nori/session/resume` for an older host whose rows carry no
  folder). A resumed session is attached to this connection, so the host
  also sends it plain `session/update` notifications and
  `session/request_permission` requests. The host client reports both as
  ignored: attach renders the subscription and answers permissions through it.
- The host client never replays unacknowledged calls after reconnect (see
  [its docs](../host_client/docs.md)). On disconnect attach receives the
  unanswered `Pending` tags; if any was a mutation, close, prompt, follow-up,
  or cancel it warns that the outcome is uncertain and marks follow-ups and
  cancel delivery unknown. Only the event subscription resumes.
- Replay failure preserves existing conversation content and displays the
  error. It does not silently substitute a new live cursor. Cursors are local
  memory, and a fresh attachment starts from the host journal's beginning.
- A changed or unavailable host identity or an unrecoverable authentication
  failure (both detected by the host client), or rejected initialization,
  stops recovery. The terminal retains conversation history and the composer
  draft, displays a persistent reason with instructions to rerun
  `nori attach`, and remains open for Ctrl+D detach.
- Snapshot and reducer [tests](tests.rs) cover roles, read-only behavior, the
  controller and watcher command menus, and sequence application;
  [approvals tests](tests_approvals.rs) cover the mode menu and the Approve
  all confirmation. The [PTY scenarios](../../../tui-pty-e2e/tests/host_attach.rs)
  exercise the complete host, multiple terminals, and dropped or silently
  stalled TCP connections.

Created and maintained by Nori.
