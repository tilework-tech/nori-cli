# Nori transcript format

Reference for Nori's versioned JSONL session transcripts. Schema v3 is the
canonical write format. Readers also accept older v1/v2 files; that
compatibility stays private to the Harness so embedders do not inherit the
retired normalized protocol as API.

Implementation: `nori-rs/harness/src/transcript/`. The versioned
`TranscriptLine` and `TranscriptEntry` storage types are private. Rust readers
use the API under
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

`{session-id}` is a UUIDv4. Its origin depends on who starts the recording:

| Recording | `{session-id}` |
| --- | --- |
| Session started through the session manager (TUI, `nori host`, or `nori exec --acp` with `multi_session`) | The conversation ID the manager assigns before launch. A same-ID restart of a failed session keeps it. |
| Branch-at-head fork child | A fresh ID minted by the recorder; the manager adopts it from `SessionForked`. |
| Harness started without a session manager | A fresh ID minted when recording starts. |
| Host managed-launch recording | A fresh ID unrelated to the managed conversation ID, stored under the binding's own home (`<binding home>/.nori/cli`). |

Write behavior:

- Session files are created with mode `0600` on Unix. Creating a file for an
  ID that already exists truncates it.
- The writer appends one JSON object per line and flushes each line without
  `fsync`, so a machine crash can lose the tail. `fsync` runs only for the
  same-ID restart seed, session-manager recovery checkpoints, and the durable
  flush on managed-launch recorders. Shutdown and fork seeding only
  flush.
- A branch-at-head fork writes a new file whose `session_meta` is followed by
  a copy of every non-metadata entry from the parent, legacy kinds included.
  Each copy is re-stamped with the fork's `ts` and `v: 3`, so a fork of a
  v1/v2 parent contains legacy entry kinds labelled `v: 3`. If seeding fails,
  the ACP fork still stands and recording continues in the parent file.
