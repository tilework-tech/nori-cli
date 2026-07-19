# Protocol Unification at the Harness Boundary

Status: **implemented and verified through `919dbb39`**
Created: 2026-07-18

This specification replaces the earlier assumption that Nori should adopt the
Codex `Event`/`EventMsg`/`Op` vocabulary. The target is one public protocol
entry point, an ACP-faithful agent/client boundary, and a much smaller set of
Nori-owned events for behavior that ACP does not define.

The configuration prerequisite landed in #545. The implementation branch was
based on commit `45754205`; the approved design was recorded in `baade242`.
Commit `cf3868f9` made ACP canonical at the harness boundary and deleted the
Codex protocol crates; `00ba480f` corrected view-only transcript filtering for
lifecycle-bearing v3 files; `298fc785` hid transcript storage compatibility;
`4d5116a9` preserved wire request correlation and setup ordering; `8c958b7f`
made agent exit terminate retained session tasks; and `1445e30e` deleted the
noncanonical cancel-tail retry fixtures. Commit `919dbb39` closed the remaining
public-boundary ordering gaps for setup, prompt start, replay fallback, raw ACP
errors, and child loss. The historical dependency, variant, and persistence
inventories below are retained as the deletion record.

## Historical grounding

This design was the unfinished protocol slice of the crate-boundary refactor,
not a replacement for the boundaries that had already landed:

- #520 documented the target leaf/host/harness/frontend layering. Its import
  audit correctly found Codex protocol on the live path, but its original
  verdict was to adopt that vocabulary.
- #524 removed the `codex-core` protocol re-export detour, making actual
  protocol dependencies visible.
- #526 severed the old ACP runtime's dependency on `codex-core`; #527 extracted
  sandboxed execution; #528 extracted `nori-config`.
- #529 extracted `nori-acp-host`; #530 made the TUI/CLI import configuration
  from its owner; #531 moved session orchestration into the harness; and #532
  named that product `nori-harness`.
- #533 prepared the central crates for publishing and specified the transcript
  format.

Those changes established the physical crate boundaries, but left two protocol
models on the harness hot path: the Codex `Event`/`EventMsg`/`Op` bus and
`nori-protocol::ClientEvent`, which normalizes ACP schema values into a second
Nori representation. At the original baseline commit `500ee202`, 13 crates still directly
depend on `codex-protocol`, and schema imports are split across
`nori-protocol`, `nori-harness`, and `nori-acp-host`.

At the implementation baseline `45754205`, #545 had removed `codex-otel` and
there were 12 direct `codex-protocol` dependents. It also established
`nori-config` as the semantic owner of Nori configuration, which resolves the
configuration prerequisite without making configuration part of the session
protocol.

The landed hard cut removed all twelve edges, `nori-rs/protocol`, and
`nori-rs/app-server-protocol`. Only `nori-protocol` directly depends on the ACP
schema. Client-side schema imports use `nori_protocol::acp`; among client-side
product crates, only `nori-acp-host` directly uses the higher-level ACP SDK.

This spec supersedes only the old protocol verdict. The landed host/harness/TUI
layering remains the foundation: the protocol cleanup makes that boundary
truthful and usable by non-TUI embedders.

## 1. Decision

The refactor is a hard cut:

- delete the `codex-protocol` crate rather than rename it, wrap it, or retain
  deprecated aliases;
- make `nori-protocol` the only import path for ACP schema types throughout the
  client-side product graph;
- preserve ACP's agent/client semantics without projecting them into parallel
  Nori enums;
- keep Nori-owned protocol types limited to harness lifecycle, persistence,
  queueing, goals, undo, hooks, history, and UI/UX concerns that ACP does not
  define;
- keep normalization, reduction, and presentation logic out of
  `nori-protocol`; and
- replace the Codex submission/event bus with typed harness methods plus one
  outward session event stream.

The desired result is both conceptually smaller and a net deletion. Embedders
of `nori-harness` should learn ACP plus a small Nori extension vocabulary, not
ACP, a Codex compatibility protocol, and a second normalized ACP model.

## 2. Ownership rule

Use this test whenever a type is proposed for `nori-protocol`:

1. If ACP defines the interaction between an agent and a client, ACP owns it.
2. If Nori adds behavior around an ACP session and ACP does not define that
   behavior, Nori may own the smallest type that expresses it.
3. If a type exists only to reduce events, assemble state, translate content,
   persist an implementation detail, or render a UI, it belongs in the harness,
   persistence adapter, or frontend—not in the protocol crate.
4. If a type describes Codex/OpenAI model internals rather than the Nori harness
   boundary, delete it or localize it to its one remaining private consumer.

Examples:

| Concern                                                                                   | Owner                                    |
| ----------------------------------------------------------------------------------------- | ---------------------------------------- |
| Agent text, thoughts, plans, tool calls, usage, modes, config options, available commands | ACP schema                               |
| Agent permission, filesystem, terminal, and elicitation requests                          | ACP schema                               |
| Prompt/load/setup/list/close responses and ACP errors                                     | ACP schema                               |
| Queue state, compaction, goals, undo, user shell, hooks                                   | Nori protocol                            |
| Session reducer, open-message assembly, request tracking                                  | `nori-harness` private implementation    |
| Tool titles, icons, grouped output, friendly labels                                       | frontend presentation                    |
| Approval, sandbox, shell-environment, MCP server, and trust configuration                 | `nori-config`                            |
| MCP authentication status                                                                 | `codex-rmcp-client`                      |
| Residual Codex login/model/app-server vocabulary                                          | its remaining private owner, or deletion |

