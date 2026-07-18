# Protocol Unification at the Harness Boundary

Status: **approved core design; implementation blocked on configuration and two Nori extension decisions**
Created: 2026-07-18

This specification replaces the earlier assumption that Nori should adopt the
Codex `Event`/`EventMsg`/`Op` vocabulary. The target is one public protocol
entry point, an ACP-faithful agent/client boundary, and a much smaller set of
Nori-owned events for behavior that ACP does not define.

No production code changes belong in this documentation PR. Implementation
starts only after the active CLI configuration rework lands and the inventories
in this document have been refreshed against its result.

## Historical grounding

This design is the unfinished protocol slice of the crate-boundary refactor,
not a replacement for the boundaries that already landed:

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
Nori representation. At baseline commit `500ee202`, 13 crates still directly
depend on `codex-protocol`, and schema imports are split across
`nori-protocol`, `nori-harness`, and `nori-acp-host`.

This spec supersedes only the old protocol verdict. The landed host/harness/TUI
layering remains the foundation: the protocol cleanup makes that boundary
truthful and usable by non-TUI embedders.

## 1. Decision

The refactor is a hard cut:

- delete the `codex-protocol` crate rather than rename it, wrap it, or retain
  deprecated aliases;
- make `nori-protocol` the only import path for ACP schema types outside the ACP
  host implementation;
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

| Concern                                                                                   | Owner                                                              |
| ----------------------------------------------------------------------------------------- | ------------------------------------------------------------------ |
| Agent text, thoughts, plans, tool calls, usage, modes, config options, available commands | ACP schema                                                         |
| Agent permission, filesystem, terminal, and elicitation requests                          | ACP schema                                                         |
| Prompt/load/setup/list/close responses and ACP errors                                     | ACP schema                                                         |
| Queue state, compaction, goals, undo, user shell, hooks                                   | Nori protocol                                                      |
| Session reducer, open-message assembly, request tracking                                  | `nori-harness` private implementation                              |
| Tool titles, icons, grouped output, friendly labels                                       | frontend presentation                                              |
| Authentication, approval policy, sandbox policy, model defaults                           | configuration/runtime owner selected by the configuration refactor |

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
Initialization and session-creation responses are buffered in original order if
construction awaits bootstrap, so a consumer that receives the harness only
after bootstrap still observes them. The relay must not block bootstrap on an
unavailable consumer or silently discard early responses.

The harness may satisfy filesystem and terminal requests itself or delegate
them according to harness configuration. A delegated request crosses the event
boundary unchanged. Its eventual response returns through a typed harness
method using the same `RequestId`; it is not converted into a Codex approval
type.

The stream is faithful, not necessarily wire-byte-identical. Transport framing
and JSON-RPC bookkeeping stay in `nori-acp-host`; the schema envelope and
request identity remain intact.

### 4.2 Nori-only notifications

The approved first-pass Nori branch is:

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

The outer variants above are the approved first pass. The live-gap audit below
must add or relocate two concerns before this becomes the final exhaustive Nori
enum. Payload fields and their exact derive sets are intentionally not invented
by this spec: they must be reduced to the fields required by real consumers
during implementation. New public structs should derive only the traits their
use requires.

Variant semantics are fixed:

- `SessionStarted` announces that the Nori harness session exists. ACP setup
  responses still appear as raw ACP responses.
- `SessionPhaseChanged` reports Nori's lifecycle projection of idle, loading,
  prompting, and cancelling. It does not copy request content or completion
  data.
- `SessionEnded` reports orderly shutdown or connection loss with a Nori-owned
  reason.
- `QueueChanged` is the current Nori prompt/operation queue snapshot.
- `ReplayStarted` and `ReplayFinished` bracket replayed raw ACP notifications.
  There is no Nori `ReplayEntry` mirror.
- `ContextCompacted` reports Nori's compaction lifecycle/result.
- `GoalChanged(Some(...))` reports the current persistent Nori goal;
  `GoalChanged(None)` reports that it was cleared.
- `CapabilitiesChanged` contains only Nori harness capabilities. ACP agent and
  session capabilities stay in ACP responses and notifications.
- `Undo`, `UserShell`, and `HookOutput` report Nori-owned operations that ACP
  does not standardize.
- `RequestFailed` means the harness or transport failed before an ACP response
  existed. ACP errors and successful prompt completion remain
  `AcpEvent::Response`.

`NoriEvent` is notification-oriented. It is not a second general-purpose
request/response bus.

