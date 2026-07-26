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

`NoriEvent` carries harness-owned lifecycle and product behavior that has no
ACP event of its own. This includes session phase and termination, queue and
replay boundaries, compaction, goals, undo, user-shell and hook output,
notices, and classified failures. A `session/update` received without a locally
owned request remains a schema-native, unowned update; no synthetic request ID
or source attribution is added. Proactive presentation has no public Nori
completion event; owned prompts complete through their correlated raw ACP
prompt response. The source-owned event boundary is defined in
[`session_event.rs`](src/session_event.rs).

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
- Proactive output remains raw `SessionEvent::Acp` traffic. Presentation
  grouping and optional lifecycle hints stay private to consumers such as
  [`nori-tui`](../tui/docs.md).
- The crate contains no reducer, normalizer, parser, formatter, transcript
  compatibility decoder, or presentation state.
- `ReplayStarted` and `ReplayFinished` bracket historical ACP notifications
  only. Current setup responses and Nori lifecycle events remain outside the
  replay body.
- The deleted Codex `Event`/`EventMsg`/`Op` API and the former public
  `ClientEvent` normalization API have no compatibility aliases.

Created and maintained by Nori.