## 3. Dependency choke point

`nori-protocol` directly depends on and publicly re-exports the ACP schema
crate:

```rust
// nori-protocol/src/lib.rs
pub use agent_client_protocol_schema as acp;
```

All schema imports in Nori client-side product crates use that public path:

```rust
use nori_protocol::acp;
use nori_protocol::acp::v1::SessionUpdate;
```

Direct imports of `agent_client_protocol_schema` outside `nori-protocol` are
forbidden in the Nori client-side product graph. This gives Nori one place to
change ACP library versions, vendor a schema implementation, or replace the
current crate without rewriting every client consumer.

`nori-acp-host` is the only client-side product exception at the SDK layer. It
may directly depend on and import higher-level runtime traits and transport
machinery from `agent-client-protocol`, while importing schema types through
`nori_protocol::acp` like every other product crate. ACP agent implementations
and conformance fixtures, including `mock-acp-agent`, may use the runtime SDK
directly on the agent side; they are not client protocol consumers. No frontend
may depend directly on either ACP crate or on `nori-acp-host`.

The intended dependency shape is:

```text
nori-cli / nori-tui
          |
          v
     nori-harness --------> nori-protocol::acp
          |                         ^
          v                         |
     nori-acp-host -----------------+
          |
          +----> agent-client-protocol (runtime SDK only)

nori-protocol ----> agent-client-protocol-schema
```

## 4. Public session event API

The outward stream uses source-owned nesting:

```rust
pub enum SessionEvent {
    Acp(AcpEvent),
    Nori(NoriEvent),
}
```

This preserves provenance at the type level. Consumers can exhaustively handle
Nori additions without interleaving them with ACP variants, and ACP schema
growth remains inside the ACP branch.

### 4.1 Raw ACP envelopes

The ACP branch contains the three directional envelopes an ACP agent can emit
toward its client:

```rust
pub enum AcpEvent {
    Notification(acp::v1::AgentNotification),
    Request {
        request_id: acp::v1::RequestId,
        request: acp::v1::AgentRequest,
    },
    Response {
        request_id: acp::v1::RequestId,
        response: Result<acp::v1::AgentResponse, acp::v1::Error>,
    },
}
```

These variants are schema-complete for the outward agent-to-client event
boundary:

| Current interaction                                                               | New envelope                                                                     |
| --------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| Session content, plans, tool calls, usage, commands, modes, config, session info  | `AcpEvent::Notification(AgentNotification::SessionNotification(...))`            |
| Permission request                                                                | `AcpEvent::Request { request: AgentRequest::RequestPermissionRequest(...), .. }` |
| Filesystem, terminal, elicitation, or extension request delegated to the embedder | `AcpEvent::Request { .. }`                                                       |
| Prompt completion and stop reason                                                 | `AcpEvent::Response { response: Ok(AgentResponse::PromptResponse(...)), .. }`    |
| Initialize, authenticate, setup, load, list, resume, config, and close completion | the corresponding `AgentResponse` variant                                        |
| ACP-defined failure                                                               | `AcpEvent::Response { response: Err(...), .. }`                                  |

The public event receiver must exist before the host issues its first ACP call.
Initialization and current session-setup responses are buffered in original
order if construction awaits bootstrap, so a consumer that receives the
harness only after bootstrap still observes them. On every launch path those
current responses precede `SessionStarted`. This includes a failed
`session/load` response followed by the fallback `session/new` response. Setup
responses are never placed between `ReplayStarted` and `ReplayFinished`; those
markers identify historical notification replay only. The relay must not block
bootstrap on an unavailable consumer or silently discard early responses.

The implementation preserves the current routing policy: advertised filesystem
requests are handled by the host, permission requests that require a consumer
decision are delegated, and terminal/extension request families are not
advertised. `AskForApproval::Never` resolves permission requests internally;
setup-time permission requests are cancelled internally because the consumer
session does not exist yet. A delegated request crosses the event boundary
unchanged. Its eventual response returns through a typed harness method using
the same `RequestId`; it is not converted into a Codex approval type. If later
configuration makes another request family delegable, it must use this same raw
envelope rather than adding a Nori mirror.

The stream is faithful, not necessarily wire-byte-identical. Transport framing
and JSON-RPC bookkeeping stay in `nori-acp-host`; the schema envelope and
request identity remain intact.

### 4.2 Nori-only notifications

The landed Nori branch is:

