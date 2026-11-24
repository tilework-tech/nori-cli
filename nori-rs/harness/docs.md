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
                nori-tui       nori host client (/acp, stdio, pipe)
                    |               |
                    |               v
                    |     RemoteAcpServer (nori-acp-host::remote)
                    v               |  ManagedAgent trait
              nori-harness  <-------+  (managed_remote.rs implements it)
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

`nori-tui` starts every session through the `SessionManager` of the host it
owns: `nori-cli` builds that host for each TUI run and stops it when the TUI
quits (see [the owned TUI host](../cli/src/host/docs.md#owned-tui-host)). The
manager calls the same launch internals as `launch_session`, so a managed
session behaves exactly like a directly launched one. The TUI serves no
remote listener; the background host (including its stdio form,
`nori exec --acp`) owns its own manager and is the only remote surface. One-shot `nori exec "prompt"` is an ACP client of that host
(a running one or an in-process one, see
[the host client](../cli/src/host/docs.md#one-shot-exec)), so it no longer
drives a `HarnessHandle` directly. The public
`launch_session`/`prepare_and_launch_session` entry points retain their direct
per-session runtime paths.

The remote ACP transport (`@/docs/remote-control.md`) is the
headless consumer. `nori-acp-host` owns the server and defines the
`ManagedAgent` trait; [`managed_remote.rs`](src/managed_remote.rs) implements
it over `SessionManager`, and [`remote_agent.rs`](src/remote_agent.rs)
re-exports that host with the transport types so frontends need not import
`nori-acp-host`. This keeps the dependency direction
`nori-harness -> nori-acp-host`. Remote mutations pass through the same typed
handles as local ones, so hooks, transcripts, goals, and permission policy all
still apply. The earlier `HarnessRemoteHost`, which served one TUI session on
an unauthenticated `/acp` endpoint, was removed; `/acp` is now the background
host's authenticated endpoint.

### Core Implementation

#### Connection preparation, session activation, and event stream

A frontend constructs `AgentPrepareSpec` with one resolved `Arc<NoriConfig>`,
the CLI version, and optional product/conversation context. `prepare_agent`
uses the bound API provider's model when present and otherwise resolves the
agent's default model. It injects the result through the agent's spawn-time channel before spawning the ACP subprocess. It then completes
`initialize`, reads the connection's capabilities, and normally calls
`session/list` when advertised. It returns an
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
agent capabilities and supplied resume data. Frontends that have already made their session choice may use
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
directory, ACP proxy/wire-recording configuration, named API binding, and default model are
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
`HarnessHandle::subscribe_events()` registers additional bounded consumers. Every consumer sees the same events in the same order.
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
`nori.dev/promptEchoId` correlation extension. Prompt metadata crosses
`HarnessHandle` intact; the harness supplies the marker when
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

[Preparation](src/backend/prepared.rs) uses the named connection resolved for the requested agent. A missing key or unsupported route fails preparation before the candidate starts. The TUI, local host sessions (including those of `nori exec`) and delegated children use this path. A delegated child resolves its own registered agent. Managed sessions start from a [binding-specific launch specification](src/managed_launch.rs). Both routes compose argv before the [process supervisor](../acp-host/src/connection/child_lifecycle.rs) takes ownership of the child and its ACP pipes.

After model injection, preparation calls [`nori-auth-providers`](../auth-providers/docs.md)' `inject_saved_keys` on a blocking task bounded by a short timeout (with `NORI_CREDENTIALS_STORE=keyring` it reads the OS keyring, which can prompt). It passes `AcpBackendConfig.prefer_api_key`, which `build_backend_config` in [`runtime.rs`](src/runtime.rs) resolves from the agent's `[auth.<agent>] prefer_api_key` ([`nori-config`](../nori-config/docs.md)). When it is false (the default) nothing is injected and the store is not read, so the agent uses its own login. When it is true, each provider key saved with `nori auth set-key` is added to the agent's launch environment, in the agent's own variable for that provider. A provider is skipped when any credential variable the agent honours for that provider is already set in the ambient environment or the agent config, the provider's base-URL variable (for example `ANTHROPIC_BASE_URL`) is set non-blank there, or its variable is removed by `launch.env_remove`; an agent bound to a named token provider receives none. A store failure or timeout is logged without values and the agent launches without saved keys. Managed launches build their environment from the admitted specification and intentionally never receive saved keys; the broker supplies their credentials.

The resolved model preference (`AcpBackendConfig.default_model`) is
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
async fn reclaim_queued_prompt(String) -> Result<Option<String>>;
async fn resume_queue() -> Result<()>;
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
flushes trivially).

Queued prompts are published as `NoriEvent::QueueChanged` snapshots built in
[`session_reducer/reclaim.rs`](src/backend/session_reducer/reclaim.rs). Each
snapshot lists user prompts only (compact, native-compact, and goal-continuation
prompts stay hidden), oldest first, as plain `prompts` texts plus `entries`
carrying the submission event ID and an `editable` flag. A prompt is editable
only when the local user typed it and its original content is all text, so
taking it back never drops an image.

`reclaim_queued_prompt(event_id)` takes one queued prompt back before it is
sent. The request is routed through
[`queue_reclaim.rs`](src/backend/queue_reclaim.rs) onto the serialized session
runtime task as `SessionRuntimeInput::ReclaimQueuedPrompt`, so it is ordered
against the reducer events that drain the queue:

| Prompt state when the request runs | Result |
| --- | --- |
| Still queued and editable | Removed exactly once; other entries keep their order and IDs; a new `QueueChanged` snapshot is published; returns the text the user typed |
| Already started, already reclaimed, unknown, or not editable | Returns `None`; queue unchanged |

The returned text is the caller's original content, never the hook, goal, or
compact context added to the wire prompt. The withdrawn prompt's pending
`prompt()` call resolves with `backend::PromptWithdrawn`.

The queue drains one prompt per prompt response with stop reason `end_turn`.
Any other ending while prompts are queued pauses the queue instead
([`session_reducer/queue_pause.rs`](src/backend/session_reducer/queue_pause.rs)):
`SessionRuntime.queue_pause` records why, every entry keeps its order and ID,
and a `QueueChanged` snapshot carrying `paused` is published. Only a queue
holding at least one user prompt pauses, so a pause is always visible to
clients; hidden work alone (a queued compact or goal continuation) is left as
it was.

| Turn ending | `QueuePause` |
| --- | --- |
| Stop reason `cancelled`, or a prompt failure while the turn was cancelling | `Interrupted` |
| Any other non-`end_turn` stop reason (`max_tokens`, `refusal`, ...) or a prompt failure | `TurnFailed` |

While paused:

- `drain_queue` sends nothing, which also holds the deferred drain that a
  settling steer would otherwise run.
- An idle-only submission (`submit_prompt_if_idle`, used by the TUI's idle
  Enter and by remote prompts) is admitted from idle: it runs ahead of the held
  entries and the queue stays paused. `prompt_is_busy` ignores a paused queue
  for the same reason.
- Reclaiming the last held user prompt clears the pause, so later follow-ups
  drain normally.

`resume_queue()` sends `InboundEvent::ResumeQueue` through
[`queue_reclaim.rs`](src/backend/queue_reclaim.rs) onto the serialized runtime.
It clears the pause; an idle session starts the next queued prompt at once
(or defers to the steer fence), and a busy one drains after its running turn
ends with `end_turn`. Draining then continues one prompt per completed turn. A
resume without a pause, or while a session load is in flight, is a no-op, and an interrupt or failure after a resume
pauses whatever is still queued.

#### Managed remote hosting

[`ManagedRemoteHost`](src/managed_remote.rs) shares one command boundary
between the WebSocket `/acp` (alias `/sessions`) adapter and managed ACP stdio. It lists
manager records and journal cursors (each row also carries `family_status`,
the record's `SessionManager::family_status`, else its own status, plus the
additive display fields `reply_preview`, `waiting_request`, `idle_seconds`,
`context_used`, and `context_size` from the replay journal and `model` from the record), resolves explicit conversation IDs before
using the frontend's selected default, and opens one-session or all-session
subscriptions. Its `_nori/session/event` notifications retain the complete
manager envelope, including Nori events.
The [wire contract](../../docs/remote-control.md) defines request parameters,
acknowledgements, and reconnect errors.

Legacy `_nori/session/prompt` admission calls `HarnessHandle::prompt_with_meta_if_idle` and returns
the harness request ID; completion remains in the envelope stream. Permission
answers validate the session/request pair against manager pending requests and
serialize managed answer claims until the event pump observes resolution.
All observers see the same `RequestResolved`, with the identity the host
assigned to the answering connection. Managed disconnect does not cancel a turn or resolve its
pending permission: the continuing session can be controlled after reconnect.

The [follow-up operation](../cli/src/host/docs.md#mid-turn-follow-ups) uses the same harness session driver for direct input and background-host controllers. It checks the expected turn, retains a receipt keyed by message ID and routes an explicit queue or guarded fallback through the existing prompt FIFO. A successful injection leaves the original prompt responsible for completion, usage and cancellation.

The background [`HostService`](../cli/src/host/service.rs) composes this
adapter with manager lifecycle operations for new, close, cancel, and resume,
and finds stored sessions through `TranscriptLoader::list_stored_sessions` and
`find_stored_session`, which scan `transcripts/by-project/*/sessions/*.jsonl`
reading only each file's metadata line and modification time.
`SessionManager::resume_stored` starts a session the manager has not run from
its transcript under the transcript's own ID, so the recorder continues the
same file; the journal numbers it from 1.
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

For a host provisioned with `[host] managed = true`, the CLI uses
[managed launches](src/managed_launch.rs) for process admission and
[the standard ACP relay](src/managed_acp.rs) for the binding's agent traffic.
When side sessions are enabled, [a router](src/managed_acp/side_sessions.rs)
keeps each side's parent identity, public ACP ID and pending request mappings
on that logical connection. Rebuilt sides occupy a process slot. An eligible
adapter hosts native forks in the parent's process. Both forms route events
outside the parent recorder and session manager.

The rebuilt worker and launch registry share an exit result. A side that
[runs without confinement](../cli/src/host/docs.md#managed-launch-control-v1)
has no namespace, and its slot can be reused when the adapter process exits.
For a confined side, after the outer Bubblewrap supervisor exits, the host signals the side's namespace init
through its pidfd and waits for exit readiness. At that point the adapter
and its clients have exited, and the slot can be reused. An unconfirmed exit
retains the slot and reports an error to its owning parent.
[Namespace exit monitor](src/managed_launch/namespace_exit.rs)

The router keeps the adapter's confirmed parent close result while it waits
for rebuilt processes to exit and native forks to close. The native relay owns
each fork until the adapter confirms closure, even if its client endpoint
has gone away. After a refusal the router can retry unfinished cleanup on the
same connection. [Parent close](src/managed_acp/parent_close.rs) ·
[Native fork cleanup](src/managed_acp/native_cleanup.rs) ·
[Managed machine protocol](../acp-host/docs.md#forking-a-read-only-side-session)

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
or error. The host records `client` for its foreground terminal and
`remote_control` for an independent controller. Both use the same pending-request
check and answer claim. `AskForApproval::Never` resolves permission requests inside the
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

The harness publishes raw ACP metadata and emits `_meta.nori.status` hints
around its own prompts. For live work observed outside a local request, it
tracks an observation ID and serializes cancellation with turn activity.
Repeated cancels for the same observation send one `session/cancel`.
A local TUI action carries the observation ID. A bare ACP cancel carries only
the session ID and targets whichever turn is active when the host admits it.

A matching broker `nori/turnId`, idle `nori/turnState` and cancelled
`nori/stopReason` confirm that the observed turn was cancelled. An unqualified
idle report closes the observation with its cancellation outcome unconfirmed.
If the agent keeps working, a timeout marks the request unconfirmed and leaves
the agent running. A locally admitted prompt still completes through its own
ACP response. See [external turn tracking](src/backend/external_turn.rs).

Session end reasons are:

- `Shutdown` for an explicit harness shutdown;
- `Closed` after a successful ACP close response;
- `ConnectionLost` for unexpected transport or child loss;
- `SpawnFailed` when no session could be established; and
- `TimedOut` when a lifecycle watchdog owns the terminal outcome.

Prompts and follow-ups submitted while the agent is still starting wait in the
runtime until it has started. If startup ends in `SpawnFailed` or `TimedOut`,
each waiting command is answered with `runtime::AgentStartFailed` (the startup
message plus the agent's ACP error code when it refused with one) rather than
being dropped. Callers such as the [background host](../cli/src/host/docs.md)
read that code, so an adapter that needs sign-in surfaces its own `-32000`
authentication error instead of a generic "did not accept the prompt".

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

See [session lifecycle and envelope delivery](src/session_manager/docs.md#session-manager-and-envelopes) for status, reconnect replay, recovery and process capacity.

#### Delegation between managed sessions

See [delegation admission, approval and result delivery](src/session_manager/docs.md#delegation-between-managed-sessions) for how the manager links parent and child sessions.

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
- The managed host exposes the manager registry; the background host admits
  one controller and independent watchers. Bounded consumers close on overflow
  without blocking sessions. [Managed WebSocket tests](tests/managed_remote.rs)
  compare reconnect delivery against the manager's envelope stream.
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
- The harness uses the submission path to decide whether the local composer
  may reclaim a queued item. For follow-ups, `HarnessHandle::local_follow_up`
  marks input from the host-established foreground pipe as local. An independently
  attached controller uses
  `HarnessHandle::follow_up`; its queued input stays read-only even if its
  displayed sender is `local`. Delegated child results also stay read-only.
- A queued prompt runs its `pre_user_prompt` hooks, consumes pending hook
  context and any compact summary, and pushes its `/undo` snapshot at
  submission, not at dispatch. Reclaiming returns the hook context that was
  pending before the prompt's own hooks ran and the compact summary, so the
  next prompt still carries them. The hooks and the orphan `/undo` snapshot are
  not undone; the resubmitted edit is a new prompt and runs its hooks again.
  Remote `nori attach` has no queue preview, so only the local TUI reclaims.

Created and maintained by Nori.
