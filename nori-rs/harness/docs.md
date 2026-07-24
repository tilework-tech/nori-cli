# Noridoc: nori-harness

Path: @/nori-rs/harness

### Overview

`nori-harness` is the headless, embeddable runtime for one Nori ACP session. It
composes the low-level ACP host with session lifecycle, private reduction,
transcripts, queueing, history, goals, undo, hooks, user-shell operations, and
worktree behavior. It has no terminal dependency.

The public embedding contract is deliberately small: launch a session, control
it through typed methods, and consume one ordered stream of
`nori_protocol::SessionEvent` values.

### How it fits into the larger codebase

```text
nori-exec              nori-tui
        \                 /
         \               /
          v             v
             nori-harness
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

### Core Implementation

#### Launch and event stream

A frontend constructs `SessionLaunchSpec` with one resolved `Arc<NoriConfig>`,
CLI version, optional product/session context, and optional `SessionResume`.
`launch_session(spec)` returns `LaunchedSession`, containing a `HarnessHandle`
and the session event receiver.

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
`SessionStarted`. On a failed-load fallback this means initialize, failed
`session/load`, and successful fallback `session/new` responses are all
observable before the Nori session-start event; none is labeled as replay.
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
async fn undo() -> Result<()>;
async fn undo_snapshots() -> Result<Vec<UndoSnapshot>>;
async fn undo_to(i64) -> Result<()>;
async fn run_user_shell(String) -> Result<()>;
async fn set_approval_policy(nori_config::AskForApproval) -> Result<()>;

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

Harness phases are `Idle`, `Loading`, `Prompting`, and `Cancelling`, with the
exact ACP wire request ID on non-idle phases. A prompt call is one ACP
`session/prompt` request: the returned ID, the `Prompting`/`Cancelling` phase
ID, and the final `AcpEvent::Response` ID are the same schema value. The host
emits that `Prompting` phase before the first ACP notification, response, or
delegated request attributable to the prompt. It does not resend the next
prompt to absorb a cancel-tail response. A successful empty `EndTurn` response
is terminal for that one prompt. Session end reasons are:

- `Shutdown` for an explicit harness shutdown;
- `Closed` after a successful ACP close response;
- `ConnectionLost` for unexpected transport or child loss;
- `SpawnFailed` when no session could be established; and
- `TimedOut` when a lifecycle watchdog owns the terminal outcome.

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
response. When a failed server-side load has already emitted partial history,
the harness emits an Agent replay batch followed by a distinct Transcript
fallback batch; it never combines two sources under one marker. Private v2
compatibility types remain in the transcript loader; storage enums are not
exported. Public readers use `Transcript::records()` and `TranscriptRecord`.

### Things to Know

- All public ACP types are reached through `nori_protocol::acp`.
- Do not move private reduction back into the protocol crate or expose it as a
  compatibility facade.
- The harness receives a resolved config; it must not reload ambient config
  during launch, resume, or probing.
- Approval, sandbox, MCP, trust, and shell policy belong to `nori-config`.
- Consumers should correlate raw requests and responses only by the supplied
  ACP `RequestId`.

Created and maintained by Nori.