```rust
pub enum NoriEvent {
    SessionStarted(SessionStarted),
    SessionPhaseChanged(SessionPhase),
    SessionEnded(SessionEnded),
    QueueChanged(QueueSnapshot),
    ReplayStarted(ReplayStarted),
    ReplayFinished,
    ContextCompacted(ContextCompactedEvent),
    GoalChanged(Option<ThreadGoal>),
    CapabilitiesChanged(NoriCapabilities),
    Undo(UndoEvent),
    UserShell(UserShellEvent),
    HookOutput(HookOutput),
    PromptSummaryUpdated(PromptSummary),
    Notice(Notice),
    RequestFailed(RequestFailure),
}
```

This is the exhaustive outer enum for the hard cut. Its payload fields and
derive sets were reduced to real channel, persistence, and consumer needs.

Variant semantics are fixed:

- `SessionStarted` announces that the Nori harness session exists. ACP setup
  responses still appear as raw ACP responses.
- `SessionPhaseChanged` reports Nori's lifecycle projection of idle, loading,
  prompting, and cancelling. It does not copy request content or completion
  data. Every non-idle phase carries the exact `RequestId` assigned to the
  owning ACP wire request; the harness does not manufacture a surrogate phase
  ID. `Cancelling` retains the active prompt's ID because `session/cancel` is a
  notification, not a replacement request.
- `SessionEnded` distinguishes explicit shutdown, ACP close, connection loss,
  spawn failure, and timeout with a Nori-owned reason.
- `QueueChanged` is the current Nori prompt/operation queue snapshot.
- `ReplayStarted` and `ReplayFinished` bracket only historical
  `AcpEvent::Notification` values, in transcript or agent source order. They do
  not bracket current setup responses, Nori lifecycle events, historical ACP
  requests, or historical ACP responses. There is no Nori `ReplayEntry`
  mirror.
- `ContextCompacted` reports Nori's compaction lifecycle/result.
- `GoalChanged(Some(...))` reports the current persistent Nori goal;
  `GoalChanged(None)` reports that it was cleared.
- `CapabilitiesChanged` contains only Nori harness capabilities. ACP agent and
  session capabilities stay in ACP responses and notifications.
- `Undo`, `UserShell`, and `HookOutput` report Nori-owned operations that ACP
  does not standardize.
- `PromptSummaryUpdated` reports Nori's background prompt-summary result. It is
  not an ACP agent message or context-compaction result.
- `Notice` reports a nonfatal user-visible diagnostic or advisory. It does not
  imply that the owning request failed.
- `RequestFailed` means the harness or transport failed before an ACP response
  existed. ACP errors and successful prompt completion remain
  `AcpEvent::Response`.

`NoriEvent` is notification-oriented. It is not a second general-purpose
request/response bus.

#### Resolved live Nori extension gaps

The baseline producer/consumer audit found two outward Nori concerns that do
not fit the approved first-pass variants without lying about their semantics:

- prompt-summary generation is a separate background-agent result used by the
  TUI footer and auto-worktree naming. It is neither context compaction nor hook
  output; and
- nonfatal user-visible notices include slow-cancellation advice, compaction
  advice, reducer out-of-phase warnings, and hook failures. They are not
  `RequestFailed`, because the owning request may continue or complete.

The approved additions are `PromptSummaryUpdated(PromptSummary)` and
`Notice(Notice)`. Each payload carries only the user-visible summary or message
required by current consumers. Do not overload `RequestFailed`,
`ContextCompacted`, or `HookOutput`. These additions narrowly complete the Nori
branch; they do not reopen the ACP envelopes or introduce ACP mirrors.

### 4.3 Commands and queries

Client-to-agent ACP operations are typed methods on the harness handle. Nori
commands and queries are also typed methods. The landed public surface is:

```rust
impl HarnessHandle {
    pub async fn prompt(
        &self,
        content: Vec<acp::v1::ContentBlock>,
    ) -> anyhow::Result<acp::v1::RequestId>;
    pub async fn shutdown(&self) -> anyhow::Result<()>;
    pub async fn respond_to_agent(
        &self,
        request_id: acp::v1::RequestId,
        response: std::result::Result<acp::v1::ClientResponse, acp::v1::Error>,
    ) -> anyhow::Result<()>;
    pub async fn cancel(&self) -> anyhow::Result<()>;

    pub async fn add_history(&self, text: String) -> anyhow::Result<()>;
    pub async fn history_entry(
        &self,
        log_id: i64,
        offset: i64,
    ) -> anyhow::Result<Option<HistoryEntry>>;
    pub async fn search_history(
        &self,
        max_results: i64,
    ) -> anyhow::Result<Vec<HistoryEntry>>;
    pub async fn custom_prompts(&self) -> anyhow::Result<Vec<CustomPrompt>>;
    pub async fn compact(&self) -> anyhow::Result<()>;
    pub async fn undo(&self) -> anyhow::Result<()>;
    pub async fn undo_snapshots(&self) -> anyhow::Result<Vec<UndoSnapshot>>;
    pub async fn undo_to(&self, index: i64) -> anyhow::Result<()>;
    pub async fn run_user_shell(&self, command: String) -> anyhow::Result<()>;
    pub async fn set_approval_policy(
        &self,
        policy: nori_config::AskForApproval,
    ) -> anyhow::Result<()>;

    pub async fn goal(&self) -> anyhow::Result<Option<nori_protocol::ThreadGoal>>;
    pub async fn set_goal(
        &self,
        objective: String,
        status: Option<nori_protocol::ThreadGoalStatus>,
    ) -> anyhow::Result<nori_protocol::ThreadGoal>;
    pub async fn set_goal_status(
        &self,
        status: nori_protocol::ThreadGoalStatus,
    ) -> anyhow::Result<nori_protocol::ThreadGoal>;
    pub async fn clear_goal(&self) -> anyhow::Result<()>;

    pub async fn get_session_config(
        &self,
    ) -> Option<Vec<acp::v1::SessionConfigOption>>;
    pub async fn set_session_config_option(
        &self,
        config_id: String,
        value: String,
    ) -> anyhow::Result<Vec<acp::v1::SessionConfigOption>>;
    pub async fn list_sessions(
        &self,
        cwd: std::path::PathBuf,
    ) -> anyhow::Result<Vec<acp::v1::SessionInfo>>;
    pub async fn close_session(&self) -> anyhow::Result<()>;
}
```

