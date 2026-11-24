# Noridoc: Session manager

Path: @/nori-rs/harness/src/session_manager

### Overview

- The [session manager](mod.rs) supervises ACP agent processes. Its clones share a registry keyed by conversation ID.
- The [registry](registry.rs) owns session records, envelope sequence numbers, derived status, family status, and the concurrency cap. It consumes session events without performing I/O, so these rules can be tested without an agent process.

### How it fits into the larger codebase

```text
 client (TUI)                    SessionManager
     | start / resume / close        |  registry: conversation ID -> record
     v                               |  capacity semaphore (max_sessions)
  StartSession ----------------------+
                                     |  one pump task per agent process
                 per-session runtime |  applies event -> status, seq
                 (HarnessHandle) ----+--> session's own stream (unbounded)
                                     +--> journal + bounded subscriber streams
```

### Core Implementation

#### Session manager and envelopes

- **Start.** `start` takes an agent (already `Prepared`, or a `Prepare` spec to
  spawn one), a `SessionStart`, an optional parent, and a `SessionSource`
  (`User`, `Fork`, or `Session`). The manager reserves a capacity slot, mints
  the conversation ID before launch, and passes it down through
  `AcpBackendConfig.conversation_id`; the transcript recorder uses it as the
  transcript ID. It returns a `ManagedSession` with the ID, a `HarnessHandle`,
  and the session's own envelope stream. Start fails with `CapacityReached`,
  `ShuttingDown`, `UnknownParent`, or `AgentAlreadyActivated`. Delegated start
  returns `ParentEnded` when its parent has no live process or has been closed
  for delegation.
  `max_sessions()` and `free_slots()` report the cap and the slots left, so
  a client can refuse a new session before it starts an agent process. A
  slot comes back when the session's agent process ends.
- **Envelopes.** One pump task per agent process applies each event to the
  registry first, then publishes the envelope to all-session subscribers and to
  the session's own stream. The pump reads two sources with a biased select:
  the handle's answer queue first, then the agent's event stream (see
  "Answered requests" below). The own stream is unbounded and lossless, matching
  the primary receiver of a directly launched session. `subscribe(capacity)`
  follows every session; a subscriber that falls `capacity` envelopes behind is
  dropped and its receiver closes, and sessions are never slowed.
