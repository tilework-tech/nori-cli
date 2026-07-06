# Nori transcript format

Reference for Nori's on-disk session transcript format, for tools that want to
read or write Nori transcripts without depending on the `nori-harness` crate.
Transcripts capture the client-side view of a conversation: what the user
typed, what the assistant responded, and which tools ran.

Canonical implementation: `nori-rs/harness/src/transcript/` (crate
`nori-harness`) — `types.rs` (schema), `recorder.rs` (writer), `loader.rs`
(reader), `project.rs` (project id derivation).

## File locations

Transcripts live under the Nori home directory:

- `$NORI_HOME` if set, otherwise `~/.nori/cli` (see `nori-config`).

Layout:

```text
$NORI_HOME/transcripts/by-project/{project-id}/
  ├── project.json            # Project metadata
  └── sessions/
      └── {session-id}.jsonl  # One file per session
```

- `{session-id}` is a UUIDv4 generated at session start.
- Session files are created with mode `0600` on Unix.
- The file is created fresh (truncated) when the session starts, then
  appended to line by line. Each line is flushed after writing, but the file
  is never fsynced, so the tail of a crashed session may be missing.

### Project ids and `project.json`

The project id is 16 lowercase hex characters derived by hashing, in priority
order:

1. the normalized git remote URL of `origin` (e.g.
   `git@github.com:user/repo.git` and `https://github.com/user/repo.git` both
   normalize to `github.com/user/repo`), else
2. the git root absolute path, else
3. the canonicalized working directory path.

The hash is Rust's `DefaultHasher`, which is **not guaranteed stable across
Rust versions**. Treat project ids as opaque directory names: discover
projects by listing `by-project/` and reading each `project.json`, and do not
try to recompute ids yourself.

`project.json` (pretty-printed JSON, rewritten on every new session in the
project):

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

`git_remote` and `git_root` are `null` outside a git repo. Because the file is
rewritten each session, `created_at` reflects the most recent session, not the
first.

## Line format

Session files are JSONL: one self-contained JSON object per line, `\n`
terminated. Every line shares an envelope, with the entry's fields flattened
into the same object:

- `ts` — ISO 8601 timestamp with millisecond precision, UTC (`Z` suffix),
  e.g. `"2026-07-03T12:30:45.123Z"`.
- `v` — schema version, currently `2`.
- `type` — the entry kind (snake_case), plus the entry's own fields.

The **first line must be a `session_meta` entry**; readers reject files whose
first line is not valid session metadata.

### `session_meta` (first line)

```json
{"ts":"2026-07-03T12:30:45.123Z","v":2,"type":"session_meta","session_id":"7f9c2f6a-1c1e-4a9b-9a3e-2f0d8b7c6d5e","project_id":"a1b2c3d4e5f60718","started_at":"2026-07-03T12:30:45.123Z","cwd":"/home/user/src/my-repo","agent":"claude-code","cli_version":"0.9.0","git":{"branch":"main","commit_hash":"1975265abc..."},"acp_session_id":"acp-sess-abc123"}
```

Optional fields, omitted when absent: `agent` (ACP agent slug, e.g.
`claude-code`, `codex`, `gemini`), `git` (and within it `branch`,
`commit_hash`), and `acp_session_id` (the ACP agent's own session id, used to
resume via `session/load`).

### `user`

```json
{"ts":"2026-07-03T12:31:02.001Z","v":2,"type":"user","id":"msg-001","content":"What files are in src?","attachments":[{"type":"file_path","path":"/tmp/screenshot.png"}]}
```

`attachments` is omitted when empty. Each attachment is tagged by `type`:
`file_path` (`path`) or `base64` (`data`, `mime_type`).

### `assistant`

```json
{"ts":"2026-07-03T12:31:10.500Z","v":2,"type":"assistant","id":"msg-002","content":[{"type":"thinking","thinking":"Let me check src."},{"type":"text","text":"src contains main.rs and lib.rs."}],"agent":"claude-code"}
```