In particular, history lookup/search, custom prompt discovery, undo snapshot
listing, session listing, config mutation, and session close do not return
correlated `NoriEvent` variants. A CLI/headless JSON-RPC adapter may add its own
correlation IDs at that external transport boundary without imposing them on
Rust embedders or the ordered session stream.

Prompt is a streaming operation: submission returns the exact wire
`RequestId`, zero or more raw ACP notifications may follow, and completion
arrives as the correlated `AcpEvent::Response`. One accepted
`HarnessHandle::prompt` call issues exactly one ACP `session/prompt` request.
Cancellation does not authorize the transport to absorb a later response and
resend the user's next prompt; the former cancel-tail resend heuristic was
deleted. A successful prompt response with `StopReason::EndTurn` is terminal
even when no content notification preceded it. Initial create/load/resume
selection belongs to `SessionLaunchSpec` rather than a post-launch
`load_session` handle method.
Query-style ACP methods return their typed result directly; their raw
`AcpEvent::Response` also remains visible so the boundary stays
schema-complete. Those callers never have to consume or correlate the stream to
finish the call. Nori does not add a duplicate response variant.

## 5. `nori-protocol` is types only

The crate contains public boundary types and no stateful reducer or
presentation code.

The hard cut moved business logic out:

- ACP-to-view reduction remains private in the harness only where runtime state
  needs it; the former public `ClientEventNormalizer` and ACP mirrors are gone.
- `SessionRuntime`, active request state, message assembly, persisted reducer
  state, and queued-prompt internals live in private
  `nori-harness` modules.
- frontend projections such as tool kind inference, friendly invocation
  descriptions, grouping, and command display live privately in `nori-tui`.
- transcript serialization uses private versioned storage types. Schema v3
  intentionally stores exact public `SessionEvent` payloads, but the storage
  enum and compatibility shapes are not public Harness API.

A small public `SessionPhase` value remains because phase changes are part of
`NoriEvent`. It is a boundary value, not the session state machine.

## 6. Compatibility and persistence

The Rust API has no compatibility facade: `codex_protocol::{Event, EventMsg,
Op}` and the former normalized `nori_protocol::ClientEvent` disappeared in the
hard cut.

User-owned transcripts are different from Rust API compatibility. Version-2
Nori transcript records contain serialized normalized `ClientEvent` values plus
persistence-owned user and assistant records. The harness loader continues to
read those records through private compatibility types. The recorder now writes
schema v3: explicit user-input records plus the exact public `SessionEvent`
stream. The versioned storage enum is private and is not re-exported from either
`nori-harness` or `nori-protocol`; public readers use `Transcript::records()`
and `TranscriptRecord`.

Replay of a new transcript emits:

1. `NoriEvent::ReplayStarted`;
2. only historical `SessionEvent::Acp(AcpEvent::Notification(...))` values, in
   original order; explicit v3 user records are projected to ACP user-message
   notifications at their recorded positions; and
3. `NoriEvent::ReplayFinished`.

Recorded ACP request/response envelopes retain `RequestId` for historical
inspection. Replay intentionally does **not** re-emit either envelope: a
historical delegated request must never ask the embedder to perform an operation
again, and a historical response must not complete a live request. The public
transcript loader exposes stored records separately from replay projection.

User input is a persistence concern because it travels client-to-agent and
therefore cannot be recovered from an agent-to-client-only event stream. V3
records it explicitly and does not also write a derived assistant-message or
normalized-client-event record alongside the raw ACP notification. That avoids
two canonical copies of the same output. Because v3 stores exact public events,
changes to request-payload persistence or redaction require a separately
designed transcript policy rather than a silent protocol projection.

## 7. Historical deletion and rehoming inventory

The top-level `ClientEvent`, `Op`, and `EventMsg` variant ledgers below are
exhaustive at the original `500ee202` baseline and were mechanically rechecked
at implementation baseline `45754205`: all 18 `ClientEvent`, 20 `Op`, and 51
`EventMsg` variants were accounted for before deletion. This section is the
review ledger that authorized the hard cut, not a description of types that
remain in the workspace.

### 7.1 Former `nori-protocol::ClientEvent`