- A same-ID restart seeds a replacement file and atomically installs it at the
  existing path; see [Restarting a failed session](#restarting-a-failed-session).

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
  "name": "demo",
  "git_remote": "https://git.example.com/demo/demo.git",
  "git_root": "/work/demo",
  "cwd": "/work/demo",
  "created_at": "2026-07-03T12:30:45.123Z",
  "updated_at": "2026-07-03T12:30:45.123Z"
}
```

- `git_remote` is the raw remote URL; `cwd` is the canonicalized working
  directory. Both Git fields are `null` outside a Git repository.
- The file is rewritten, non-atomically, whenever a new session or a
  branch-at-head fork starts in the project. A same-ID restart does not
  rewrite it.
- Each rewrite sets both `created_at` and `updated_at` to the write time, so
  `created_at` is not the project's first-seen time. `name` and `cwd` reflect
  the most recent writer.

## Common line envelope

Each nonblank JSONL line is a self-contained object with flattened entry
fields:

- `ts`: ISO 8601 UTC write time with millisecond precision;
- `v`: storage schema version, currently `3`; and
- `type`: snake-case entry kind.

Writers put `session_meta` on the first line. The full loader requires valid
session metadata and treats an unparsable line before it as a hard error.
After metadata, it skips blank, unknown, or unparsable lines so a readable
transcript survives schema changes. Metadata-only readers (session listing and
the resume picker) are stricter: `session_meta` must be the literal first
line.

## Canonical schema v3

Live recording writes three entry kinds: `session_meta`, `user`, and
`session_event`. Legacy kinds appear in a v3 file only when a fork copies them
from its parent; see [Legacy v1/v2 entries](#legacy-v1v2-entries).

### `session_meta`

```json
{
  "ts": "2026-07-03T12:30:45.123Z",
  "v": 3,
  "type": "session_meta",
  "session_id": "3f6c1a2e-0000-4000-8000-000000000002",
  "project_id": "a1b2c3d4e5f60718",
  "started_at": "2026-07-03T12:30:45.123Z",
  "cwd": "/work/demo",
  "agent": "claude-code",
  "cli_version": "0.9.0",
  "git": { "branch": "main", "commit_hash": "1975265abc0000000000000000000000000000ab" },
  "acp_session_id": "acp-session-0001",
  "forked_from": "3f6c1a2e-0000-4000-8000-000000000001"
}
```

- `agent`, `git`, `acp_session_id`, and `forked_from` are omitted when absent.
  Within `git`, `branch` (absent on a detached HEAD) and `commit_hash` are
  optional. `cwd` is the session's working directory as given.
- `forked_from` is the parent transcript's `session_id` for a branch-at-head
  fork. Rewind-to-message children and delegated children have no lineage
  field: a delegated child's `session_meta` carries no link to its parent.
- `acp_session_id` is the agent's session identity that resume passes to
  `session/load` or `session/resume`. It is written when the file is created
  and when a same-ID restart replaces it. Summary-and-swap compaction does not
  update it, so after that compaction resume targets the ACP session recorded
  at start, not the post-compaction one.

### `user`

User input is recorded explicitly, alongside the Harness's canonical ACP
user-message notification, because it travels from client to agent and an
agent-to-client stream cannot always reconstruct it.

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

A `user` line is written when the prompt actually reaches the agent:

- a prompt sent immediately, once its ACP request ID is assigned;
- a queued prompt when the queue dispatches it, not when it is queued;
- a follow-up steered into a running turn through `_session/steering`, once
  the agent confirms it was injected. The steer adds input to the running turn
  rather than starting a `session/prompt`, but still writes a `user` line and
  a `user_message_chunk` notification carrying the `nori.dev/promptEchoId` and
  `nori.dev/followUpSender` metadata.

Internal prompts such as compaction or goal continuation never write `user`
lines. A rejected follow-up, or one whose delivery is unknown, leaves nothing.

### `session_event`

The `event` field contains the exact public `nori_protocol::SessionEvent`
delivered by the Harness. Its outer `source` is `acp` or `nori`. Optional
fields beside `event` carry observation and replay metadata without changing
the nested payload:

| Field | Meaning |
| --- | --- |
| `observation_id` | Stable identity assigned once before live reduction and recording |
| `observed_at` | Original observation time, distinct from the writer's `ts` |
| `historical_evidence` | Archived context-timeline evidence, excluded from records and replay |
| `history_incomplete` | The recording has known gaps in prompt-boundary history |
| `response_kind` | Private discriminator (`prompt`, `new_session`, `fork`) for ambiguous untagged ACP responses |

Live writes always set `observation_id` and `observed_at`. Absent flags default
to false. Older entries without observation metadata use their position in the
file for identity and `ts` for observation time. Fork seeds and transcript
fallback seeds keep the original values; a native-load seed records the
agent's replay as new observations. Legacy prompt responses without
`response_kind` are recognized by their `stopReason` field. Public readers
never decode this metadata themselves.

A representative ACP notification:

```json
{
  "ts": "2026-07-03T12:31:03.001Z",
  "v": 3,
  "type": "session_event",
  "event": {
    "source": "acp",
    "event": {
      "message_type": "notification",
      "sessionId": "acp-session-0001",
      "update": {
        "sessionUpdate": "agent_message_chunk",
        "content": { "type": "text", "text": "src contains main.rs." }
      }
    }
  },
  "observation_id": "3f6c1a2e-0000-4000-8000-0000000000a1",
  "observed_at": "2026-07-03T12:31:03.000Z"
}
```

A Nori lifecycle event has the same envelope with an `event` of:

```json
{ "source": "nori", "event": { "event_type": "session_phase_changed", "event": { "phase": "idle" } } }
```

The outer tags come from `SessionEvent` (`source`), `AcpEvent`
(`message_type`), and `NoriEvent` (`event_type`). The nested ACP casing and
shape come from the ACP schema that `nori-protocol` re-exports and track it.
ACP requests and responses keep the schema `RequestId`, which may be a string,
number, or `null`. The raw ACP notification is the only stored copy of agent
output; no derived assistant record or presentation projection is persisted.

## What the runtime records

Only events published by the Harness runtime reach the transcript, in
publication order. Undo, compaction, and fork append `Undo`,
`ContextCompacted`, and `SessionForked` events; earlier lines are never
rewritten.

| Activity | Recorded as |
| --- | --- |
| Queue | No queue entry kind. Each `QueueChanged` snapshot (`prompts`, `entries`, `paused`) is recorded, but replay never restores the queue. Prompts removed or taken back before dispatch leave only `QueueChanged` traces. |
| Steered follow-up | `user` line plus `user_message_chunk`, as described under [`user`](#user). |
| Delegation | A delegated child has its own transcript in the project of the parent's working directory; its first `user` line is the task. The child's result returns to the parent as an ordinary prompt starting `[Nori delegation result]` and is recorded as a parent `user` line. `DelegationChanged`, delegation "waiting on you" notices, and `RequestResolved` come from the session manager, not the runtime, and are not recorded. |
| Session start | `SessionStarted` carries `transcript_id` and `transcript_path`. |

Because no file records the parent link, delegation lineage lives only in the
running session manager. After a host restart, resuming a child transcript
opens it as a top-level session.

A host managed-launch recording (`nori-rs/harness/src/backend/managed_additions.rs`)
uses a separate recorder. It writes only `session_meta` (with the ACP session
ID as `acp_session_id`), `user` lines with text blocks joined by newlines, and
the matching `user_message_chunk` notifications. It records no agent output,
responses, or Nori events.

## Setup, phases, and replay

- Initialize and session setup responses precede `NoriEvent::SessionStarted`.
- If `session/load` fails and Nori falls back to `session/new`, both the
  failed load response and the fallback-new response precede `SessionStarted`.
- `SessionPhase::{Loading, Prompting, Cancelling}` stores the exact ACP wire
  `RequestId` for the active operation.
- Each accepted prompt turn corresponds to exactly one ACP `session/prompt`. A
  successful empty `EndTurn` is terminal for that request.

Replay is a filtered projection, not a second way to read every stored event.
The public replay sequence is:

1. `NoriEvent::ReplayStarted`, whose `source` is `transcript` or `agent`;
2. historical `SessionEvent::Acp(AcpEvent::Notification(...))` values in
   source order; and
3. `NoriEvent::ReplayFinished`.

Transcript replay applies these rules:

- Entries marked `historical_evidence` are excluded before anything else and
  cannot suppress the legacy fallback below.
- Raw `user_message_chunk` notifications supersede the `user` entry with the
  same message ID, which keeps attachment blocks without duplicating the
  prompt. A `user` entry without matching chunks is projected to a
  user-message notification at its recorded position.
- Legacy `assistant` entries are projected to agent message and thought
  notifications only when the file contains no stored ACP session
  notification.
- Stored notifications are emitted unchanged except that their session ID is
  retargeted to the active session. Notifications inside earlier recorded
  replay brackets replay again, which is how a resumed transcript keeps its
  older history.
- Stored Nori events, ACP requests, and ACP responses are never replayed, so
  historical requests cannot repeat side effects and historical responses
  cannot complete live requests.

Agent-sourced replay from `session/load` follows the same outward rule: the
markers bracket the load-time notifications in agent order, and the current
load response stays outside the brackets, before `SessionStarted`.

## Resume

Resume reads `session_meta.acp_session_id` and picks the first path the agent
supports
([`AcpBackend::resume_session`](../nori-rs/harness/src/backend/session.rs)):

| Agent capability | Path | History source |
| --- | --- | --- |
| `loadSession` | `session/load` with the recorded ACP session ID | Agent replay (`ReplayStarted` source `agent`) |
| `session/resume` | Live reattach with `session/resume` | None replayed; a failure is returned, not retried with `session/new` |
| Neither, or `session/load` failed with a transcript available | `session/new`, then transcript replay | Transcript replay (source `transcript`) plus a summary prepended to the first prompt |

The TUI resume picker and `nori resume` open the selected transcript in a new
conversation with a new file. When that file starts, Nori first copies the old
transcript's context-timeline evidence into it as `historical_evidence`, then
records the replay normally.

### Restarting a failed session

[`SessionManager::resume`](../nori-rs/harness/src/session_manager/mod.rs)
restarts a failed session under its existing conversation ID (session list
Ctrl+R, or host `_nori/session/resume`). Because the conversation ID equals the
transcript's `session_id`, the Harness continues the same file through
[`TranscriptRecorder::resume_same_session`](../nori-rs/harness/src/transcript/recorder.rs):

- It keeps the path and original metadata, updating only `acp_session_id`.
- It writes the seed to a temporary file in the same directory, syncs it, and
  renames it over the original. Readers never see an empty or partial file,
  and a writer still holding the old file cannot append to the replacement.
  The directory itself is not synced after the rename.
- If this recorder cannot be created, activation fails instead of running
  without history. A new session whose recorder fails only logs a warning and
  runs without a transcript.

The seed depends on the history source:

| Resume history | Seed |
| --- | --- |
| `session/load` replayed conversation content | Archived context evidence, then the agent replay in replay brackets |
| Transcript fallback, or a load without conversation content | Every existing entry, with its original observation metadata |

Agent replay can include messages newer than the local file, so it wins when
present. That seed drops the older `user` lines: first-prompt previews, turn
counts, and `records()` see only `user` lines written after the restart.
Partial replay from a failed load never replaces the local seed.

The [runtime](../nori-rs/harness/src/runtime.rs) still publishes the replay to
rebuild views and feed subscribers but does not record it twice; new live
events append normally. Envelope sequence numbers continue in memory for the
conversation ID and are not restored after a host restart. Before publishing
`SessionEnded`, the runtime drains and closes the writer (flush, not `fsync`),
so an immediate restart reads the complete prior stream.

## Legacy v1/v2 entries

Older files can contain `assistant` (text and thinking blocks), `client_event`,
`tool_call`, `tool_result`, and `patch_apply` entries. Readers must accept
them. Live recording never produces them, though a fork copies them from a
legacy parent. Their storage types stay private to `nori-harness` and are not
re-exported from `nori-protocol`.

`Transcript::records()` exposes user text, legacy assistant and thinking text,
and exact v3 `SessionEvent` values. It skips `session_meta`, archived evidence,
`client_event`, and legacy tool and patch entries. Decoding is tolerant:
unknown fields are ignored, and there is no version-specific decoder API.

## Context timeline and token usage

There is no Nori token-count or checkpoint entry. The
[context timeline](../nori-rs/harness/src/context_timeline/docs.md)
reconstructs prompt start and end readings from raw ACP usage, correlated
responses, and Nori lifecycle events. `Transcript::context_timeline()` uses the
same reducer as the live `HarnessHandle::context_timeline().await`. A boundary
keeps the latest reading available at that time; a late update never rewrites
a completed checkpoint.

- A resume into a fresh file archives the old file's timeline evidence with
  `historical_evidence`, so the new file reconstructs prior checkpoints without
  reading its ancestor.
- A fork seeds the child from the parent and keeps observation identity.
- Repeated resume never turns archived or replayed evidence into new turns.
- A resumed unfinished turn gets an unknown end reading. Transcripts with only
  legacy entries report incomplete history.
- Shrink, compaction, or a changed context capacity breaks comparison with the
  previous baseline without deleting recorded checkpoints.

Goodbye-card token statistics come from the agent's own transcript files
(`nori-rs/harness/src/transcript_discovery.rs`); those formats are outside this
reference and create no timeline observations.

## Reading transcripts programmatically

Use `nori_harness::TranscriptLoader` (also exported under
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

`Transcript.meta` exposes the parsed `session_meta`. Do not depend on the
private storage enum. A third-party producer that writes JSONL directly must
version its integration against this document and the ACP schema selected by
`nori-protocol`; serialized public events are not a frozen storage ABI.