- **Replay.** [`replay.rs`](replay.rs) journals envelopes
  under the same mutex as registry updates and publication. `subscribe_from`
  validates per-conversation cursors, snapshots envelopes strictly after them,
  and registers the live subscriber atomically. Single-session filters keep
  that identity; all-session filters include future sessions. Missing cursors
  start at zero. History eviction is global across sessions, and an expired or
  invalid cursor fails instead of silently truncating replay. Retention is
  bounded and lasts only as long as the manager; persisted transcripts are
  not a source of transport sequence numbers. Resume preserves the journal,
  sequence counter, and subscribers for the same conversation ID, so both
  single-session and all-session clients continue across process replacement.
  Alongside the envelopes the journal keeps per-session display facts for
  session lists: the title (the agent's `session_info_update` title or the
  prompt summary, whichever came last; without one, or once the agent clears
  its title, the first user message's first line), the start of
  the latest reply through the shared `nori_protocol::ReplyPreview`, the
  oldest open request through the shared `nori_protocol::PendingRequest`
  (reported as `waiting_request`, such as `approve: cargo test -p parser`;
  lists show it in place of the reply while the session waits on you), the last
  `usage_update` (tokens used and context size), and the time of the latest
  envelope (reported as idle seconds).
- **Status.** Each record carries a `SessionStatus`:

  | Status | When |
  | --- | --- |
  | `Starting` | Before `SessionStarted` |
  | `Working` | Phase is `Prompting`, `Loading`, or `Cancelling` |
  | `WaitingOnYou` | An ACP permission request or child-start approval is pending |
  | `Ready` | Idle |
  | `Failed` | `SessionEnded` with `ConnectionLost`, `SpawnFailed`, or `TimedOut`, or the stream closed with no end event |
  | `Closed` | `SessionEnded` with `Shutdown` or `Closed`, or superseded by a fork |
  | `Done` | A delegated child's `SessionEnded(Closed)` after its task completed |

  The [delegation driver](delegation_driver.rs) marks a
  child's registry entry task-done (`Registry::mark_task_done`) when its
  delegation reaches `Completed`, before closing the child; the entry then
  records that close as `Done`. Every other end keeps its ordinary status.
  With `[host] managed = false`, the parent link and terminal status live
  only in the in-memory manager. After a local host
  restart a finished child is not listed at all (`nori host ls` and the
  `nori attach` list omit it), and reopening its transcript with `/resume`
  starts it as a live top-level `ready` session with no parent link.

  A pending request clears when any `HarnessHandle` clone answers it, when the
  phase returns to `Idle` or enters `Cancelling`, or when the session ends.
- **Answered requests.** `respond_to_agent` on any handle clone calls a
  crate-private observer with the request ID and the handle's responder label
  just before it sends the answer. The manager turns that into
  `NoriEvent::RequestResolved { request_id, answered_by }` on its answer queue;
  because the pump drains that queue ahead of agent events, the announcement is
  numbered before anything the agent does in reaction to the answer. The
  registry publishes it only if the request was still pending, so it appears
  once per request on the session's own stream and on `subscribe()` streams.
  A client can read a request straight from the harness and answer it before
  the pump has numbered the request. The registry holds such an early answer
  and publishes it right after the request's own envelope; held answers are
  dropped when the turn ends, is cancelled, or the session ends.
  Requests cleared by `Idle`, `Cancelling`, or session end produce no
  `RequestResolved`; the phase or end event already conveys it. An answer the
  backend then rejects may still have been announced. Harness sessions never
  emit this event themselves, so it is absent from directly launched streams
  and from transcripts; it exists only on manager envelopes so that every
  client following a session learns the request no longer waits on the user.
- **Families.** A session family is a top-level session plus all of its
  descendants. `family_status` rolls member statuses up with priority
  `WaitingOnYou` > `Failed` > `Working` > `Starting` > `Ready` > `Closed` /
  `Done` (equal rank).
- **Fork.** On `NoriEvent::SessionForked` (branch at head) the parent becomes
  `Closed` and a child record (source `Fork`, parent = previous conversation)
  takes over the same agent process and capacity slot. `SessionForked` is
  delivered as sequence 1 of the child conversation. A fork to an earlier
  message instead starts a new agent process: the caller marks the
  `PreparedAgent` with `with_fork_parent(id)`, and the client starts it with
  source `Fork` and that parent. Such a fork does not close its parent or set
  the parent's `forked_into`; the client closes the old session separately.
- **Resume.** `resume(failed_id, spec)` restarts a `Failed` session from its
  transcript on a new agent process while keeping its conversation ID,
  registration order, parent, source, children, and sequence counter. Registry
  replacement and process routing change atomically; events and EOF from the
  old process generation cannot alter the restarted session. Native ACP load
  uses the recorded ACP session ID when supported; a fallback new ACP session
  stays mapped to the same Nori identity. `get`, `handle`, remote targeting,
  and subscriptions therefore keep using `failed_id`. `resumed_from` is
  private registry metadata used for restart logging; neither `SessionRecord`
  nor managed remote list responses expose it.
  Resume validates failed status, agent identity, and a readable transcript,
  then reserves capacity before launching the replacement. `reopen(id)` marks
  a closed session whose process has exited as restartable, so the same
  `resume` path brings it back under its ID; the background host uses this
  when a client loads or resumes a session it earlier closed with ACP
  `session/close`.
- **Managed checkpoint metadata.** `checkpoint_session(id, include_private_transcript)`
  captures the named conversation's identity, parent, source, status and its
  delegation records. The caller holds the binding admission barrier.
  A checkpoint that requests continuation evidence requires a live, idle
  recorder. The writer drains prior writes, syncs its file and reads from
  that same open file. The returned JSONL is private recovery data.
  `import_checkpoint` restores historical records to the destination registry
  with the supplied workspace and policy. A finished child retains its
  `Done` status and parent ID even when its parent is on a different host.
  A previously live session appears interrupted (`Failed`). Import consumes
  no process slot and submits no prompts. Provider continuation and durable
  cross-machine delegation result delivery require their own owning paths.
- [Explicit close](explicit_close.rs) waits for a capable adapter’s acknowledgement.
  Permission answers and forced shutdown remain available during the wait.
  The event pump binds a successful close to the original process and captures
  its descendants before publishing the closed row. Cleanup retries address
  those process generations. A refusal leaves the tree and pending work live.
- **Forced close and shutdown.** `close(id, child_grace)` cancels delegated descendants and shuts the
  target session down, bounded by `shutdown_timeout`. An ended target has no
  process to stop, but its running descendants and pending approvals still
  require cancellation.
  Successful close waits for the manager's event pump to observe process end
  and release the capacity slot, so a caller can immediately start a
  replacement. Backend shutdown and that observation share one timeout. The
  pump notifies waiters after reducing terminal state or stream EOF; close
  does not synthesize a lifecycle event.
  `close` follows the session's lineage to the live process: a parent forked at
  head leads to the child that took over its agent process; a resumed
  session keeps that same lineage target. `shutdown(child_grace)` stops
  admission, then shuts every live session down in parallel, each with the same
  timeout, and returns a
  `ShutdownReport` of closed, timed-out, and failed sessions. Registration and handle installation share one lock, so shutdown sees every admitted process.
- **Handle lifetime.** The manager drops its handle to a session as soon as the
  session ends. A dead agent process therefore does not keep the session's
  runtime alive: once clients drop their handles too, the session's event
  stream closes.
- **Queries.** `get`, `list` (registration order, ended sessions kept), and
  `handle` (live sessions only) read the registry. A record holds the agent,
  parent, source, cwd, model, approval policy, transcript path,
  ACP session ID, status, and `pending_requests` (open agent request IDs in
  arrival order). The model starts as the config default and is
  replaced by the agent-advertised model-category config option from
  `SessionStarted` or a config option update. `model_name` keeps that
  option's display name (the selected choice's name, else its raw value) for
  session lists.