Delete ACP mirrors:

- `ToolSnapshot`
- `ApprovalRequest`
- `MessageDelta`
- `PlanSnapshot`
- `AgentCommandsUpdate`
- `SessionUpdateInfo`
- `SessionConfigUpdate`
- `SessionModeChanged`

Replace rather than carry forward:

- `SessionPhaseChanged` → `NoriEvent::SessionPhaseChanged`
- `PromptCompleted` → `AcpEvent::Response`; transport/runtime failure only →
  `NoriEvent::RequestFailed`
- `LoadCompleted` → `AcpEvent::Response`
- `QueueChanged` → `NoriEvent::QueueChanged`
- `ContextCompacted` → `NoriEvent::ContextCompacted`
- `ReplayEntry` → raw ACP notifications bracketed by `ReplayStarted` and
  `ReplayFinished`
- `SessionCapabilitiesChanged` → raw ACP capabilities plus
  `NoriEvent::CapabilitiesChanged` for Nori-only capabilities
- `ThreadGoalUpdated` and `ThreadGoalCleared` →
  `NoriEvent::GoalChanged(Option<ThreadGoal>)`
- `Warning` → an ACP error/notification when ACP owns the condition; otherwise
  the pending Nori notice/diagnostic decision for user-visible nonfatal
  conditions, or a private diagnostic only when no consumer needs it

Delete the associated ACP projection vocabulary:

- `AgentCommandInfo`, `AgentCommandsUpdate`
- `AgentCapabilitiesView`, `SessionCapabilitiesView`,
  `NoriClientCapabilitiesView`, and the ACP-derived portion of
  `CommandAvailability`
- `SessionConfigUpdate`, `SessionUpdateInfo`, `SessionUpdateKind`,
  `SessionModeChanged`
- `ReplayEntry`
- `MessageDelta`, `MessageStream`
- `PlanSnapshot`, `PlanEntry`, `PlanStatus`
- `ToolSnapshot`, `ToolKind`, `ToolPhase`, `ToolLocation`
- `ApprovalRequest`, `ApprovalSubject`, `ApprovalOption`, `ApprovalOptionKind`
- `Invocation`, `Artifact`, `FileChange`, `FileOperation`
- `ClientEventNormalizer`

Move the following implementation types from `nori-protocol` into private
`nori-harness` modules rather than exposing them as protocol:

- `ActiveRequestKind`, `ActiveRequestState`
- `OpenMessage`
- `PersistedSessionState`, `SessionInfoState`, `SessionUsageState`
- `TranscriptMessage`, `TranscriptRole`
- `QueuedPromptKind`, `QueuedPrompt`
- `SessionRuntime`

### 7.2 Former `codex_protocol::Op`

Delete or replace every variant:

| `Op` variant             | Disposition                                                                            |
| ------------------------ | -------------------------------------------------------------------------------------- |
| `Interrupt`              | ACP cancel method                                                                      |
| `UserInput`              | typed ACP prompt method with ACP content blocks                                        |
| `ThreadGoalGet`          | typed Nori goal query                                                                  |
| `ThreadGoalSet`          | typed Nori goal mutation                                                               |
| `ThreadGoalClear`        | typed Nori goal mutation                                                               |
| `UserTurn`               | delete Codex model-turn vocabulary                                                     |
| `OverrideTurnContext`    | rehome post-refactor runtime/config mutation; do not put it in protocol                |
| `ExecApproval`           | ACP client response to the original permission request                                 |
| `PatchApproval`          | ACP client response to the original permission request                                 |
| `ResolveElicitation`     | ACP client response when it is an ACP request; delete the current dead MCP legacy path |
| `AddToHistory`           | typed Nori history command                                                             |
| `GetHistoryEntryRequest` | typed Nori history query                                                               |
| `SearchHistoryRequest`   | typed Nori history query                                                               |
| `ListCustomPrompts`      | typed Nori query                                                                       |
| `Compact`                | typed Nori operation plus Nori lifecycle event                                         |
| `Undo`                   | typed Nori operation plus `NoriEvent::Undo`                                            |
| `UndoList`               | typed Nori query                                                                       |
| `UndoTo`                 | typed Nori operation plus `NoriEvent::Undo`                                            |
| `Shutdown`               | harness shutdown/ACP close methods plus `SessionEnded`                                 |
| `RunUserShellCommand`    | typed Nori operation plus `NoriEvent::UserShell`                                       |

Delete `Submission` and `Event` with the queue protocol. The harness API owns
command dispatch and the `SessionEvent` receiver owns outward events.

### 7.3 Former `codex_protocol::EventMsg`

Replace ACP-owned variants with raw ACP notifications, requests, or responses:

- `TokenCount`
- `AgentMessage`, `UserMessage`, `AgentMessageDelta`
- `AgentReasoning`, `AgentReasoningDelta`, `AgentReasoningRawContent`,
  `AgentReasoningRawContentDelta`, `AgentReasoningSectionBreak`
