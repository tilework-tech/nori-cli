# Noridoc: Attached host frontend

Path: @/nori-rs/tui/src/attach

### Overview

- Renders a terminal attached to a background host without owning its sessions
  or agent processes.
- Keeps conversation presentation, session selection, connection role, and
  replay cursors local to this terminal.

### How it fits into the larger codebase

- [CLI host dispatch](../../../cli/src/host/docs.md) supplies a resolved endpoint
  and token after enforcing `multi_session`. This frontend is separate from the
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
  `<agent> · <name> · <status>`, where the name is the session title through
  `nori_protocol::session_display_name` (else the short ID). Inside a delegated
  child whose parent is listed, it becomes a breadcrumb
  `<parent agent> · <parent name> › <agent> · <child name> · <status>` followed
  by the `parent_session` key (Alt+Up by default).
- The session list is tree-ordered: [AttachState](state.rs) runs the listed
  `(session_id, parent)` pairs through the shared picker `tree_order` and gives
  each item its depth, and the picker is built with `tree_column("title")`.
  Tree guides and the `(current)` badge therefore sit inside TITLE while ID,
  AGENT, STATUS, and AGE stay aligned at every depth. Status words take the
  shared `session_status::status_tone` colors (waiting on you amber, working
  cyan, ready green, failed red). The list ignores the host's `depth` field and
  derives depth from parent links.

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
- Child permissions stay in the child's conversation. The configured
  `next_waiting_session` binding (Alt+Down by default) navigates there. If the
  cached list has no waiting target, a connected terminal retains the navigation
  action while refreshing the list, then selects a newly discovered waiting
  session from the response. Later input or navigation, list errors, and
  disconnection cancel that pending action. The response also requires an empty
  composer, including after paste, so a draft begun in the parent stays there.
  Parent notices name the child and parent readably (see the
  [harness](../../../harness/docs.md#delegation-between-managed-sessions)).
  The `parent_session` binding moves the selection from a child to its parent
  when that parent is in the cached list, closing the list and resubscribing.
  Results return through the parent's ordinary event stream without a terminal
  review step.

### Things to Know

- [Input routing](input.rs) honors `hotkeys.open_session_list`, also opens the
  list with Left on an empty composer or `/sessions`, and resumes a highlighted
  failed session with Ctrl+R while retaining its conversation identity.
- Detach closes only the client. Session close, cancellation, and resume are
  explicit host commands, and a role handoff never closes the old controller.
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
