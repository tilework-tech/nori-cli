# Nori transcript format

Reference for Nori's versioned JSONL session transcripts. Schema v3 is the
canonical write format. The Harness keeps older v1/v2 storage compatibility
private so embedders do not inherit the retired normalized protocol as API.

Canonical implementation: `nori-rs/harness/src/transcript/` in
`nori-harness`. The versioned `TranscriptLine` and `TranscriptEntry` storage
types are private. Public Rust readers use `TranscriptLoader`, `Transcript`,
`Transcript::records()`, `TranscriptRecord`, and `Transcript::context_timeline()`.

## File locations

Transcripts live under the Nori home directory (`$NORI_HOME`, or
`~/.nori/cli` by default):

```text
$NORI_HOME/transcripts/by-project/{project-id}/
  ├── project.json
  └── sessions/
      └── {session-id}.jsonl
```

- `{session-id}` is a UUIDv4 generated when recording starts.
- Session files are created with mode `0600` on Unix.
- The writer creates a fresh file, appends one JSON object per line, and flushes
  each line. It does not `fsync`, so a crashed session can lose its tail.

### Project IDs and `project.json`

The project ID is 16 lowercase hexadecimal characters derived by hashing, in
priority order:

1. the normalized `origin` Git remote;
2. the absolute Git root; or
3. the canonicalized working directory.

Rust's `DefaultHasher` is not guaranteed stable across Rust versions. Treat the
ID as an opaque directory name and discover projects by listing `by-project/`
and reading `project.json`.

```json
{
  "id": "a1b2c3d4e5f60718",
  "name": "my-repo",
  "git_remote": "git@github.com:user/my-repo.git",
  "git_root": "/home/user/src/my-repo",
  "cwd": "/home/user/src/my-repo",
  "created_at": "2026-07-03T12:30:45.123Z",
  "updated_at": "2026-07-03T12:30:45.123Z"
}
```

`git_remote` and `git_root` are `null` outside a Git repository. The file is
rewritten when a new session starts in the project.

## Common line envelope

Each nonblank JSONL line is a self-contained object with flattened entry
fields:

- `ts`: ISO 8601 UTC timestamp with millisecond precision;
- `v`: storage schema version, currently `3`; and
- `type`: snake-case entry kind.

Canonical writers put `session_meta` first. The full loader must find valid
session metadata and treats an unparseable line before metadata as a hard
error. After metadata, it skips blank, unknown, or unparseable lines so an
otherwise readable transcript survives schema changes.

## Canonical schema v3

The runtime v3 writer emits three entry kinds: `session_meta`, `user`, and
`session_event`. It does not write `assistant`, `client_event`, `tool_call`,
`tool_result`, or `patch_apply` entries.

### `session_meta`

```json
{
  "ts": "2026-07-03T12:30:45.123Z",
  "v": 3,
  "type": "session_meta",
  "session_id": "7f9c2f6a-1c1e-4a9b-9a3e-2f0d8b7c6d5e",
  "project_id": "a1b2c3d4e5f60718",
  "started_at": "2026-07-03T12:30:45.123Z",
  "cwd": "/home/user/src/my-repo",
  "agent": "claude-code",
  "cli_version": "0.9.0",
  "git": { "branch": "main", "commit_hash": "1975265abc..." },
  "acp_session_id": "acp-sess-abc123"
}
```

Optional identity fields, including `agent`, `git`, `acp_session_id`, and
`forked_from`, are omitted when absent. `forked_from` identifies the parent
Nori transcript for a branch-at-head fork. Within `git`, `branch` and
`commit_hash` are optional.
`acp_session_id` is the agent's identity used for ACP session load or resume;
it is distinct from Nori's transcript `session_id`.

### `user`

User input has an explicit stable record alongside the Harness's canonical ACP
user-message notification. Replay uses the canonical notification when present
and falls back to this record for older or incomplete streams.

