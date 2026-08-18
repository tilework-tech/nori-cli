# Noridoc: nori-protocol

Path: @/nori-rs/nori-protocol

### Overview

`nori-protocol` is the public, types-only boundary for embedding a Nori ACP
session. It re-exports the official ACP schema as `nori_protocol::acp` and adds
only the session events whose semantics belong to the Nori harness.

### How it fits into the larger codebase

```text
agent-client-protocol-schema
              |
              v
        nori-protocol
         /          \
        v            v
 nori-acp-host   nori-harness -> nori-tui / headless consumers
```

This crate is the sole direct ACP schema dependency in the workspace. Other
client-side crates import schema values through `nori_protocol::acp`, which
keeps a future ACP library or vendoring change behind one public path.

### Core Implementation

The public stream is source-owned at its outer edge:

```rust
pub enum SessionEvent {
    Acp(AcpEvent),
    Nori(NoriEvent),
}

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

`NoriEvent` has exactly sixteen outer variants:

```rust
pub enum NoriEvent {
    SessionStarted(SessionStarted),
    SessionPhaseChanged(SessionPhase),
    SessionEnded(SessionEnded),
    QueueChanged(QueueSnapshot),
    ReplayStarted(ReplayStarted),
    ReplayFinished,
    ContextCompacted(ContextCompactedEvent),
    SessionForked(SessionForked),
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

`NoriEvent` carries harness-owned lifecycle and product behavior that has no
ACP event of its own. A `session/update` received without a locally owned
request remains a schema-native, unowned update; no synthetic request ID or
source attribution is added. Presentation of unowned updates or an agent-owned
turn has no public Nori completion event; client-owned prompt turns complete
through their correlated raw ACP prompt response. The source-owned event
boundary is defined in
[`session_event.rs`](src/session_event.rs).

`SessionForked` is emitted when branch-at-head `/fork` forks the transcript. It
carries `previous_conversation_id` (the now-frozen parent conversation),
`new_conversation_id` (the fresh conversation seeded from the parent and made
active), and `new_acp_session_id` (the forked ACP session). It is recorded into
the new conversation, not the parent.

Session termination reasons are `Shutdown`, `Closed`, `ConnectionLost`,
`SpawnFailed`, and `TimedOut`. Raw ACP request and response envelopes retain the
original `RequestId`; consumers do not correlate requests from content or from
Nori-generated IDs. `SessionPhase::{Loading, Prompting, Cancelling}` carries the
exact ACP wire `RequestId` for harness-issued operations.

### Things to Know

- ACP owns messages, thoughts, plans, tools, permissions, filesystem and
  terminal requests, modes, config options, capabilities, usage, and method
  responses. Do not mirror them into Nori types.
- Nori owns only behavior around the ACP session: lifecycle, queueing, replay,
  compaction, goals, undo, user-shell output, hooks, summaries, notices, and
  failures that have no ACP response.
- Output without an active local request remains raw `SessionEvent::Acp`
  traffic. Presentation grouping and optional lifecycle hints stay private to
  consumers such as [`nori-tui`](../tui/docs.md).
- The crate contains no reducer, normalizer, parser, formatter, transcript
  compatibility decoder, or presentation state.
- `ReplayStarted` and `ReplayFinished` bracket historical ACP notifications
  only. Current setup responses and Nori lifecycle events remain outside the
  replay body.
- The deleted Codex `Event`/`EventMsg`/`Op` API and the former public
  `ClientEvent` normalization API have no compatibility aliases.

Created and maintained by Nori.
