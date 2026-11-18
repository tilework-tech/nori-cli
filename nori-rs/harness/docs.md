# Noridoc: nori-harness

Path: @/nori-rs/harness

### Overview

`nori-harness` is the headless, embeddable runtime for Nori ACP sessions. It
composes the low-level ACP host with session lifecycle, private reduction,
transcripts, queueing, history, goals, undo, hooks, user-shell operations, and
worktree behavior. It has no terminal dependency.

The public embedding contract separates connection preparation from session
activation: initialize and inspect one agent connection, consume that unique
prepared owner with an explicit session choice, then control the active session
through typed methods and one ordered stream of `nori_protocol::SessionEvent`
values. Additional bounded subscribers can follow the same ordered stream
without disturbing the primary consumer.

The session manager sits above that per-session runtime. It keeps a registry of
live sessions keyed by conversation ID, runs each session on its own agent
process, tags every event as a `nori_protocol::SessionEnvelope`, and tracks a
coarse `SessionStatus` for each session and each session family.

### How it fits into the larger codebase

```text
nori-exec       nori-tui       remote ACP client
        \           |               |  WebSocket /acp
         \          |               v
          \         |     RemoteAcpServer (nori-acp-host::remote)
           v        v               |  HostedAgent trait
              nori-harness  <-------+  (remote_agent.rs implements it)
               /       \
              v         v
      nori-acp-host   nori-config
              |
              v
        ACP agent process
```

The harness consumes raw ACP traffic from `nori-acp-host`, publishes it without
changing ACP semantics, and adds the small Nori-owned event branch. Stateful
ACP reduction remains private because it supports harness behavior; widget and
display projection remains private to the TUI.

`nori-exec` is the production headless consumer. It uses the same typed handle
and event stream to implement both a finite plaintext projection and a bounded
ACP agent facade. The facade preserves ACP request/response semantics where the
shell caller participates, rather than serializing the private reducer or
inventing a second public event vocabulary.

`nori-tui` starts every session through one app-owned `SessionManager`. The
manager calls the same launch internals as `launch_session`, so a managed
session behaves exactly like a directly launched one. With the runtime
`multi_session` flag, `nori-exec --acp` and the remote `/sessions` endpoint also
use that manager. Plaintext, flag-off exec, and the public
`launch_session`/`prepare_and_launch_session` entry points retain their direct
per-session runtime paths.

The remote ACP transport (`@/docs/remote-control.md`) is the other
headless consumer. `nori-acp-host` owns the WebSocket server and defines the
`HostedAgent` trait; [`remote_agent.rs`](src/remote_agent.rs) implements that
trait over `HarnessHandle`, while
[`managed_remote.rs`](src/managed_remote.rs) implements `ManagedAgent` over
`SessionManager`. Both keep the dependency direction
`nori-harness -> nori-acp-host`. Remote mutations pass through the same typed
handle as local ones, so hooks, transcripts, goals, and permission policy all
still apply, and the TUI observes remote-driven activity on its own stream.

### Core Implementation

#### Connection preparation, session activation, and event stream

A frontend constructs `AgentPrepareSpec` with one resolved `Arc<NoriConfig>`,
the CLI version, and optional product/conversation context. `prepare_agent`
resolves the persisted default model and injects it through the agent's
spawn-time channel before spawning the ACP subprocess. It then completes
`initialize`, reads the connection's capabilities, and normally calls
`session/list` when advertised. When a version-1
`_meta.nori.remoteControl.activeSessionId` marker is paired with `loadSession`,
the prepared owner records that ID, skips listing, and loads it as the first
post-initialize request during activation. It returns an
opaque, non-cloneable `PreparedAgent` that still owns that exact
`AcpConnection`, its event receiver, resolved backend configuration, buffered
initialize event, and a `SessionCatalog`. The catalog distinguishes
`Unsupported` from a successful `Listed` result, including an empty list; an
advertised list call that fails fails preparation rather than masquerading as
either state. Preparation is implemented in
[`prepared.rs`](src/backend/prepared.rs).

The frontend then chooses `SessionStart::New` or
`SessionStart::Resume(SessionResume)` and passes the prepared owner in
`SessionLaunchSpec`. `launch_session` consumes it, so a connection cannot be
activated twice or used for pre-session prompt/config operations. Resume
selects the existing load, live-resume, or transcript-replay path from the
agent capabilities and supplied resume data. A recognized Nori remote-control
ID overrides the ordinary product choice, including picker-first TUI flows;
load failure remains terminal and never creates a replacement session.
Frontends that have already made their session choice may use
`prepare_and_launch_session`; it performs the two phases in one background
lifecycle while retaining the same connection. It returns the handle
immediately, queues ordinary commands until activation, and
keeps preparation inside the runtime's connection warning, timeout, and
shutdown race. Both launch paths return `LaunchedSession`, containing a
`HarnessHandle` and the session event receiver.

A frontend that keeps a prepared connection behind a picker may call
`refresh_prepared_agent(&mut agent, spec)` immediately before activation. This
replaces session-time policy and context from the latest resolved config while
preserving the initialized transport. The prepared agent identity, working
directory, ACP proxy/wire-recording configuration, and default model are
preparation-fixed inputs; if any changed, refresh fails and the caller must
discard the connection and prepare again. This lets mutable settings change
during a picker without silently applying them to the wrong process or
transport.