- `SessionConfigured`
- `McpToolCallBegin`, `McpToolCallEnd`
- `WebSearchBegin`, `WebSearchEnd`
- `ExecCommandBegin`, `ExecCommandOutputDelta`, `ExecCommandEnd`
- `ViewImageToolCall`
- `ExecApprovalRequest`, `ElicitationRequest`, `ApplyPatchApprovalRequest`
- `PatchApplyBegin`, `PatchApplyEnd`, `TurnDiff`
- `PlanUpdate`
- `TurnAborted`
- ACP-owned cases of `Error`, `Warning`, and `StreamError`

Replace Nori-owned variants as follows:

- `ContextCompacted` → `NoriEvent::ContextCompacted`
- user-shell uses of `TaskStarted` and `TaskComplete` →
  `NoriEvent::UserShell`; prompt lifecycle uses ACP responses and Nori phase
- `UndoStarted`, `UndoCompleted`, `UndoListResult` → `NoriEvent::Undo` plus a
  typed list query
- `GetHistoryEntryResponse`, `SearchHistoryResponse`,
  `ListCustomPromptsResponse` → typed method results
- `ShutdownComplete` → `NoriEvent::SessionEnded`
- `PromptSummary` → the pending Nori prompt-summary decision; it is not
  compaction or hook output
- `HookOutput` → `NoriEvent::HookOutput`
- harness/transport failures represented by `Error` before any ACP response
  exists → `NoriEvent::RequestFailed`
- user-visible nonfatal `Warning` cases → the pending Nori notice decision
- diagnostics with no public consumer → private logging/diagnostics
- dead `StreamError` cases → deletion after the refreshed producer audit

Delete the following no-op or legacy variants after the refreshed producer
audit confirms the current result:

- `RawResponseItem`
- `ItemStarted`
- `ItemCompleted`
- `AgentMessageContentDelta`
- `ReasoningContentDelta`
- `ReasoningRawContentDelta`
- `McpStartupUpdate`
- `McpStartupComplete`
- `DeprecationNotice`
- `BackgroundEvent`

At the implementation baseline, the first six were explicitly ignored by the
TUI and had no producer. `AgentReasoningSectionBreak` had a consumer but no
known producer. The final four appeared consumer-only with no live ACP producer.
These findings were rechecked before deletion.

### 7.4 Former `codex-protocol` modules

| Module                                    | Baseline symbols/variants                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                              | Disposition                                                                                              |
| ----------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| `account`                                 | `PlanType`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             | auth/app-server concern; rehome or delete, never absorb into Nori session protocol                       |
| `config_types`                            | `ReasoningEffort`, `ReasoningSummary`, `Verbosity`, `ForcedLoginMethod`, `TrustLevel`, `McpServerConfig`, `McpServerTransportConfig`, `SandboxMode`, shell environment types, and related config enums                                                                                                                                                                                                                                                                                                                                                                                                                 | move trust/MCP/sandbox/shell policy to `nori-config`; localize or delete residual model/login vocabulary |
| `models`                                  | `ResponseInputItem::{Message, FunctionCallOutput, McpToolCallOutput, CustomToolCallOutput}`; `ContentItem::{InputText, InputImage, OutputText}`; `ResponseItem::{Message, Reasoning, LocalShellCall, FunctionCall, FunctionCallOutput, CustomToolCall, CustomToolCallOutput, WebSearchCall, GhostSnapshot, CompactionSummary, Other}`; `LocalShellStatus::{Completed, InProgress, Incomplete}`; `LocalShellAction::Exec`; `WebSearchAction::{Search, OpenPage, FindInPage, Other}`; `ReasoningItemReasoningSummary::SummaryText`; `ReasoningItemContent::{ReasoningText, Text}`; shell/function-output payload structs | delete Codex/OpenAI Responses vocabulary or localize a genuinely live private app-server consumer        |
| `items`                                   | `TurnItem::{UserMessage, AgentMessage, Reasoning, WebSearch}`, `UserMessageItem`, `AgentMessageContent::Text`, `AgentMessageItem`, `ReasoningItem`, `WebSearchItem`, legacy conversions                                                                                                                                                                                                                                                                                                                                                                                                                                | delete; ACP session updates are canonical                                                                |
| `user_input`                              | `UserInput::{Text, Image, LocalImage}`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 | ACP content blocks; local image preprocessing belongs in harness                                         |
| `plan_tool`                               | `StepStatus`, `PlanItemArg`, `UpdatePlanArgs`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          | delete in favor of ACP plan types                                                                        |
| `parse_command`                           | `ParsedCommand::{Read, ListFiles, Search, Unknown}`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    | private harness/frontend parsing and presentation                                                        |
| `num_format`                              | numeric formatting helpers                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             | frontend/common presentation, not protocol                                                               |
| `custom_prompts`                          | `CustomPrompt`, `CustomPromptKind`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     | may remain Nori query-domain values; prefix/presentation constants stay in frontend                      |
| `message_history`                         | `HistoryEntry`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         | may remain a Nori persistence/query-domain value                                                         |
| `conversation_id`                         | `ConversationId`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       | persistence-owned Nori transcript identity, distinct from ACP `SessionId`                                |
| `approvals`                               | `SandboxRiskLevel::{Low, Medium, High}`, `SandboxCommandAssessment`, `ExecApprovalRequestEvent`, `ApplyPatchApprovalRequestEvent`, `ElicitationRequestEvent`, `ElicitationAction::{Accept, Decline, Cancel}`                                                                                                                                                                                                                                                                                                                                                                                                           | delete ACP mirrors; approval policy is owned by `nori-config`                                            |
| `protocol/history` and legacy event files | `InitialHistory`, `ResumedHistory`, `RolloutItem`, `RolloutLine`, `SessionMeta`, `SessionMetaLine`, `CompactedItem`, `TurnContextItem`, `SessionSource`, `SubAgentSource`, legacy tags/constants, and `HasLegacyEvent`                                                                                                                                                                                                                                                                                                                                                                                                 | private legacy transcript decoder only where required; otherwise delete                                  |
| token/rate/final-output helpers           | `TokenUsage`, `TokenUsageInfo`, `RateLimitSnapshot`, `RateLimitWindow`, `CreditsSnapshot`, `FinalOutput` and formatting logic                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          | ACP usage data is canonical; CLI formatting belongs in frontend                                          |