`content` is a list of blocks tagged by `type`: `text` (`text`) or `thinking`
(`thinking`). `agent` is optional.

### `tool_call` / `tool_result`

```json
{"ts":"2026-07-03T12:31:05.000Z","v":2,"type":"tool_call","call_id":"call-001","name":"shell","input":{"command":"ls src/"}}
{"ts":"2026-07-03T12:31:05.250Z","v":2,"type":"tool_result","call_id":"call-001","output":"main.rs\nlib.rs","exit_code":0}
```

`input` is arbitrary JSON. On results, `truncated` (bool) is omitted when
false and `exit_code` is omitted when not applicable. `call_id` correlates
call and result.

### `patch_apply`

```json
{"ts":"2026-07-03T12:31:20.000Z","v":2,"type":"patch_apply","call_id":"call-002","operation":"edit","path":"/home/user/src/my-repo/src/main.rs","success":true}
```

`operation` is `edit`, `write`, or `delete`. `error` (string) is present only
on failure.

### `client_event`

Wraps a normalized ACP-native event from the `nori-protocol` crate. The
payload lives under `event` and is internally tagged by `event_type`
(snake_case), with the variant's fields flattened alongside the tag:

```json
{"ts":"2026-07-03T12:31:06.000Z","v":2,"type":"client_event","event":{"event_type":"tool_snapshot","call_id":"call-001","title":"Edit src/main.rs","kind":"edit","phase":"completed","locations":[],"invocation":null,"artifacts":[],"raw_input":null,"raw_output":null}}
```

Variants (see `nori-rs/nori-protocol/src/lib.rs` for payload shapes):
`tool_snapshot`, `approval_request`, `message_delta`, `plan_snapshot`,
`session_phase_changed`, `prompt_completed`, `load_completed`,
`queue_changed`, `context_compacted`, `replay_entry`,
`agent_commands_update`, `session_capabilities_changed`,
`session_update_info`, `session_config_update`, `session_mode_changed`,
`thread_goal_updated`, `thread_goal_cleared`, `warning`. This set tracks the
protocol crate and changes more often than the core entry types; readers
should be prepared to skip unknown `event_type`s.

## Token usage

Nori transcript entries do not carry token counts. The only usage data that
can appear is the optional `usage` field inside `session_update_info` client
events. The token statistics shown in the TUI footer are parsed from the
underlying agent's *own* transcript files (`~/.claude/projects/`,
`~/.codex/sessions/`, `~/.gemini/tmp/`) by
`nori-rs/harness/src/transcript_discovery.rs`; those are third-party formats
and out of scope here.

## Versioning and compatibility

- `v` is currently `2`. The loader does not branch on it; it exists for
  forward compatibility.
- **Unknown fields are ignored** (no `deny_unknown_fields` anywhere in the
  schema).
- **Unknown or unparseable lines are silently skipped** by the full-transcript
  loader — except the first line, which must parse as `session_meta` or the
  load fails.
- Blank lines are skipped.
- Lightweight scanners (session pickers, first-user-message preview, user-turn
  counts) inspect only the `type` field per line, so extra entry kinds never
  break listing.

Writer expectations for third-party producers:

- One JSON object per line, no pretty-printing, `\n` terminated.
- First line is `session_meta` with at minimum `session_id`, `project_id`,
  `started_at`, `cwd`, `cli_version` (plus the `ts`/`v`/`type` envelope).
- Name the file `{session_id}.jsonl` and keep `session_id` consistent between
  the filename and the metadata — loaders derive paths from the id.

## Reading transcripts programmatically

If you can take a Rust dependency, use the `nori-harness` crate
(`nori-rs/harness`): `TranscriptLine` / `TranscriptEntry` are the canonical
serde types, `TranscriptLoader` handles discovery, listing, and
tolerant parsing, and `TranscriptRecorder` is the canonical writer. Anything
this document says is a simplification of those types.