Product context has HTTP-MCP and non-HTTP-MCP variants. After initialization
reveals the connected agent's capabilities, the harness selects the matching
variant and prepends it to the first locally submitted prompt only. This keeps
source identity common across ACP agents while reserving MCP fallback guidance
for agents that cannot use Nori's HTTP MCP affordances.

`PreparedAgent` owns teardown before activation: explicit `shutdown` awaits
process cleanup after a brief 250 ms stdin-EOF grace, while dropping an
abandoned prepared value reaches the connection's immediate subprocess
backstop. The short explicit grace lets a sessionless agent perform cooperative
stdio cleanup without applying the active cloud-session detach policy. After
activation, teardown ownership moves to the session-bound `AcpBackend` and
`HarnessHandle`. The backend never carries an optional session ID merely to
represent preparation.

The stream is an ordered fan-out (`SessionEventFanout` in
[`runtime.rs`](src/runtime.rs)): the primary receiver returned by
`launch_session` is unbounded and unchanged, while
`HarnessHandle::subscribe_events()` registers additional bounded consumers such
as the remote ACP host. Every consumer sees the same events in the same order.
A subscriber that falls a full queue behind is dropped — its receiver closes —
so a slow consumer can never block the harness or the primary frontend.
Subscribe commands are honored immediately even during the connect phase,
while ordinary commands queue until the backend is ready, so a subscriber
attached right after launch cannot miss startup events such as
`SessionStarted`.

When a queued user prompt becomes active, the harness publishes the caller's
original ordered ACP content as `session/update` `user_message_chunk`
notifications before downstream agent output can enter the fan-out. Every
chunk for that prompt carries one shared message id, so all frontends—including
the submitter—observe and group the same canonical input. Nori-owned context
prepended to the downstream wire prompt is not part of this public copy.
`nori_protocol::PROMPT_ECHO_ID_META_KEY` names the
`nori.dev/promptEchoId` correlation extension. Remote prompt metadata crosses
`HostedAgent` and `HarnessHandle` intact; the harness supplies the marker when
absent, retains the complete metadata while queued, and forwards it on the
top-level downstream `session/prompt` without changing prompt content. The
canonical user chunks carry only this marker, not the prompt's other metadata.
A downstream user chunk is suppressed with its paired private reducer update
only when the marker identifies the active prompt and its content matches the
active wire prompt. Load/replay chunks, unowned activity, and unmarked user
messages continue through unchanged; content alone is never used to guess
ownership.

Every admitted prompt is also bracketed in the shared fan-out by
`SessionInfoUpdate` notifications carrying `_meta.nori.status`. `working` is
published after the downstream transport assigns the prompt request id and
before canonical user chunks or agent output; `idle` follows all completion,
cancellation, or failure output. These are observer lifecycle hints, not prompt
responses: a frontend that did not submit the turn can render and flush the
complete stream without claiming the initiating request or receiving its stop
reason.

[`HarnessHandle::with_first_prompt_started_callback`](src/runtime.rs) exposes
that same transport boundary to product infrastructure without adding an ACP or
Nori protocol event. A decorated handle invokes its callback once, after its
first successful [`prompt`](src/runtime.rs) call receives the real downstream
wire request ID. Clones share the one-shot guard, so concurrent or repeated
prompts cannot duplicate the callback. A prompt rejected before transport does
not fire it. Nori's frontends use this hook for authenticated activity reporting
through [`nori-installed`](../installed/docs.md); the harness remains unaware of
identity, event schemas, and network delivery.

The public event stream has two source-owned branches:

```rust
match event {
    nori_protocol::SessionEvent::Acp(acp_event) => {
        // Agent/client semantics from the ACP schema.
    }
    nori_protocol::SessionEvent::Nori(nori_event) => {
        // Harness lifecycle and product behavior ACP does not define.
    }
}
```

Initialization, session setup/load, prompts, config changes, active-session
list calls, and close retain their raw ACP responses and original request IDs
in the stream. The pre-session list is inspection traffic consumed while
building `SessionCatalog`; it is fully drained before activation so its
response cannot be mistaken for the later session directive. The harness
buffers public bootstrap events until the consumer can receive them,
preserving current-response order without making construction depend on a
concurrently draining UI. Historical load notifications preserve their own
relative order inside replay. Current setup responses always precede
`SessionStarted`. When a local transcript permits failed-load fallback, this
means initialize, failed `session/load`, and successful fallback `session/new`
responses are all observable before the Nori session-start event; none is
labeled as replay. Without a transcript, a failed `session/load` is surfaced
and session setup ends without creating an empty replacement session.
Setup follows the actual method response rather than assuming a fixed event
count, so interleaved notifications and any post-spawn default-model config
response also retain their transport order before session start. Preparation drains its
ordered connection receiver concurrently with paginated listing so the
bounded host channel cannot deadlock; delegated requests before a session
exists are cancelled. A raw ACP setup error precedes
`SessionEnded(SpawnFailed)` and is not mirrored as
`NoriEvent::RequestFailed`.