```json
{
  "ts": "2026-07-03T12:31:02.001Z",
  "v": 3,
  "type": "user",
  "id": "msg-001",
  "content": "What files are in src?",
  "attachments": [{ "type": "file_path", "path": "/tmp/screenshot.png" }]
}
```

`attachments` is omitted when empty. Supported private storage shapes are
`file_path` (`path`) and `base64` (`data`, `mime_type`). The current Harness
input path records display text with an empty attachment list.

### `session_event`

The `event` field contains the exact public `nori_protocol::SessionEvent`
delivered by the Harness. Its outer `source` is `acp` or `nori`.

Optional fields alongside `event` carry Nori observation and replay metadata;
they do not modify the nested ACP payload:

| Field | Meaning |
| --- | --- |
| `observation_id` | Stable identity assigned once before live reduction and recording |
| `observed_at` | Original observation time, distinct from the JSONL writer's `ts` |
| `historical_evidence` | Archived timeline evidence, excluded from conversation records and replay |
| `history_incomplete` | The recording has known gaps in prompt-boundary history |
| `response_kind` | Private discriminator preserving the ACP response variant when its untagged payload is ambiguous |

Absent flags default to false. Older entries without observation metadata use
their transcript-relative position for identity and `ts` for observation time.
Copies made during resume or fork preserve these original values. The response
discriminator belongs to the storage decoder, not the public protocol; legacy
prompt responses can still be recognized from their required `stopReason`
field. Public readers never need to decode this metadata themselves.

A representative ACP notification is:

```json
{
  "ts": "2026-07-03T12:31:03.001Z",
  "v": 3,
  "type": "session_event",
  "event": {
    "source": "acp",
    "event": {
      "message_type": "notification",
      "sessionId": "acp-sess-abc123",
      "update": {
        "sessionUpdate": "agent_message_chunk",
        "content": { "type": "text", "text": "src contains main.rs." }
      }
    }
  }
}
```

A representative Nori lifecycle event is:

```json
{
  "ts": "2026-07-03T12:31:03.002Z",
  "v": 3,
  "type": "session_event",
  "event": {
    "source": "nori",
    "event": {
      "event_type": "session_phase_changed",
      "event": { "phase": "idle" }
    }
  }
}
```

ACP payload casing and shape come from `nori_protocol::acp`; the outer Nori
tags come from `SessionEvent`, `AcpEvent`, and `NoriEvent`. ACP requests and
responses retain the schema `RequestId`, which may be a string, number, or
`null`. Because v3 stores the exact public event payload, its nested ACP shape
tracks the ACP schema re-export selected by `nori-protocol`.

V3 intentionally has one canonical copy of agent output: the raw ACP
notification. It does not also persist a derived assistant record or the
private TUI/Harness presentation projection.

## Setup, phases, and replay

The recorded live stream preserves Harness publication order.

- Current initialize and session setup responses precede
  `NoriEvent::SessionStarted`.
- If `session/load` fails and Nori falls back to `session/new`, both the failed
  load response and fallback-new response precede `SessionStarted`.
- `SessionPhase::{Loading, Prompting, Cancelling}` stores the exact ACP wire
  `RequestId` for the active operation.
- One accepted Harness prompt corresponds to exactly one ACP
  `session/prompt`; there is no cancel-tail resend heuristic. A successful
  empty `EndTurn` is terminal for that request.

Replay is a filtered projection, not a second reading mode for all stored
events. Archived `historical_evidence` entries are excluded before selecting
replay content; their notifications cannot suppress legacy conversation fallback.
The public replay sequence is:

1. `NoriEvent::ReplayStarted`;
2. historical `SessionEvent::Acp(AcpEvent::Notification(...))` values in
   source order; and
3. `NoriEvent::ReplayFinished`.

For v3, canonical raw user-message chunks supersede the explicit `user` entry
with the same message id during replay. This retains attachment blocks without
duplicating the prompt. A `user` entry without matching raw chunks is projected
to an ACP user-message notification at its recorded position for compatibility.
Stored ACP notifications remain exact, apart from retargeting their session ID
to the active session. Stored Nori events, ACP requests, and ACP responses are
not emitted inside replay brackets. In particular, the current load/new
response is never mislabeled as historical replay. Historical requests cannot
repeat side effects, and historical responses cannot complete live requests.