#### Two live Nori concerns still need variant co-design

The baseline producer/consumer audit found two outward Nori concerns that do
not fit the approved first-pass variants without lying about their semantics:

- prompt-summary generation is a separate background-agent result used by the
  TUI footer and auto-worktree naming. It is neither context compaction nor hook
  output; and
- nonfatal user-visible notices include slow-cancellation advice, compaction
  advice, reducer out-of-phase warnings, and hook failures. They are not
  `RequestFailed`, because the owning request may continue or complete.

Before implementation, co-design the smallest truthful additions—provisionally
a prompt-summary update and a user-visible notice/diagnostic variant—or
explicitly remove/relocate those behaviors. Do not overload `RequestFailed`,
`ContextCompacted`, or `HookOutput`. This is a narrow completion of the Nori
branch; it does not reopen the ACP envelopes or introduce ACP mirrors.

### 4.3 Commands and queries

Client-to-agent ACP operations are typed methods on the harness handle. Nori
commands and queries are also typed methods. The exact trait split can follow
the existing harness API, but it must preserve these categories:

```rust
// Illustrative method families; names and concrete return types are finalized
// against the post-configuration-refactor harness API.
impl HarnessHandle {
    pub async fn prompt(&self, content: Vec<acp::v1::ContentBlock>) -> Result<acp::v1::RequestId>;
    pub async fn load_session(&self, request: acp::v1::LoadSessionRequest) -> Result<acp::v1::RequestId>;
    pub async fn cancel(&self) -> Result<()>;
    pub async fn respond_to_agent(
        &self,
        request_id: acp::v1::RequestId,
        response: std::result::Result<acp::v1::ClientResponse, acp::v1::Error>,
    ) -> Result<()>;

    pub async fn history_entry(&self, key: HistoryKey) -> Result<Option<HistoryEntry>>;
    pub async fn search_history(&self, query: HistoryQuery) -> Result<Vec<HistoryEntry>>;
    pub async fn custom_prompts(&self) -> Result<Vec<CustomPrompt>>;
    pub async fn undo_snapshots(&self) -> Result<Vec<UndoSnapshot>>;
    pub async fn list_sessions(&self) -> Result<acp::v1::ListSessionsResponse>;
    pub async fn set_config_option(&self, change: acp::v1::SetSessionConfigOptionRequest) -> Result<acp::v1::SetSessionConfigOptionResponse>;
    pub async fn close_session(&self) -> Result<acp::v1::CloseSessionResponse>;
}
```

In particular, history lookup/search, custom prompt discovery, undo snapshot
listing, session listing, config mutation, and session close do not return
correlated `NoriEvent` variants. A CLI/headless JSON-RPC adapter may add its own
correlation IDs at that external transport boundary without imposing them on
Rust embedders or the ordered session stream.

Prompt and load are streaming operations: submission returns the `RequestId`,
zero or more raw ACP notifications may follow, and completion arrives as the
correlated `AcpEvent::Response`. Query-style and other non-streaming ACP methods
return their typed result directly; their raw `AcpEvent::Response` also remains
visible so the boundary stays schema-complete. Those callers never have to
consume or correlate the stream to finish the call. Nori does not add a
duplicate response variant.

## 5. `nori-protocol` is types only

The crate contains public boundary types and no stateful reducer or
presentation code.

Current business logic moves out:

- `ClientEventNormalizer` and its ACP-to-Nori projection helpers move into the
  harness only where reduction is still needed; most projection code disappears
  because raw ACP events cross the boundary.
- `session_runtime::SessionRuntime`, `ActiveRequestState`, message assembly,
  persisted reducer state, and queued-prompt internals move into private
  `nori-harness` modules.
- frontend projections such as tool kind inference, friendly invocation
  descriptions, grouping, and command display move to or remain in `nori-tui`.
- transcript serialization uses a persistence-owned envelope rather than
  making the public event API serve as a storage schema by accident.

A small public `SessionPhase` value may remain because phase changes are part
of `NoriEvent`. It is a boundary value, not the session state machine.

## 6. Compatibility and persistence

The Rust API has no compatibility facade: `codex_protocol::{Event, EventMsg,
Op}` and the current normalized `nori_protocol::ClientEvent` disappear in the
hard cut.

User-owned transcripts are different from Rust API compatibility. Current
version-2 Nori transcript records contain serialized normalized `ClientEvent`
values. The harness loader must continue to read those records, while the
recorder writes a new version containing raw ACP envelopes plus Nori events.
Legacy decode types live privately in the persistence adapter and are not
re-exported from `nori-protocol`.