`ReviewDecision` becomes ACP's permission outcome. Codex `FileChange` becomes ACP
tool/diff content. `GitInfo`, MCP authentication status, goals, and hooks are
reviewed as Nori runtime/query types, not carried over merely because they
lived in `codex-protocol` at the baseline.

### 7.5 Former dependency consumers

At implementation baseline `45754205`, 12 workspace crates directly depend on
`codex-protocol`:

- `codex-app-server-protocol`
- `codex-common`
- `codex-core`
- `codex-linux-sandbox`
- `codex-rmcp-client`
- `codex-sandbox`
- `codex-windows-sandbox`
- `nori-acp-host`
- `nori-cli`
- `nori-config`
- `nori-harness`
- `nori-tui`

Every edge was deleted, rehomed, or replaced before the crate was removed. The
app-server protocol was deleted; sandbox policy moved to its semantic owner in
`nori-config` rather than polluting the public harness protocol.

## 8. Landed post-configuration ownership

The post-#545 audit and hard cut established:

1. `nori-config` owns approval, trust, MCP server, sandbox, and shell-environment
   configuration, including the resolved policies consumed by sandbox crates;
2. `codex-rmcp-client` owns computed MCP authentication status;
3. Codex/OpenAI reasoning and app-server protocol vocabulary was deleted;
   remaining login concerns are localized to their actual owners;
4. `nori-protocol` receives none of those values merely to break a dependency
   cycle;
5. Nori's persisted conversation identity belongs to the transcript/history
   domain and remains distinct from ACP's `SessionId`; and
6. the two live event gaps are the approved `PromptSummaryUpdated` and `Notice`
   variants in §4.2.

Moving resolved sandbox and shell policy to `nori-config` deliberately changes
the old sandbox documentation's claim that the execution crate has no config
dependency. Adding a new neutral policy crate would increase architecture and
dependency count for no current benefit; this hard cut uses the existing
semantic owner.

## 9. Implementation and verification state

Implemented through `919dbb39`:

- [x] `nori-rs/protocol`, `codex-protocol`, and app-server protocol were deleted.
- [x] `nori-protocol` is the sole direct ACP schema dependency and re-exports it
      as `nori_protocol::acp`.
- [x] `nori-acp-host` alone uses the ACP SDK among client-side product crates;
      the mock agent uses it legitimately on the agent side.
- [x] `nori-protocol` contains boundary types only.
- [x] The harness emits raw ACP envelopes and exactly fifteen Nori outer event
      variants, including prompt summaries and notices.
- [x] Delegated permission requests retain `RequestId` and round-trip as raw ACP;
      filesystem requests remain host-handled.
- [x] Typed harness commands and queries replaced the generic operation bus.
- [x] Transcript v3 records explicit user turns and exact public session events;
      private v2 compatibility remains behind `Transcript::records()`.
- [x] The TUI and black-box harness integration use the same public boundary.
- [x] View-only transcript selection counts user turns rather than lifecycle
      records.
- [x] Prompt and load phases carry their exact ACP wire `RequestId`; one Harness
      prompt produces one ACP prompt request.
- [x] An empty successful ACP `EndTurn` response terminates that prompt; it is
      never swallowed as a reason to retry.
- [x] Current setup responses precede `SessionStarted`, including failed-load
      and fallback-new responses, and never enter replay brackets.
- [x] Setup draining follows the actual ACP response rather than a fixed event
      count, so interleaved notifications and default-model config responses
      also precede `SessionStarted`.
- [x] Bootstrap ACP errors remain raw responses before `SpawnFailed`; the
      harness does not mirror them as `NoriEvent::RequestFailed`.
- [x] `Prompting` with the exact wire request ID is the first public event
      attributable to an accepted prompt, including before delegated requests.
- [x] Replay brackets contain historical ACP notifications only and preserve
      notification order. A failed load with partial agent history and a local
      transcript emits separate Agent and Transcript replay batches.
- [x] Unexpected child loss correlates `RequestFailed` with the active prompt's
      exact ACP wire request ID.
- [x] Explicit shutdown aborts retained reducer and relay tasks.

