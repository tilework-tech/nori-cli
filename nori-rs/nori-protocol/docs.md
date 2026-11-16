# Noridoc: nori-protocol

Path: @/nori-rs/nori-protocol

### Overview

`nori-protocol` is the public, types-only boundary for embedding a Nori ACP
session. It re-exports the official ACP schema as `nori_protocol::acp` and adds
only the session events whose semantics belong to the Nori harness, plus the
envelope and status types the session manager publishes for multi-session
clients.

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

`NoriEvent` variants cover session lifecycle (started, phase changed, ended,
forked), queue and replay bracketing, compaction, goals, capabilities, undo,
user-shell and hook output, prompt summaries, notices, request failure and
resolution, and delegation changes (`DelegationChanged`).

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

`RequestResolved` (`request_id`, `answered_by`) announces that a client
answered a pending agent request; `answered_by` is a responder label such as
`client` or `remote_control`. Only the harness session manager emits it, on
managed envelope streams, so clients following a session learn the request no
longer waits on the user. Harness sessions and transcripts never carry it.

`SessionEnvelope` (in [`session_envelope.rs`](src/session_envelope.rs)) wraps
one `SessionEvent` with the `conversation_id` it belongs to, the `agent` name
from the agent registry, and a `seq` number. `seq` starts at 1 for each
conversation and increases by one per event with no gaps, so a client
following several sessions can order events per session and detect a drop. A
fork starts a new conversation and therefore a new sequence. The envelope adds
routing only; the wrapped event is unchanged.

`SessionStatus` is the coarse per-session status shown in a session list:
`Starting`, `Working`, `WaitingOnYou`, `Ready`, `Failed`, `Closed`, or `Done`
(serialized in snake case). `Closed` is a session the user closed or shut down,
or one superseded by a fork; `Done` is a delegated child whose task finished.
`SessionStatus::is_closed()` is true for both, and frontends treat the two
alike wherever they ask whether a session has ended normally. The rules that
derive status from the event stream live in the harness session manager (see
[`nori-harness`](../harness/docs.md)); this crate defines only the values.

`Delegation` is the record a `DelegationChanged` event carries for one
delegated child: request, parent, and child identities, both agent names, the
task, depth, `DelegationStatus`, optional result, and `DelegationOrigin`
(`Agent` when the parent agent called the `delegate` tool, `User` for
`/delegate` or a direct manager call). `origin` has a serde default of `Agent`
so events from hosts that predate the field still deserialize; a user's
`/delegate` relayed by such a host therefore reads as the agent's request.

[`delegation.rs`](src/delegation.rs) holds the shared naming and
result-prompt wording that the harness, the local TUI, and `nori attach` must
agree on.

| Function | Produces |
| --- | --- |
| `session_full_name` | First non-empty trimmed line of a task or title, uncut |
| `shorten_name` | A name in at most a given number of characters, cut in the middle (start, `…`, end) |
| `session_display_name` | `session_full_name` shortened to 40 characters with `shorten_name` |
| `delegation_child_name` | The child's display name from its task, else `UNTITLED_SESSION_NAME` |
| `delegation_status_word` | `waiting on you`, `working`, `done`, `failed`, `declined`, or `cancelled`, matching session status words |
| `delegation_result_prompt` | The prompt delivered to the parent: `[Nori delegation result] from <agent> · <child name> · <status word>` (the child name is the task's whole first line, else `UNTITLED_SESSION_NAME`), then `Child session: <child id>`, then, for a user-origin delegation, `The user started this delegation with /delegate; you did not request it.` (so the parent agent does not deny starting it), then the result text. A declined delegation omits the `Child session:` line because its child never started |
| `delegation_result_summary` | The text after `DELEGATION_RESULT_HEADER` on a prompt's first line, when the second line starts with `Child session: ` or the first line ends in ` · declined`; `None` for any other text |
| `delegation_result_parts` | `DelegationResultParts` (agent, child name, status word) parsed from a result prompt's summary; the name is everything between the first and last ` · `, so a name containing ` · ` survives. `None` for any other text |

`UNTITLED_SESSION_NAME` (`new session`) is the shared name for a session with
no title or task; the harness, the local TUI, and `nori attach` use it instead
of an ID fragment.

Names are cut in the middle rather than at the end because sibling and parent
sessions often share a long task prefix; keeping the end keeps them apart.
`shorten_name` counts characters, for fixed-length names such as notices.
Lines that fit names to the terminal width take the whole name
(`session_full_name`) and cut it once by display cells in the TUI.

The result prompt keeps the child ID on its own line so the receiving agent can
still address that session, while the first line stays readable. The local
TUI and `nori attach` recognize a delivered result through
`delegation_result_summary`, which requires the header plus either the
`Child session: ` second line or the declined ending, so ordinary user text
that merely begins with the header is left whole. The harness and TUI also use
it to keep result prompts from titling the session that receives them. To
draw the collapsed result line they read the agent, name, and status back out
with `delegation_result_parts` instead of rebuilding the header, so the header
wording exists only in `delegation_result_prompt`. Both strings are a
cross-crate contract.

[`ReplyPreview`](src/reply_preview.rs) is the other piece of shared text
logic: it observes one session's `SessionEvent`s and keeps the start of the
agent's latest reply for session lists. The host journal in the
[harness](../harness/docs.md) and the local TUI's session previews both use it,
so `nori attach`, `nori host ls`, and the local list show the same reply.

- A `Prompting` or `Loading` phase marks the reply stale; the previous turn's
  reply stays until the new turn's first agent text replaces it.
- A tool call or agent request after text makes the next text start a new
  sentence (joined with a space), so separate messages are not glued together.
- `text()` collapses whitespace to one line, keeps at most 200 characters
  (lists cut further to their column), and is `None` before any agent text.

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
  compaction, goals, undo, user-shell output, hooks, summaries, notices,
  delegation, and failures that have no ACP response.
- Output without an active local request remains raw `SessionEvent::Acp`
  traffic. Presentation grouping and optional lifecycle hints stay private to
  consumers such as [`nori-tui`](../tui/docs.md).
- `SessionEnvelope` and `SessionStatus` are Nori routing and summary types, not
  ACP extensions.
- The crate contains no reducer, normalizer, transcript compatibility
  decoder, or presentation state. The delegation text helpers are plain
  strings with no styling; each frontend applies its own colors.
- `ReplayStarted` and `ReplayFinished` bracket historical ACP notifications
  only. Current setup responses and Nori lifecycle events remain outside the
  replay body.
- Unit tests in [`delegation.rs`](src/delegation.rs) pin the result-prompt
  and summary shapes; `just test protocol` runs `cargo test -p nori-protocol`.
- The deleted Codex `Event`/`EventMsg`/`Op` API and the former public
  `ClientEvent` normalization API have no compatibility aliases.

Created and maintained by Nori.