The persisted `[default_models]` entry (`AcpBackendConfig.default_model`) is
applied at both process and session boundaries. During preparation,
[`prepared.rs`](src/backend/prepared.rs) calls the injection helper in
[`spawn_and_relay.rs`](src/backend/spawn_and_relay.rs) before spawning the exact
subprocess retained by `PreparedAgent`. `AcpAgentConfig::inject_model` forces
the model through the agent's spawn-time channel (see model injection in
[`nori-acp-host`](../acp-host/docs.md)); this is best-effort — a failure is
logged and never blocks startup. After spawn,
`session_defaults::apply_default_model` still issues the live
`session/set_config_option` RPC for agents whose model differs from the advertised
`currentValue`; when injection already made the model current, that RPC is skipped
(no config response is emitted), and for agents with no injection channel the RPC
remains the only mechanism. Injection is how an out-of-catalog model becomes the
session's current model, because the post-spawn RPC would reject it. The harness
re-exports `AgentKind::other_models()` / `OtherModel` so the TUI's `/model`
picker can offer a curated list of these injectable, unadvertised models (see
[`nori-acp-host`](../acp-host/docs.md) and [`nori-tui`](../tui/docs.md)).

#### Typed control surface

The current `HarnessHandle` methods are grouped by responsibility:

```rust
// ACP prompt and delegated-request control.
async fn prompt(Vec<acp::v1::ContentBlock>) -> Result<acp::v1::RequestId>;
async fn respond_to_agent(
    acp::v1::RequestId,
    Result<acp::v1::ClientResponse, acp::v1::Error>,
) -> Result<()>;
async fn cancel() -> Result<()>;
async fn shutdown() -> Result<()>;

// Harness-owned commands and queries.
async fn add_history(String) -> Result<()>;
async fn history_entry(i64, i64) -> Result<Option<HistoryEntry>>;
async fn search_history(i64) -> Result<Vec<HistoryEntry>>;
async fn custom_prompts() -> Result<Vec<CustomPrompt>>;
async fn compact() -> Result<()>;
async fn branch() -> Result<()>;
async fn undo() -> Result<()>;
async fn undo_snapshots() -> Result<Vec<UndoSnapshot>>;
async fn undo_to(i64) -> Result<()>;
async fn run_user_shell(String) -> Result<()>;
async fn set_approval_policy(nori_config::AskForApproval) -> Result<()>;
async fn subscribe_events() -> Result<mpsc::Receiver<SessionEvent>>;
async fn flush_transcript() -> Result<()>;

// Goal, live ACP config, and agent session lifecycle.
async fn goal() -> Result<Option<nori_protocol::ThreadGoal>>;
async fn set_goal(String, Option<nori_protocol::ThreadGoalStatus>)
    -> Result<nori_protocol::ThreadGoal>;
async fn set_goal_status(nori_protocol::ThreadGoalStatus)
    -> Result<nori_protocol::ThreadGoal>;
async fn clear_goal() -> Result<()>;
async fn get_session_config() -> Option<Vec<acp::v1::SessionConfigOption>>;
async fn set_session_config_option(String, String)
    -> Result<Vec<acp::v1::SessionConfigOption>>;
async fn list_sessions(PathBuf) -> Result<Vec<acp::v1::SessionInfo>>;
async fn close_session() -> Result<()>;
```

Prompt content is canonical as an ordered `Vec<ContentBlock>`. The harness
retains that caller-supplied value separately before adding private Nori
context to the downstream wire prompt. Text used by hooks, display, and
persistence is a derived projection only and does not replace or reconstruct
either representation.

History, prompt discovery, undo listing, goal lookup, session listing, and
config calls return typed values directly. A consumer does not wait for a Nori
response event or use a generic operation enum. ACP-backed methods still leave
their raw response visible for schema-complete observation.

`flush_transcript()` is a write barrier: everything recorded before the call is
on disk when it returns (backed by `AcpBackend::flush_transcript` in
[`submit_and_ops.rs`](src/backend/submit_and_ops.rs); a missing recorder
flushes trivially). The remote host uses it before serving `session/load` from
the transcript.

#### Remote ACP hosting

[`remote_agent.rs`](src/remote_agent.rs) implements the acp-host `HostedAgent`
trait over `HarnessHandle` as `HarnessRemoteHost`, and re-exports the server
types so frontends reach the whole remote surface through
`nori_harness::remote_agent` without importing `nori-acp-host`. An embedding
frontend creates one host for its own application lifetime; the harness does
not keep a process-global host or own listener policy.

- `attach(handle, nori_home)` follows a newly launched session through a
  `subscribe_events` subscription, replacing any previously followed session.
  Embedders that use this path call it immediately after `launch_session` so
  the subscription registers ahead of startup events.
- The TUI instead calls `attach_started(handle, nori_home, started)` only after
  an active session publishes `SessionStarted`. This seeds the outward identity
  and working directory from the already-consumed start event, then subscribes
  from that commit boundary onward. During an agent switch the current session
  remains attached until the candidate reaches this event, so candidate
  preparation, failure, or cancellation cannot replace it.
- With multi-session TUI navigation, `set_default_started` stages the visible
  session for the next legacy subscription without replacing the current
  controller's attached session. Committed identities are remembered for
  background widgets too, so the most recently started session cannot override
  the on-screen default. `clear_default_session` leaves the next controller
  unattached when the visible session has not started or has ended, without
  retargeting an already connected controller.