Agent-sourced `session/load` replay follows the same outward rule: the markers
bracket the load-time ACP notifications in agent order, while the current load
response remains outside the brackets and before `SessionStarted`.

## Private v1/v2 compatibility

Older transcripts can contain these retired storage records:

- `assistant` text/thinking blocks;
- normalized `client_event` values;
- `tool_call` and `tool_result`; and
- `patch_apply`.

Those shapes remain private to the Harness transcript types and loader. They
are not re-exported from `nori-protocol`, and `TranscriptEntry` /
`TranscriptLine` are not public Harness types. New code must not write v2 or
recreate the former normalized protocol facade.

The compatibility projection is intentionally narrower than the old storage
enum:

- `Transcript::records()` exposes user text, legacy assistant text, legacy
  thinking text, and exact v3 `SessionEvent` values;
- it skips archived timeline evidence, normalized client events, and legacy
  tool/patch storage records;
- internal resume code may privately derive completed display/goal state from
  selected legacy records; and
- v1/v2 user/assistant content can be synthesized as ACP message notifications
  when no raw v3 notification stream exists.

The checked-in public compatibility test covers v2 user, assistant, and
thinking records. There is not currently a checked-in fixture matrix covering
every legacy top-level record and former normalized `ClientEvent` tag.

The loader does not expose a version-specific decoder API. It tolerantly
deserializes the private storage enum; unknown fields are ignored, unknown or
unparseable post-metadata lines are skipped, and blank lines are skipped.

## Context timeline and token usage

There is no separate Nori token-count or computed-checkpoint record. The
[Harness context timeline](../nori-rs/harness/src/context_timeline/docs.md)
reconstructs prompt start/end readings from raw ACP usage, correlated responses,
and Nori lifecycle evidence. `Transcript::context_timeline()` applies the same
reducer as the live `HarnessHandle::context_timeline().await` query. Boundaries
retain the latest observation available at that time; a late update never
rewrites a completed checkpoint.

When resume creates a fresh transcript, it first archives the supplied local
transcript's required raw timeline evidence with `historical_evidence` set.
The new file can reconstruct prior checkpoints without reading an ancestor
file. `Transcript::records()` and conversation replay exclude these archives,
while provider replay is still recorded separately, including messages newer
than the local transcript. Fork seeds the child from the parent transcript and
preserves observation identity. Repeated resume does not turn archived or
replayed evidence into new turns.

Unavailable prior boundaries remain incomplete history. A resumed unfinished
turn receives an unknown end reading rather than inheriting a later session's
completion. Shrink, compaction, or changed context capacity breaks comparison
with the previous baseline; this does not delete the recorded checkpoints.

Provider-specific token statistics may also be read from the underlying
agent's transcript by
[`transcript_discovery.rs`](../nori-rs/harness/src/transcript_discovery.rs).
Those third-party formats and latest-usage fallbacks are outside this schema
and do not create ACP timeline observations.

## Reading transcripts programmatically

Use `nori_harness::TranscriptLoader` (or the equivalent exports under
`nori_harness::transcript`) and iterate `Transcript::records()`:

```rust
for record in transcript.records() {
    match record {
        TranscriptRecord::User { content } => { /* ... */ }
        TranscriptRecord::Assistant { content } => { /* legacy v1/v2 */ }
        TranscriptRecord::Thinking { content } => { /* legacy v1/v2 */ }
        TranscriptRecord::SessionEvent(event) => { /* exact v3 event */ }
    }
}
```

Do not depend on the private versioned storage enum. A third-party producer
that writes JSONL directly must version its integration against this document
and the selected `nori-protocol` ACP schema; Nori does not promise that exact
serialized public events are a permanently frozen storage ABI.