- [x] The repository-prescribed workspace/all-target tests, format, fix, strict
      Clippy, workspace check, binary build, and PTY coverage pass on the
      documentation-complete branch. Exact commands are recorded in the
      implementation plan and draft PR.
- [x] Final stale-term and dependency audits find no live Codex protocol edge,
      direct schema import outside `nori-protocol`, or current-doc claim that the
      removed control plane remains public.

## 10. Approved decision record (verbatim and chronological)

The excerpts below preserve the exact prompts and proposals that established
this design. Typos and punctuation are intentionally unchanged.

### 10.1 Initial boundary requirement

User:

> We focus on point number two there and clean up the unification of the protocol into one place. This should require that all protocol imports go through the nori protocol and we fully absorb whatever variants or domain objects are captured in the codex protocol into the nori protocol crate. This is a very important layer of the API to get correct because anyone who uses the nori Harness to embed ACP-like behavior or agents in some headless manner or into another interface will have to inherit whatever assumptions or mistakes or decisions we make with a nori protocol.

### 10.2 Hard cut and deletion gate

User:

> definitely a hard cut

User:

> agreed I would also favor deletion, but you can give me a final list of exactly which variants those are. So I can sanity check the deletion first.

Assistant proposal:

> Deletion will be a review gate: I’ll provide the exact modules, types, and enum variants proposed for removal, with their remaining consumers, before deleting any of them.

### 10.3 Narrow Nori protocol around ACP

User:

> Agreed, yes, but if we're getting into the point of changing anything with the actual protocol contents or variants, we absolutely need to narrow down the scope of the Nori protocol and lean more heavily on the ACP schema. The Nori protocol might be useful for things that are out of scope of ACP or for handling UI/UX concerns. that ACP can't address. However, overall, I'm very cognizant that our current protocol implementation is a bit too complicated and the whole point of using the ACP ecosystem is it much more carefully standardizes the exact boundary between interface on one side and agent on the other.

Assistant proposals:

> ACP owns all agent↔client semantics.

> `nori-protocol` re-exports the ACP schema rather than copying it.

> Nori-owned types exist only for harness lifecycle, persistence, queueing, goals, undo, hooks, history, and presentation concerns that ACP does not define.

> Translation or UI enrichment belongs in the harness/TUI, not in protocol types.

> Codex/OpenAI model vocabulary is deleted rather than adopted.

### 10.4 Re-export boundary and source-owned nesting

User:

> That's definitely the best way to do this, because if we ever need to switch out our ACP library choice, use a different implementation, vendor some of the ACP code, whatever the situation is, we can do that easily if we export it from Nori protocol for every other crate to use. So definitely follow that. Your proposed rule about ACP owning agent to client semantics, and then protocol re-exports the ACP schema, and then the Nori specific types are just those other concerns not yet defined, like hardest lifecycle, persistence, etc, etc. That definitely sounds right to me. It definitely matters a lot how we do the exact details of that enum for the type of the Nori protocol. So let's co-design that.

Assistant proposal:

```rust
pub enum SessionEvent {
    Acp(AcpEvent),
    Nori(NoriEvent),
}
```

User:

<!-- prettier-ignore -->
> Agreed, that shape looks good.  Source-owned nested enums are definitely the way to go. Do those three variants of ACP event really encapsulate all of the ACP-like data that we currently hand back and forth across the event boundary between the Nori harness and any potential consumer like the current TUI or an eventual CLI-only headless exact version? like a third-party integration, for example, putting a Nori harness into a VS Code extension or other place.

### 10.5 Raw ACP envelopes

Assistant proposal:

```rust
pub enum AcpEvent {
    Notification(acp::v1::AgentNotification),
    Request {
        request_id: acp::v1::RequestId,
        request: acp::v1::AgentRequest,
    },
    Response {
        request_id: acp::v1::RequestId,
        response: Result<acp::v1::AgentResponse, acp::v1::Error>,
    },
}
```

User:

> I think that looks right to me.

### 10.6 Nori notifications and typed queries

Assistant proposal:

```rust
pub enum NoriEvent {
    SessionStarted(SessionStarted),
    SessionPhaseChanged(SessionPhase),
    SessionEnded(SessionEnded),
    QueueChanged(QueueSnapshot),
    ReplayStarted(ReplayStarted),
    ReplayFinished,
    ContextCompacted(ContextCompacted),
    GoalChanged(Option<ThreadGoal>),
    CapabilitiesChanged(NoriCapabilities),
    Undo(UndoEvent),
    UserShell(UserShellEvent),
    HookOutput(HookOutput),
    RequestFailed(RequestFailure),
}
```

Assistant proposals:

> I recommend keeping `NoriEvent` notification-oriented rather than turning it into another general request/response bus.

> Those should be typed async methods on the harness handle. A headless JSON-RPC adapter can add correlation IDs at its own boundary without forcing every Rust embedder to coordinate unrelated query responses through the ordered session event stream.

User:

> That's perfect. Exactly right.

### 10.7 Implementation prerequisite

User:

> There's a significant configuration rework going on in the CLI, so to avoid conflicts with our refactor work we need to wait for that to land.
