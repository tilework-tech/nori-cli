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
notices, classified failures, and completion of a turn observed through a
shared broker session.

`ObservedTurnCompleted` contains the ACP `StopReason` and the last assembled
agent message. It completes a turn initiated by another attached client after
the harness receives the broker's idle status metadata; locally initiated turns
continue to complete through their raw ACP prompt response. The source-owned
event boundary is defined in [`session_event.rs`](src/session_event.rs).

Session termination reasons are `Shutdown`, `Closed`, `ConnectionLost`,
`SpawnFailed`, and `TimedOut`. Raw ACP request and response envelopes retain the
original `RequestId`; consumers do not correlate requests from content or from
Nori-generated IDs. `SessionPhase::{Loading, Prompting, Cancelling}` carries the
exact ACP wire `RequestId` for harness-issued operations. An observed turn has
no request from the observing client, so its `Prompting` phase uses a stable
harness-owned synthetic ID only to preserve the phase shape.

### Things to Know

- ACP owns messages, thoughts, plans, tools, permissions, filesystem and
  terminal requests, modes, config options, capabilities, usage, and method
  responses. Do not mirror them into Nori types.
- Nori owns only behavior around the ACP session: lifecycle, observed-turn
  completion, queueing, replay, compaction, goals, undo, user-shell output,
  hooks, summaries, notices, and failures that have no ACP response.
- The crate contains no reducer, normalizer, parser, formatter, transcript
  compatibility decoder, or presentation state.
- `ReplayStarted` and `ReplayFinished` bracket historical ACP notifications
  only. Current setup responses and Nori lifecycle events remain outside the
  replay body.
- The deleted Codex `Event`/`EventMsg`/`Op` API and the former public
  `ClientEvent` normalization API have no compatibility aliases.

Created and maintained by Nori.