- Host attachment and listener lifetime are independent. The TUI keeps the
  app-owned host attached while runtime remote control is disabled, allowing a
  later enable to expose the current session without relaunching it. Stopping
  the listeners disconnects the controller but does not shut down the host or
  harness.
- The outward ACP session id is the stable Nori conversation id (transcript
  id), captured from `SessionStarted`. Downstream swaps that continue the
  conversation (compact, restore) stay invisible to remote clients; forwarded
  `session/update` notifications have their session ids rewritten to the
  outward id. A fork mints a new conversation id, so the host closes the
  remote connection and a reconnecting client rediscovers the forked session
  through `session/list`.
- `session/load` flushes the transcript, loads it from disk through
  `TranscriptLoader`, and projects it with
  `transcript_to_replay_session_events`, keeping only session notifications
  and restamping them with the outward id.
- Live `session/update` notifications—including canonical user prompt chunks
  and `working`/`idle` lifecycle hints—are forwarded from the shared harness
  fan-out without content translation; only their outward session id is
  rewritten, so correlation and observer lifecycle survive the boundary. The
  transport still has one controller, while the fan-out remains the common
  source for the TUI and future bounded observers.
- Remote prompt admission is serialized with the session runtime and rejects
  with ACP error `-32015` while a turn is active or a local prompt is already
  queued. Local submissions keep the ordinary harness queue. The admission
  barrier provides a fast rejection before prompt hooks and snapshots, while
  the reducer repeats the same decision at the state transition boundary.
- Turn ownership is tracked by harness request id after a remote prompt is
  admitted. An outcome that races ahead of registration is claimed from a
  small bounded buffer. Only remote-owned turns forward their final response,
  `RequestFailed`, and delegated permission requests to the controller; local
  turns expose their content and lifecycle notifications but keep responses
  and permission decisions with the local frontend.
- Prompt registration runs in a detached host task, so dropping the calling
  WebSocket task during disconnect does not abandon the admission operation or
  leave the host permanently busy. Each registration carries a monotonic
  submission token; completion may clear pending prompt and cancel state only
  while that token still matches. Session replacement invalidates the token,
  while controller reconnection leaves the host-owned registration running to
  completion.
- Remote `session/cancel` targets a harness request id rather than whichever
  turn happens to be active. If cancel arrives while an admitted remote prompt
  is still awaiting request-id registration, the host remembers it and sends
  the request-scoped cancel immediately after registration. The reducer applies
  that cancel only when the same request still owns the active turn, so a late
  cancel cannot affect a later local turn. With no pending or registered remote
  prompt, remote cancel is a no-op.
- When the remote controller detaches or is replaced, its unanswered delegated
  requests are answered with a cancelled permission outcome so they cannot
  wedge the agent.

#### Managed remote hosting

[`ManagedRemoteHost`](src/managed_remote.rs) shares one command boundary
between the WebSocket `/sessions` adapter and managed ACP stdio. It lists
manager records and journal cursors (each row also carries `family_status`,
the record's `SessionManager::family_status`, else its own status, plus the
additive display fields `reply_preview`, `waiting_request`, `idle_seconds`,
`context_used`, and `context_size` from the replay journal and `model` from the record), resolves explicit conversation IDs before
using the frontend's selected default, and opens one-session or all-session
subscriptions. Its `_nori/session/event` notifications retain the complete
manager envelope, including Nori events; legacy ACP projection stays separate.
The [wire contract](../../docs/remote-control.md) defines request parameters,
acknowledgements, and reconnect errors.

Prompt admission calls `HarnessHandle::prompt_with_meta_if_idle` and returns
the harness request ID; completion remains in the envelope stream. Permission
answers validate the session/request pair against manager pending requests and
serialize managed answer claims until the event pump observes resolution.
They use the remote responder label, so TUI and remote observers see the same
`RequestResolved`. Managed disconnect does not cancel a turn or resolve its
pending permission: the continuing session can be controlled after reconnect.

The background [`HostService`](../cli/src/host/service.rs) composes this
adapter with manager lifecycle operations for new, close, cancel, and resume.
It owns the manager independently of every attached terminal. Transport
authentication and controller/watcher admission live below this layer in
[`acp-host`](../acp-host/src/remote/host_control.rs); the harness remains the
authority for prompt, permission, transcript, and session state. The
`ManagedClient` re-export lets an attached frontend consume this transport
without importing the ACP host crate directly.

With managed recovery and private recording enabled, the binding keeps each
recorder open through the checkpoint barrier. The writer hashes bytes appended
since its last successful sync. A writeback error may surface only once, so on
retry the writer checks the file length, rewrites the unsynced suffix at its
original offsets, verifies its digest and syncs the file again. A length or
digest mismatch fails the barrier. [Recorder sync retry](src/transcript/sync_retry.rs)

#### Goal ownership and MCP routing

Thread goals are Nori-owned live state. The harness is the authority for their
objective, status, and continuation lifecycle; an agent's native goal store is
not interchangeable with that state. The backend-owned `nori-client` MCP server
is the agent-facing control plane: agents read the goal with `get_goal` from the
`nori-client` MCP server and change it with `update_goal` from the `nori-client`
MCP server. Completion uses the exact status `complete`; genuine impasses use
`blocked`.