`SessionManagerConfig` defaults to eight sessions and a five-second shutdown
timeout.

#### Delegation between managed sessions

The [session manager](mod.rs) owns delegation admission,
approval, parent/child links, result delivery, and descendant lifetime. Both terminal `/delegate` commands and the session-bound `delegate`
tool on the existing authenticated
[`nori-client` HTTP MCP server](../backend/nori_client_mcp.rs) enter this same
manager path. The tool takes an agent and task; its authority comes from the
calling session, not a caller-supplied parent ID. Every session the manager
launches gets a delegation context, so delegation is always available there;
a session launched directly through `launch_session` outside the manager has
none, and its `delegate` tool answers "delegation is available only in
sessions the session manager runs".

```text
parent agent -- HTTP MCP delegate --+
local /delegate -------------------+--> SessionManager
remote /session/delegate ----------+         |
                                   different agent? -> approve / decline
                                             |
                                      child ACP process
                                             |
                                  correlated prompt outcome
                                             |
                                 attributed parent prompt queue
```

- Delegation records reserve a child conversation identity before approval.
  Same-kind children start immediately; different-kind children stay
  `awaiting_approval` until the user approves or declines. Approval is scoped to the
  requesting parent and request ID. Spawn admission uses the manager's shared
  session cap, including children, and is checked when a child actually starts.
- Depth counts delegation edges, separately from fork ancestry. A top-level
  session may delegate to a child, which may delegate to a grandchild;
  deeper requests fail with an error returned to the requester. Branch-at-head
  transfers delegation ownership to the conversation now carried by that
  process without adding depth. Parent links, pending start approvals, and
  result correlation follow the fork; the delegation request identity remains
  stable. [State reduction](delegation_state.rs) republishes
  the changed attribution for observers.
- `DelegationChanged` carries request, parent, and child identities, agent
  names, task, depth, status, optional result, and origin. The MCP tool path
  passes the calling process as its expected process and records origin
  `agent`; the public `SessionManager::delegate` (local and remote `/delegate`)
  records `user`. Frontends use origin only to word who asked. Manager envelopes make those
  changes observable through ordinary subscriptions and reconnect replay,
  including events published while a failed parent has no process. Child-start
  approval contributes to the parent's `WaitingOnYou` status separately from
  its ACP permission request IDs.
  ACP permission requests remain owned by the child conversation; permission
  responses must target that child, not its parent. When a child sends an ACP
  permission request, [state reduction](delegation_state.rs)
  emits a `Notice` into the parent:
  `<agent> · <child name> is waiting on you (delegated by <parent agent> · <parent name>). Press Alt+Down to answer it.`
  The child name comes from `nori_protocol::delegation_child_name`; the parent
  name is the parent's replay-journal title through
  `nori_protocol::session_display_name` (40 characters, cut in the middle),
  else `this session`.
