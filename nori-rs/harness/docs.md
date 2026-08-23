# Noridoc: nori-harness

Path: @/nori-rs/harness

### Overview

`nori-harness` is the headless, embeddable runtime for one Nori ACP session. It
composes the low-level ACP host with session lifecycle, private reduction,
transcripts, queueing, history, goals, undo, hooks, user-shell operations, and
worktree behavior. It has no terminal dependency.

The public embedding contract is deliberately small: launch a session, control
it through typed methods, and consume one ordered stream of
`nori_protocol::SessionEvent` values. Additional bounded subscribers can follow
the same ordered stream without disturbing the primary consumer.

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

The remote ACP transport (`@/docs/specs/remote-acp-transport.md`) is the other
headless consumer. `nori-acp-host` owns the WebSocket server and defines the
`HostedAgent` trait; [`remote_agent.rs`](src/remote_agent.rs) implements that
trait over `HarnessHandle`, keeping the dependency direction
`nori-harness -> nori-acp-host`. Remote mutations pass through the same typed
handle as local ones, so hooks, transcripts, goals, and permission policy all
still apply, and the TUI observes remote-driven activity on its own stream.

### Core Implementation

#### Launch and event stream

A frontend constructs `SessionLaunchSpec` with one resolved `Arc<NoriConfig>`,
CLI version, optional product/session context, and optional `SessionResume`.
Product context has HTTP-MCP and non-HTTP-MCP variants. After ACP
initialization reveals the connected agent's capabilities, the harness selects
the matching variant and prepends it to the first locally submitted prompt
only. This keeps source identity common across ACP agents while reserving MCP
fallback guidance for agents that cannot use Nori's HTTP MCP affordances.
`launch_session(spec)` returns `LaunchedSession`, containing a `HarnessHandle`
and the session event receiver.

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

Initialization, session setup/load, prompts, config changes, list, and close
retain their raw ACP responses and original request IDs in the stream. The
harness buffers bootstrap events until the consumer can receive them,
preserving current-response order without making construction depend on a
concurrently draining UI. Historical load notifications preserve their own
relative order inside replay. Current setup responses always precede
`SessionStarted`. When a local transcript permits failed-load fallback, this
means initialize, failed `session/load`, and successful fallback `session/new`
responses are all observable before the Nori session-start event; none is
labeled as replay. Without a transcript, a failed `session/load` is surfaced
and session setup ends without creating an empty replacement session.
Setup follows the actual method response rather than assuming a fixed event
count, so interleaved notifications and a configured default-model response
also retain their transport order before session start. A raw ACP setup error
precedes `SessionEnded(SpawnFailed)` and is not mirrored as
`NoriEvent::RequestFailed`.

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

Prompt content is canonical as an ordered `Vec<ContentBlock>` and is forwarded
to ACP without regrouping or reordering blocks. Text used by hooks, display,
and persistence is a derived projection only and does not replace or
reconstruct the wire prompt.

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
`nori_harness::remote_agent` without importing `nori-acp-host`.

- `attach(handle, nori_home)` follows a newly launched session through a
  `subscribe_events` subscription, replacing any previously followed session.
  It must be called immediately after `launch_session` so the subscription
  registers ahead of the session's startup events.
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
- Turn ownership is tracked by harness request id: `prompt` submits without
  holding the state lock (a queued prompt resolves only when issued), then
  registers the returned id as remote-owned; an outcome that raced ahead of
  the registration is claimed from a small unclaimed-outcome buffer instead.
  Only remote-owned turns forward their final response, `RequestFailed`, and
  delegated permission requests to the remote controller. A locally initiated
  turn's permission requests stay with the TUI.
- When the remote controller detaches or is replaced, its unanswered delegated
  requests are answered with a cancelled permission outcome so they cannot
  wedge the agent.
- `set_active_host` / `active_host` are the process-global install point (set
  once at remote-mode startup); the TUI uses it to attach every launched
  session.

#### Goal ownership and MCP routing

Thread goals are Nori-owned live state. The harness is the authority for their
objective, status, and continuation lifecycle; an agent's native goal store is
not interchangeable with that state. The backend-owned `nori-client` MCP server
is the agent-facing control plane: agents read the goal with `get_goal` from the
`nori-client` MCP server and change it with `update_goal` from the `nori-client`
MCP server. Completion uses the exact status `complete`; genuine impasses use
`blocked`.

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
or error. `AskForApproval::Never` resolves permission requests inside the
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
unowned activity. The first non-metadata update in an unowned burst emits
`Received update with no active local request`; later updates do not repeat the
warning until a local prompt or load starts. The warning does not create a
synthetic request or `Prompting` phase, invent attribution, or prevent
projection. User, agent, thought, plan, and tool updates all follow that rule;
unowned tool snapshots retain `owner_request_id = None`, and an unknown tool
update is normalized from a default tool call rather than discarded. This
logic lives in
[`session_reducer.rs`](src/backend/session_reducer.rs) and
[`session_runtime_driver.rs`](src/backend/session_runtime_driver.rs).

The public stream forwards raw ACP session metadata unchanged. The harness does
not interpret metadata as prompt completion or publish an agent-turn completion
event, so presentation of unowned updates or an agent-owned turn cannot drain
the queue, end cancellation, or change request state. Optional presentation
hints are interpreted only by the TUI, as described in
[`nori-tui`](../tui/docs.md).

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

ACP `UsageUpdate` values retain both current and maximum context tokens for
consumers such as the TUI footer. Transcript discovery remains a provider
fallback: `TranscriptTokenUsage.last_context_tokens` reports the latest context
fill, while the detected `AgentKind` supplies the maximum window size. The TUI
uses live ACP usage when present and otherwise combines those transcript values;
the default and atomic footer context segments share that resolved state.

Transcript schema v3 records:

- session metadata;
- explicit user-input records, because input travels client to agent; and
- exact public `SessionEvent` records in source order.

It does not write a duplicate derived assistant or `ClientEvent` record beside
the raw ACP notification. Historical request/response envelopes may be retained
for inspection, but the public replay body contains only historical ACP
notifications in source order. Explicit v3 user records are projected to ACP
user-message notifications at their recorded positions. Replay never
re-executes a historical request or completes a live request from a historical
response. Client-side fallback after a failed server-side load is available
only when `SessionResume` carries a local transcript. If the failed load has
already emitted partial history, the harness emits an Agent replay batch
followed by a distinct Transcript fallback batch; it never combines two
sources under one marker. A resume without a local transcript must preserve
the requested remote session identity, so load failure is terminal rather than
calling `session/new`. Private v2 compatibility types remain in the transcript
loader; storage enums are not exported. Public readers use
`Transcript::records()` and `TranscriptRecord`.

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

### Things to Know

- All public ACP types are reached through `nori_protocol::acp`.
- Do not move private reduction back into the protocol crate or expose it as a
  compatibility facade.
- The harness receives a resolved config; it must not reload ambient config
  during launch, resume, or probing.
- Approval, sandbox, MCP, trust, and shell policy belong to `nori-config`.
- Nori thread-goal completion is proven only by the Nori-owned status returned
  by `update_goal` from the `nori-client` MCP server; similarly named native
  agent tools cannot stop harness continuation.
- Consumers should correlate raw requests and responses only by the supplied
  ACP `RequestId`.
- The remote host exposes exactly one session (the running one) and keeps
  exactly one remote consumer; a newer subscription replaces the current one
  (last connect wins), and a consumer whose bounded queue overflows is dropped,
  which closes its connection. Remote-host behavior is exercised in
  `@/nori-rs/harness/tests/remote_host.rs` against the mock ACP agent.

Created and maintained by Nori.