[`nori_client_mcp.rs`](src/backend/nori_client_mcp.rs) serves streamable HTTP
at an ephemeral loopback `/mcp` endpoint protected by a per-server bearer token.
The `rmcp` service disables legacy session mode and SSE keep-alives, preserving
stateless transport while sharing harness-owned goal state across requests.
It explicitly advertises MCP `2024-11-05`; upgrading the SDK does not change
that protocol contract. The newer `discover` method returns method-not-found,
preserving `initialize` as the connection boundary that marks the agent
connected, publishes capabilities, and enables hidden goal continuations.
Dropping the server aborts its listener task.

This ownership boundary is repeated in the active-goal context, automatic
continuation prompt, and `nori-client` MCP server instructions. Before ending a
goal turn, the agent is instructed to read the Nori-owned goal, update it only
with `update_goal` from the `nori-client` MCP server when complete or blocked,
and verify the returned status. Native or unqualified `create_goal`, `get_goal`,
and `update_goal` tools must not control Nori continuation.

#### Request routing

ACP permission requests that require a consumer decision are emitted as
`SessionEvent::Acp(AcpEvent::Request { request_id, request })`. A consumer
responds through `respond_to_agent` with the same ID and a schema-native success
or error. Each handle carries a responder label (`with_responder`, default
`runtime::DEFAULT_RESPONDER` = `client`; the remote host labels its attached
handle `remote_agent::REMOTE_CONTROL_RESPONDER` = `remote_control`) that the
session manager uses to say who answered. `AskForApproval::Never` resolves permission requests inside the
harness, and setup-time permission requests are cancelled internally because no
consumer session exists yet. Filesystem requests are handled in
`nori-acp-host`; they do not appear as duplicate delegated requests. Terminal
and extension families are not currently advertised.

#### Lifecycle and failures

Harness phases are `Idle`, `Loading`, `Prompting`, and `Cancelling`. For a
locally submitted prompt, the returned ID, the `Prompting`/`Cancelling` phase
ID, and the final `AcpEvent::Response` ID are the same ACP wire request ID. The
host emits that `Prompting` phase before the first ACP notification, response,
or delegated request attributable to the prompt. It does not resend the next
prompt to absorb a cancel-tail response. A successful empty `EndTurn` response
is terminal for that one prompt.

An active local prompt or load owns request-scoped updates until its response.
Without one, the reducer accepts, preserves, and projects each update as
unowned activity. The first non-metadata update in an unbounded unowned burst
emits `Received update with no active local request`; later updates do not
repeat the warning until a local prompt or load starts. A Nori
`SessionInfoUpdate` with status `working` begins bounded observer activity, and
status `idle` ends it and resets warning eligibility. Content inside that
envelope is still unowned but does not emit the warning. The warning and
observer state never create a synthetic request or `Prompting` phase, invent
attribution, drain the queue, or prevent projection. User, agent, thought,
plan, and tool updates all follow that rule; unowned tool snapshots retain
`owner_request_id = None`, and an unknown tool update is normalized from a
default tool call rather than discarded. This logic lives in
[`session_reducer.rs`](src/backend/session_reducer.rs) and
[`session_runtime_driver.rs`](src/backend/session_runtime_driver.rs).

Raw ACP session metadata remains public. The harness recognizes only the
optional `_meta.nori.status` values needed to bound observer activity when no
local request is active, and also publishes those hints around turns it
admits. Status metadata does not complete a prompt, return a stop reason, end
cancellation, or change local request state; the ACP prompt response remains
the sole completion authority for the initiator. Presentation behavior is
described in [`nori-tui`](../tui/docs.md).

Session end reasons are:

- `Shutdown` for an explicit harness shutdown;
- `Closed` after a successful ACP close response;
- `ConnectionLost` for unexpected transport or child loss;
- `SpawnFailed` when no session could be established; and
- `TimedOut` when a lifecycle watchdog owns the terminal outcome.

Explicit local shutdown uses immediate ACP process-group cleanup. Cloud exit
supplies a short child grace so `nori-handroll cloud-acp` can process stdin EOF
as a detach before the same forced-cleanup path runs.

ACP method errors remain `AcpEvent::Response { response: Err(..) }`. A failed
prompt additionally emits a correlated `NoriEvent::RequestFailed` with the same
transport-assigned wire request ID and a `Retryable` or `Fatal` disposition.
`SessionPhaseChanged::Prompting` precedes both events. Product consumers use
the classified failure to complete prompt and loop lifecycle handling, while
the raw response remains available for protocol observation. Errors from other
ACP methods remain raw responses only; failures that prevent an ACP response,
such as unexpected connection loss, also emit `RequestFailed`. Successful close
ordering is the ACP close response, then `SessionEnded(Closed)`, then stream
closure.
Unexpected loss emits `RequestFailed` for affected work and
`SessionEnded(ConnectionLost)`; an active prompt failure carries that prompt's
exact ACP wire request ID. The relay then stops and aborts the private reducer
task. A frontend may remain open to show the terminal state.
Explicit `shutdown()` aborts the retained reducer and relay tasks during
teardown so those session-owned tasks cannot outlive the harness.

[`HarnessHandle`](src/runtime.rs) clones share a normal-teardown marker for
their process. It is set while handling shutdown or close, retained on success,
reset on failure, and also set when the relay observes `Shutdown` or `Closed`.
Command-channel and response-channel closure use a private typed error;
`is_expected_shutdown_error` requires both that type and the normal-teardown
marker. Frontends can therefore omit late command failures caused by ordinary
child cleanup without hiding an actual backend error, even one returned before
shutdown began. Unexpected process loss does not establish normal teardown,
and a replacement process receives a fresh marker.