Replay of a new transcript emits:

1. `NoriEvent::ReplayStarted`;
2. the recorded `SessionEvent::Acp` notifications in original order, plus only
   the Nori events needed to restore Nori-owned state; and
3. `NoriEvent::ReplayFinished`.

Recorded ACP request/response envelopes retain `RequestId` so an offline
transcript reader can correlate the historical exchange. Replay intentionally
does **not** re-emit either envelope: a historical delegated request must never
ask the embedder to perform an operation again, and a historical response must
not complete a live request. A typed transcript-inspection API exposes stored
records separately from replay.

Secrets, transport-only metadata, and ephemeral approval channels must not be
persisted merely because the public stream carries a schema object. The
persistence adapter must redact or omit unsafe request/response content while
preserving the minimum correlation and user-visible history required by the
documented transcript format.

## 7. Exact current deletion and rehoming inventory

This records the current deletion and rehoming candidates at commit `500ee202`
before the configuration rework. The top-level `ClientEvent`, `Op`, and
`EventMsg` variant ledgers below are exhaustive at that baseline; secondary
module types are grouped by ownership. The implementation must refresh all
symbols, producers, and consumers after the configuration work lands, publish
the resulting exact list for sanity-check, and receive approval before deleting
them. That refreshed audit—not this frozen baseline—is the final deletion gate.

### 7.1 Current `nori-protocol::ClientEvent`

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

### 7.2 Current `codex_protocol::Op`

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

### 7.3 Current `codex_protocol::EventMsg`

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

At the current baseline, the first six are explicitly ignored by the TUI and
have no producer. `AgentReasoningSectionBreak` has a consumer but no known
producer. The final four appear consumer-only with no live ACP producer. These
facts must be rechecked rather than assumed at deletion time.

### 7.4 Other `codex-protocol` modules

| Module                                    | Current symbols/variants                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               | Disposition                                                                                       |
| ----------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------- |
| `account`                                 | `PlanType`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             | auth/app-server concern; rehome or delete, never absorb into Nori session protocol                |
| `config_types`                            | `ReasoningEffort`, `ReasoningSummary`, `Verbosity`, `ForcedLoginMethod`, `TrustLevel`, `McpServerConfig`, `McpServerTransportConfig`, `SandboxMode`, shell environment types, and related config enums                                                                                                                                                                                                                                                                                                                                                                                                                 | configuration refactor decides ownership; do not automatically move them into `nori-protocol`     |
| `models`                                  | `ResponseInputItem::{Message, FunctionCallOutput, McpToolCallOutput, CustomToolCallOutput}`; `ContentItem::{InputText, InputImage, OutputText}`; `ResponseItem::{Message, Reasoning, LocalShellCall, FunctionCall, FunctionCallOutput, CustomToolCall, CustomToolCallOutput, WebSearchCall, GhostSnapshot, CompactionSummary, Other}`; `LocalShellStatus::{Completed, InProgress, Incomplete}`; `LocalShellAction::Exec`; `WebSearchAction::{Search, OpenPage, FindInPage, Other}`; `ReasoningItemReasoningSummary::SummaryText`; `ReasoningItemContent::{ReasoningText, Text}`; shell/function-output payload structs | delete Codex/OpenAI Responses vocabulary or localize a genuinely live private app-server consumer |
| `items`                                   | `TurnItem::{UserMessage, AgentMessage, Reasoning, WebSearch}`, `UserMessageItem`, `AgentMessageContent::Text`, `AgentMessageItem`, `ReasoningItem`, `WebSearchItem`, legacy conversions                                                                                                                                                                                                                                                                                                                                                                                                                                | delete; ACP session updates are canonical                                                         |
| `user_input`                              | `UserInput::{Text, Image, LocalImage}`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 | ACP content blocks; local image preprocessing belongs in harness                                  |
| `plan_tool`                               | `StepStatus`, `PlanItemArg`, `UpdatePlanArgs`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          | delete in favor of ACP plan types                                                                 |
| `parse_command`                           | `ParsedCommand::{Read, ListFiles, Search, Unknown}`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    | private harness/frontend parsing and presentation                                                 |
| `num_format`                              | numeric formatting helpers                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             | frontend/common presentation, not protocol                                                        |
| `custom_prompts`                          | `CustomPrompt`, `CustomPromptKind`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     | may remain Nori query-domain values; prefix/presentation constants stay in frontend               |
| `message_history`                         | `HistoryEntry`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         | may remain a Nori persistence/query-domain value                                                  |
| `conversation_id`                         | `ConversationId`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       | Nori transcript identity, not ACP `SessionId`; exact post-refactor name requires review           |
| `approvals`                               | `SandboxRiskLevel::{Low, Medium, High}`, `SandboxCommandAssessment`, `ExecApprovalRequestEvent`, `ApplyPatchApprovalRequestEvent`, `ElicitationRequestEvent`, `ElicitationAction::{Accept, Decline, Cancel}`                                                                                                                                                                                                                                                                                                                                                                                                           | delete ACP mirrors; approval/sandbox policy ownership is revisited after configuration lands      |
| `protocol/history` and legacy event files | `InitialHistory`, `ResumedHistory`, `RolloutItem`, `RolloutLine`, `SessionMeta`, `SessionMetaLine`, `CompactedItem`, `TurnContextItem`, `SessionSource`, `SubAgentSource`, legacy tags/constants, and `HasLegacyEvent`                                                                                                                                                                                                                                                                                                                                                                                                 | private legacy transcript decoder only where required; otherwise delete                           |
| token/rate/final-output helpers           | `TokenUsage`, `TokenUsageInfo`, `RateLimitSnapshot`, `RateLimitWindow`, `CreditsSnapshot`, `FinalOutput` and formatting logic                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          | ACP usage data is canonical; CLI formatting belongs in frontend                                   |

