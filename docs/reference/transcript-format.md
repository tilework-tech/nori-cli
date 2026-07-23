# Nori transcript format

Reference for Nori's versioned JSONL session transcripts. Schema v3 is the
canonical write format. The Harness keeps older v1/v2 storage compatibility
private so embedders do not inherit the retired normalized protocol as API.

Canonical implementation: `nori-rs/harness/src/transcript/` in
`nori-harness`. The versioned `TranscriptLine` and `TranscriptEntry` storage
types are private. Public Rust readers use `TranscriptLoader`, `Transcript`,
`Transcript::records()`, and `TranscriptRecord`.

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

Optional fields are omitted when absent: `agent`, `git`, and
`acp_session_id`. Within `git`, `branch` and `commit_hash` are optional.
`acp_session_id` is the agent's identity used for ACP session load or resume;
it is distinct from Nori's transcript `session_id`.

### `user`

User input is stored explicitly because it travels from client to agent and
cannot be reconstructed from an agent-to-client event stream.

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
events. The public replay sequence is:

1. `NoriEvent::ReplayStarted`;
2. historical `SessionEvent::Acp(AcpEvent::Notification(...))` values in
   source order; and
3. `NoriEvent::ReplayFinished`.

For v3, an explicit `user` entry is projected to an ACP user-message
notification at its recorded position. Stored ACP notifications remain exact,
apart from retargeting their session ID to the active session. Stored Nori
events, ACP requests, and ACP responses are not emitted inside replay brackets.
In particular, the current load/new response is never mislabeled as historical
replay. Historical requests cannot repeat side effects, and historical
responses cannot complete live requests.

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
- it skips normalized client events and legacy tool/patch storage records;
- internal resume code may privately derive completed display/goal state from
  selected legacy records; and
- v1/v2 user/assistant content can be synthesized as ACP message notifications
  when no raw v3 notification stream exists.

The checked-in public compatibility test covers v2 user, assistant, and
thinking records. There is not currently a checked-in fixture matrix covering
every legacy top-level record and all 18 former normalized `ClientEvent` tags.

The loader does not expose a version-specific decoder API. It tolerantly
deserializes the private storage enum; unknown fields are ignored, unknown or
unparseable post-metadata lines are skipped, and blank lines are skipped.

## Token usage

Canonical v3 transcript entries have no separate Nori token-count record. ACP
usage can appear inside stored ACP session notifications. The goodbye-card
token statistics may also be read from the underlying agent's own transcript
files by `nori-rs/harness/src/transcript_discovery.rs`; those third-party
formats are outside this reference.

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