#### Compaction, branching, and session swap

`compact()` has two paths, chosen in `harness/src/backend/submit_and_ops.rs`. If
the connected agent advertises a native `compact` slash command (checked against
the reducer's `available_commands`), `/compact` is forwarded as an ordinary
in-session turn under `QueuedPromptKind::NativeCompact`: the agent compacts its
own context with no session swap and no summary re-injection, and turn
completion emits only the `ContextCompacted { summary: None }` divider. Native
detection depends on the agent re-advertising `compact` after the session
bootstrap window, because setup drops bootstrap-time `SessionUpdate`s. When no
native command is advertised, `/compact` falls back to summarize-and-swap: a
hidden summarization prompt captures the agent's summary into
`pending_compact_summary` (prepended to the next prompt), then the active
session is replaced with a brand-new one.

The session-replacement machinery is shared in
`harness/src/backend/session_swap.rs`. `swap_active_session(SessionSwapMode)`
re-assembles MCP servers, re-registers the backend-owned `nori-client` MCP
server, obtains a replacement session id, commits, swaps the active
`session_id`, rebroadcasts capabilities, and rolls back the goal-MCP connected
flag on failure. The mode selects how the replacement id is obtained:
`NewAfterCompact` calls `connection.create_session` (summarize-and-swap
fallback), while `ForkFromHead { from }` calls `connection.fork_session` in
`nori-acp-host`.

`branch()` implements branch-at-head. It is capability-gated on
`session_capabilities.fork` (errors "This agent does not support branching"
otherwise), requires the session to be idle, then forks the current head and
swaps the active session to the forked id. The original session stays
resumable. `HarnessCommand::Branch` and `handle.branch()` expose it on the
runtime; `AgentCapabilitiesView.session_fork` surfaces the fork capability to
consumers.

`ForkFromHead` (and only that mode) also forks the transcript via
`fork_transcript`: the active recorder is flushed and left frozen on disk, and a
fresh conversation is created with `TranscriptRecorder::new_forked`, seeded from
the parent's entries (via `read_seed_entries`) and stamped with
`SessionMeta.forked_from = <parent conversation id>` and the new ACP session id.
The backend's `transcript_recorder` and `conversation_id` are interior-mutable
cells (`Arc<RwLock<…>>`); the fork swaps both to the child before emitting
`NoriEvent::SessionForked`. The event-forwarding task in `runtime.rs` re-reads
the recorder cell per event rather than capturing it once, so post-fork entries
record into the child and never corrupt the frozen parent.

#### Private reduction and transcripts

ACP update reduction in `harness/src/normalized/` is private implementation
state. It assembles streaming messages and tools, tracks live phase/config/
usage, and supports persistence and product behavior. It is neither exported
from `nori-protocol` nor delivered as a second public event vocabulary.

ACP `UsageUpdate` values retain the schema-native context usage payload.
[`context_timeline`](src/context_timeline/docs.md) derives immutable prompt
start/end checkpoints and observed-growth segments from those notifications
and Nori lifecycle boundaries. `HarnessHandle::context_timeline().await`
returns a detached live snapshot; `Transcript::context_timeline()` uses the
same reducer over saved evidence. The runtime reduces each event before
publishing it, so a consumer that receives a boundary can query its checkpoint.
These are Harness read APIs, not a second usage protocol or an ACP extension.

Transcript discovery remains a provider fallback for the TUI's latest-usage
summary: `TranscriptTokenUsage.last_context_tokens` reports the latest context
fill, while the detected `AgentKind` supplies the maximum window size. Those
estimates never become ACP timeline observations. The footer's default and
atomic context segments share their resolved latest-usage state.

Transcript schema v3 records:

- session metadata;
- explicit user-input records when the queued prompt becomes active, because
  input travels client to agent; and
- exact public `SessionEvent` records in source order.

It does not write a duplicate derived assistant or `ClientEvent` record beside
the raw ACP notification. Historical request/response envelopes may be retained
for inspection, but the public replay body contains only historical ACP
notifications in source order. A canonical raw user-message sequence supersedes
the stable user record with the same message id during replay, preserving
attachments without rendering the prompt twice. Stable records without a raw
counterpart—including older transcripts—are still projected to ACP user
notifications at their recorded positions. Replay never re-executes a
historical request or completes a live request from a historical response.
Client-side fallback after a failed server-side load is available only when
`SessionResume` carries a local transcript. If the failed load has already
emitted partial history, the harness emits an Agent replay batch followed by a
distinct Transcript fallback batch; it never combines two sources under one
marker. A resume without a local transcript must preserve the requested remote
session identity, so load failure is terminal rather than calling
`session/new`. Private v2 compatibility types remain in the transcript loader;
storage enums are not exported. `transcript::load_transcript_from_path` is the
public entry for reading a saved transcript file; the TUI uses it to resume a
failed session from its manager record's transcript path. Public readers use
`Transcript::records()` and
`TranscriptRecord` for conversation events, or `Transcript::context_timeline()`
for checkpoint history. Resume into a new conversation archives the local raw
timeline evidence with its original observation identity and time, separately
from recorded agent replay. Same-ID crash resume instead uses
[`TranscriptRecorder::resume_same_session`](src/transcript/recorder.rs) to
atomically replace the canonical file with a complete seed and updated ACP
identity. Successful native load with conversation content supplies that seed,
plus archived local timeline evidence; fallback or empty native history keeps
the existing local entries. The runtime publishes seeded history replay without
recording it twice and drains the writer before publishing `SessionEnded`, so
an immediate resume reads the completed prior stream. Continuation-recorder
failure ends activation rather than dropping persistence. Fork copies the
parent transcript and retains its observation identities. See the
[transcript format](../../docs/transcripts.md) for storage and replay details.

#### Session manager and envelopes

[`session_manager`](src/session_manager/mod.rs) is the multi-session layer.
Its public type `SessionManager` is cheap to clone; clones share one registry.
The registry itself ([`registry.rs`](src/session_manager/registry.rs)) is pure
state with no I/O, so status and family rules are unit-tested without agent
processes.

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

- **Start.** `start` takes an agent (already `Prepared`, or a `Prepare` spec to
  spawn one), a `SessionStart`, an optional parent, and a `SessionSource`
  (`User`, `Fork`, or `Session`). The manager reserves a capacity slot, mints
  the conversation ID before launch, and passes it down through
  `AcpBackendConfig.conversation_id`; the transcript recorder uses it as the
  transcript ID. It returns a `ManagedSession` with the ID, a `HarnessHandle`,
  and the session's own envelope stream. Start fails with `CapacityReached`,
  `ShuttingDown`, `UnknownParent`, or `AgentAlreadyActivated`.
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
- **Replay.** [`replay.rs`](src/session_manager/replay.rs) journals envelopes
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

  The [delegation driver](src/session_manager/delegation_driver.rs) marks a
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
  then reserves capacity before launching the replacement.
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
- **Close and shutdown.** `close(id, child_grace)` cancels delegated descendants and shuts the
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
  `ShutdownReport` of closed, timed-out, and failed sessions. A session whose
  launch was in flight when admission closed is shut down as soon as it
  registers its handle.
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

The [session manager](src/session_manager/mod.rs) owns delegation admission,
approval, parent/child links, result delivery, and descendant lifetime. Both terminal `/delegate` commands and the session-bound `delegate`
tool on the existing authenticated
[`nori-client` HTTP MCP server](src/backend/nori_client_mcp.rs) enter this same
manager path. The tool takes an agent and task; its authority comes from the
calling session, not a caller-supplied parent ID. Delegation requires the
parent's resolved `multi_session` flag. Direct single-session launch behavior
is unchanged.

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
  stable. [State reduction](src/session_manager/delegation_state.rs) republishes
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
  permission request, [state reduction](src/session_manager/delegation_state.rs)
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
  through its normal prompt queue. The [result driver](src/session_manager/delegation_driver.rs)
  retains that delivery until the parent's correlated ACP `PromptResponse`,
  not merely queue admission. If the process crashes first, stable-ID resume
  resubmits the unacknowledged result; the parent may therefore see it again.
  A child that delegates waits for its descendants and resulting continuation
  turns before returning its own final result. Terminal completion stops the
  child process and releases its capacity slot while retaining the registry
  record and transcript.
- A result prompt never titles the parent that receives it. The
  [replay journal](src/session_manager/replay.rs) skips user-message chunks
  that are result prompts when it derives the first-prompt title, and takes
  that title for a parent from the task of a user-origin `DelegationChanged` it requested, so a
  session whose first input is `/delegate` is named by that task. The
  first-prompt summary hook in [user input](src/backend/user_input.rs) likewise
  ignores result prompts, leaving the summary for the user's own first prompt.
- Explicit parent close cancels descendants and invalidates pending delegation
  approvals, including when the parent has already failed. A parent process
  crash leaves children running; results wait for the parent's stable-ID
  resume. The held result queue belongs to the live manager. Managed checkpoints
  can carry historical delegation metadata as described above. A child crash produces a failure
  result without failing its parent.
- Admission and explicit close share a mutex so a start cannot escape a parent
  close. Result work is claimed under manager state, then prompts and process
  shutdown run outside that lock. Prompt request IDs, outstanding descendants,
  and in-flight result submissions determine completion; an idle phase alone
  does not prove that a delegated task has finished. The MCP authority follows
  its registered process through a weak manager reference, avoiding an
  ownership cycle and rejecting calls from an ended process.

Agents without HTTP MCP support can still receive children started by the
user's `/delegate`; an agent-side stdio delegation tool is not provided.
The [managed remote contract](../../docs/remote-control.md#delegation) defines
controller authorization and request payloads.

#### Browser sessions and profile tiers

`backend/browser_session.rs` (`#[cfg(unix)]`) launches a headed Chrome browser
with CDP (Chrome DevTools Protocol) remote debugging enabled so the ACP agent
can script it via the shell tool. It is invoked by the TUI's `/browser` slash
command. `BrowserSession::launch_and_store(mode)` finds a Chrome/Chromium binary
via the `which` crate (searching `google-chrome-stable`, `google-chrome`,
`chromium-browser`, `chromium`), then spawns it with `--remote-debugging-port=0`
(OS-assigned port) against a `--user-data-dir` resolved from the requested
`BrowserProfileMode`. It parses the CDP WebSocket URL from Chrome's stderr (the
`DevTools listening on ws://...` line) with a 15-second timeout. Helpers
`parse_cdp_ws_url()` and `extract_cdp_port()` handle stderr parsing and port
extraction; `compose_agent_prompt()` builds the CDP endpoint message sent to the
agent.

`BrowserProfileMode` (defined in `nori-config`, persisted as the top-level
`browser_profile` key and defaulting to `Throwaway`) selects which profile to
launch against. `backend/browser_profile.rs::resolve_profile_dir()` turns that
choice into a concrete directory and owns its lifetime:

- **`Throwaway`** (the secure default): a fresh `tempfile::TempDir`, wiped on
  shutdown. Shares no cookies/logins/settings with the user's real Chrome.
- **`Persistent`**: a nori-owned `<nori_home>/browser-profile` directory,
  created if absent and left on disk so logins survive across launches, while
  staying isolated from the user's real Chrome.
- **`System`**: the user's real default Chrome (or Chromium) profile —
  `~/.config/google-chrome` on Linux, `~/Library/Application Support/Google/Chrome`
  on macOS — with all their logins and cookies. Because this reuses the real
  profile, an already-running Chrome silently hands the launch off and never
  exposes CDP; the launch detects that failure and returns a precise
  "fully quit Chrome, then run `/browser` again" hint (`SYSTEM_PROFILE_BUSY_HINT`).

Only `Throwaway` owns a `TempDir`; `Persistent` and `System` resolve to
`ProfileDir::Keep(PathBuf)` and are never deleted. The `ProfileDir` is stored as
the `BrowserSession`'s last field so, on drop, the child is killed (the manual
`Drop` SIGTERMs Chrome, and `kill_on_drop` backs it) before a throwaway profile's
temp dir is removed.

