# Nori transcript format

Reference for Nori's versioned JSONL session transcripts. Schema v3 is the
only write format; readers also accept older v1/v2 files.

Implementation: `nori-rs/harness/src/transcript/`. The versioned storage types
are private; Rust readers use the API under
[Reading transcripts programmatically](#reading-transcripts-programmatically).

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
- A branch-at-head fork writes a new file whose `session_meta` is followed by a
  copy of every non-metadata entry from the parent. Copied entries are
  re-stamped with the fork's `ts` and the current `v`, so a v3 file forked from
  a legacy parent can contain legacy entry kinds.

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
rewritten whenever a session starts in the project, and both timestamps are set
to the rewrite time, so `created_at` is not the project's first-seen time.

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

The v3 writer records three entry kinds: `session_meta`, `user`, and
`session_event`. Apart from entries copied by a fork, it never writes the
legacy kinds listed under [Legacy v1/v2 entries](#legacy-v1v2-entries).

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
  "acp_session_id": "acp-sess-abc123",
  "forked_from": "5d0c8e21-..."
}
```

Optional fields are omitted when absent: `agent`, `git`, `acp_session_id`, and
`forked_from`. Within `git`, `branch` and `commit_hash` are optional.
`acp_session_id` is the agent's identity used for ACP session load or resume;
it is distinct from Nori's transcript `session_id`. `forked_from` is the parent
transcript's `session_id` when the file was created by a branch-at-head fork.

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

A Nori lifecycle event has the same envelope with an `event` of:

```json
{ "source": "nori", "event": { "event_type": "session_phase_changed", "event": { "phase": "idle" } } }
```

ACP payload casing and shape come from `nori_protocol::acp`; the outer Nori
tags come from `SessionEvent`, `AcpEvent`, and `NoriEvent`. ACP requests and
responses retain the schema `RequestId`, which may be a string, number, or
`null`. Because v3 stores the exact public event payload, its nested ACP shape
tracks the ACP schema re-export selected by `nori-protocol`. The raw ACP
notification is the only stored copy of agent output; no derived assistant
record or presentation projection is persisted.

## Setup, phases, and replay

The recorded live stream preserves Harness publication order.

- Current initialize and session setup responses precede
  `NoriEvent::SessionStarted`.
- If `session/load` fails and Nori falls back to `session/new`, both the failed
  load response and fallback-new response precede `SessionStarted`.
- `SessionPhase::{Loading, Prompting, Cancelling}` stores the exact ACP wire
  `RequestId` for the active operation.
- One accepted Harness prompt corresponds to exactly one ACP
  `session/prompt`. A successful empty `EndTurn` is terminal for that request.

Replay is a filtered projection, not a second reading mode for all stored
events. The public replay sequence is:

1. `NoriEvent::ReplayStarted`, whose `source` is `transcript` or `agent`;
2. historical `SessionEvent::Acp(AcpEvent::Notification(...))` values in
   source order; and
3. `NoriEvent::ReplayFinished`.

For v3, canonical raw user-message chunks supersede the explicit `user` entry
with the same message id during replay. This retains attachment blocks without
duplicating the prompt. A `user` entry without matching raw chunks is projected
to an ACP user-message notification at its recorded position. Legacy
`assistant` entries are projected to agent message and thought notifications
only when the file contains no stored ACP session notification.
Stored ACP notifications remain exact, apart from retargeting their session ID
to the active session. Stored Nori events, ACP requests, and ACP responses are
never replayed, so historical requests cannot repeat side effects and
historical responses cannot complete live requests.

Agent-sourced `session/load` replay follows the same outward rule: the markers
bracket the load-time ACP notifications in agent order, while the current load
response remains outside the brackets and before `SessionStarted`.

## Legacy v1/v2 entries

Older files can contain `assistant` (text and thinking blocks),
`client_event`, `tool_call`, `tool_result`, and `patch_apply` entries. Readers
must accept them; writers must never produce them. `Transcript::records()`
exposes legacy `assistant` blocks as `Assistant` and `Thinking` records and
skips the other legacy kinds. Their storage types stay private to
`nori-harness` and are not re-exported from `nori-protocol`.

## Token usage

There is no Nori token-count entry; ACP usage appears only inside stored ACP
notifications. Goodbye-card token statistics come from the agent's own
transcript files (`nori-rs/harness/src/transcript_discovery.rs`), whose formats
are outside this reference.

## Reading transcripts programmatically

Use `nori_harness::TranscriptLoader` and iterate `Transcript::records()`
(`TranscriptRecord` is exported from `nori_harness::transcript`):

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

A third-party producer that writes JSONL directly must version its integration
against this document and the selected `nori-protocol` ACP schema; serialized
public events are not a frozen storage ABI.