- A child's final output or failure becomes an attributed prompt queued into
  its parent automatically, without a review step. Its text is
  `nori_protocol::delegation_result_prompt`: a readable first line naming the
  child agent, the child's whole first task line, and outcome, then the child ID, then the result.
  It does not repeat the parent's own ID. The result is the text of the
  child's agent message chunks from its last prompting turn; when a tool call,
  permission request, or other session update arrived between chunks, the next
  chunk starts a new paragraph (`\n\n`) rather than being joined directly. A
  child that crashed reports only the first non-empty line of its
  `RequestFailed` message (for example `Agent process exited unexpectedly
  (exit code 17).`); the agent's stderr tail stays in the child's own history.
  Frontends recognize these prompts through
  `nori_protocol::delegation_result_summary`. A busy parent consumes it
  through its normal prompt queue. The [result driver](delegation_driver.rs)
  retains that delivery until the parent's correlated ACP `PromptResponse`,
  not merely queue admission. If the process crashes first, stable-ID resume
  resubmits the unacknowledged result; the parent may therefore see it again.
  A child that delegates waits for its descendants and resulting continuation
  turns before returning its own final result. Terminal completion stops the
  child process and releases its capacity slot while retaining the registry
  record and transcript.
- A result prompt never titles the parent that receives it. The
  [replay journal](replay.rs) skips user-message chunks
  that are result prompts when it derives the first-prompt title, and takes
  that title for a parent from the task of a user-origin `DelegationChanged` it requested, so a
  session whose first input is `/delegate` is named by that task. The
  first-prompt summary hook in [user input](../backend/user_input.rs) likewise
  ignores result prompts, leaving the summary for the user's own first prompt.
- Explicit parent close cancels descendants and invalidates pending delegation
  approvals, including when the parent has already failed. A parent process
  crash leaves children running; results wait for the parent's stable-ID
  resume. The held result queue belongs to the live manager. Managed checkpoints
  can carry historical delegation metadata as described above. A child crash produces a failure
  result without failing its parent.
- Child registration and tree retirement share the registry lock. Result work is claimed under manager state, then prompts and process
  shutdown run outside that lock. Prompt request IDs, outstanding descendants,
  and in-flight result submissions determine completion; an idle phase alone
  does not prove that a delegated task has finished. The MCP authority follows
  its registered process through a weak manager reference, avoiding an
  ownership cycle and rejecting calls from an ended process.

#### Approval policy, permission mode, and agent replacement

`set_approval_policy` changes a running session's approval policy and records
it on the session. When the policy reads as a different permission mode
(`permission_mode::PermissionMode::from_approval_policy`, so `on-request` to
`on-failure` is no change), it also emits `NoriEvent::ApprovalModeChanged` into
the journal and the session's own stream.

Every client learns a session's mode from that event. The registry emits it
in two other places, building it with `permission_mode::announced` from the
policy in the session's settings:

| When | Order |
| --- | --- |
| The agent's `SessionStarted` arrives | Just before `SessionStarted`, so a client has the mode before it draws the started session |
| A `SessionForked` arrives | Just after the fork, addressed to the child, which gets no start event of its own |
| `set_approval_policy` changes the mode | On its own |

A session's events replay from the journal, so a client that subscribes later
sees the latest announcement while it is still in the replay window; once it
has been evicted, the `approval_mode` field of `_nori/session/list` rows
(from the record's approval policy) is the fallback.

`replace_agent` stops a top-level session's process and
starts another agent under the same conversation ID; the host only uses it
before the session's first prompt, so the restarted transcript loses nothing.

### Things to Know

Agents without HTTP MCP support can still receive children started by the
user's `/delegate`; an agent-side stdio delegation tool is not provided.
The [managed remote contract](../../../../docs/remote-control.md#delegation) defines
controller authorization and request payloads.

Created and maintained by Nori.