#### Shell command structure

[`bash`](src/bash.rs) exposes `summarize(src)`, a display-oriented view of a
shell command line built on tree-sitter-bash. It returns top-level
`ShellSegment`s (each with the `&&`, `||`, or `;` separator joining it to the
previous one; newlines count as `;`) made of pipeline `ShellStage`s. A stage
carries byte ranges into the source for its command name, the script an
interpreter runs (`python3 tools/gen.py`), and its whole text including
redirections, plus whether arguments follow, the line count of a heredoc body,
and whether output is redirected to a file other than `/dev/null`. `VAR=`
assignment prefixes are not part of the name range.

Parsing is tolerant: segments inside tree-sitter `ERROR` nodes are still
collected, and `None` means no command was recovered at all. The module makes
no safety or execution decisions; its consumer is the TUI's tool call
rendering ([`exec_cell`](../tui/src/exec_cell/render.rs)), which uses the
ranges to summarize commands, bold command names, and decide whether an
Execute call is read-only.

### Things to Know

- All public ACP types are reached through `nori_protocol::acp`.
- Do not move private reduction back into the protocol crate or expose it as a
  compatibility facade.
- The harness receives a resolved config; it must not reload ambient config
  during preparation, refresh, launch, or resume.
- `PreparedAgent` is the unique ownership boundary between an initialized
  connection and a session-bound backend; unsupported listing, an empty
  catalog, and inspection failure are separate outcomes.