`ReviewDecision` becomes ACP's permission outcome. Codex `FileChange` becomes ACP
tool/diff content. `GitInfo`, MCP authentication status, goals, and hooks are
reviewed as Nori runtime/query types, not carried over merely because they
currently live in `codex-protocol`.

### 7.5 Current dependency consumers

At the baseline, 13 workspace crates directly depend on `codex-protocol`:

- `codex-app-server-protocol`
- `codex-common`
- `codex-core`
- `codex-linux-sandbox`
- `codex-otel`
- `codex-rmcp-client`
- `codex-sandbox`
- `codex-windows-sandbox`
- `nori-acp-host`
- `nori-cli`
- `nori-config`
- `nori-harness`
- `nori-tui`

Every edge must be deleted, rehomed, or replaced before the crate is removed.
The app-server and sandbox support crates do not justify polluting the public
harness protocol; their residual local types should move to their actual
owner.

## 8. Configuration prerequisite and open ownership checks

Implementation waits for the active configuration rework. Immediately after it
lands:

1. rebase and rerun the dependency, import, producer, and consumer audits;
2. identify the new owner of login, model, sandbox, approval, MCP, and shell
   environment configuration;
3. ensure no old config type was moved into `nori-protocol` merely to break a
   dependency cycle;
4. decide whether `AskForApproval` and `SandboxPolicy` remain public harness
   configuration, become private runtime policy, or are replaced by the new
   config vocabulary; and
5. co-design the exact prompt-summary and nonfatal-notice additions identified
   in §4.2; and
6. present the refreshed exact deletion list for approval before any deletion.

These checks may change where configuration types live. They do not reopen the
approved ACP/Nori ownership rule or the hard-cut deletion of Codex protocol
vocabulary.

## 9. Acceptance criteria

The implementation is complete only when:

- `nori-rs/protocol` and the `codex-protocol` workspace package are deleted;
- `cargo tree -i codex-protocol` reports no package;
- only `nori-protocol` directly imports `agent-client-protocol-schema`;
- among Nori client-side product crates, only `nori-acp-host` directly imports
  the higher-level `agent-client-protocol` SDK; ACP agents and conformance
  fixtures may import it on the agent side;
- every other client-side product ACP schema import begins with
  `nori_protocol::acp`;
- `nori-protocol` contains boundary types but no reducer, normalizer, parser,
  formatter, or state machine;
- the harness emits the raw ACP envelopes and the approved Nori event variants,
  including the explicitly approved resolution for prompt summaries and
  nonfatal notices;
- delegated ACP requests round-trip without conversion into Codex types;
- query callers receive typed results without correlating the session stream,
  and no duplicate Nori response variants exist;
- old version-2 transcripts still load, while new transcripts persist the new
  event schema;
- the TUI and a headless harness integration both pass against the same public
  API; and
- the repository docs describe the implemented boundary without references to
  Codex protocol as Nori's control plane.

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