- Refreshing a prepared agent may replace session-time configuration only;
  agent identity, cwd, ACP proxy settings, and default model remain fixed to
  the prepared process and transport.
- Approval, sandbox, MCP, trust, and shell policy belong to `nori-config`.
- Nori thread-goal completion is proven only by the Nori-owned status returned
  by `update_goal` from the `nori-client` MCP server; similarly named native
  agent tools cannot stop harness continuation.
- Consumers should correlate raw requests and responses only by the supplied
  ACP `RequestId`.
- The legacy `HarnessRemoteHost` exposes one attached session; the managed
  host exposes the manager registry. Legacy `--remote` WebSocket endpoints
  share one controller slot (last connect wins); the background host instead
  admits one controller and independent watchers. Bounded consumers close on overflow without
  blocking sessions. [Legacy remote tests](tests/remote_host.rs) exercise the
  ACP projection; [managed WebSocket tests](tests/managed_remote.rs) compare
  reconnect delivery against the manager's envelope stream.
- Session manager invariants: one agent process per session; conversation ID
  = transcript ID = envelope `conversation_id`; envelope `seq` starts at 1 per
  conversation with no gaps, a crash resume continues it, and a fork starts a
  new sequence; a capacity slot is held from before spawn until the session
  ends, and a fork moves it to the child.
- A dead agent process fails only the session it carries. Other sessions keep
  their status, sequence, and slot.
- The manager record does not mirror approval-policy changes made later
  through the raw `HarnessHandle`. Prepared agents held outside the manager
  (such as a TUI warm-up before any session exists) do not count against the
  cap.
- Legacy remote and local frontends share notifications but not request
  ownership. Local prompts may queue; remote prompts reject while busy,
  legacy remote cancel is scoped to the registered harness request id, and only the remote submitter
  receives a correlated prompt outcome. Prompt registration belongs to
  `HarnessRemoteHost`, so controller replacement cannot strand admission state,
  while hosted-session replacement invalidates the old registration token.

Created and maintained by Nori.
