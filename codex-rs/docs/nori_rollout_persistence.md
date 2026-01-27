# Nori ACP rollout (transcript) persistence specification

This document defines the Nori-specific rollout (transcript) persistence for ACP-backed sessions. It replaces the legacy Codex rollout storage inside `codex-core` for Nori/ACP use while remaining fully separate from Codex message history.

## Goals

- **Store the client-visible transcript**: Persist only what the user and Nori client observed from the ACP session (user inputs, assistant outputs, tool calls/outputs, streaming checkpoints). The stored transcript must be sufficient for the ACP client to render a prior session without replaying agent-side execution details.
- **Project-grouped storage**: Organize transcripts by git project when available, otherwise by the session working directory.
- **Nori-specific home**: Persist under `NORI_HOME` (typically `~/.nori/cli`) instead of `CODEX_HOME`.
- **Independent from message history**: Rollout storage is its own system and file, distinct from any in-memory or protocol message history.
- **Legacy-compatible structure**: The schema mirrors the legacy Codex rollout JSONL behavior for tools/streaming checkpoints so existing rendering logic can be reused.

## Non-goals

- Session resume or replay by the agent is out of scope. This storage only supports viewing previous sessions for now.
- Migration of existing Codex rollout files is out of scope.
- The spec does not change or depend on Codex message history.

## Storage layout

All rollouts are written under `NORI_HOME` using project-grouped directories:

```
$NORI_HOME/
  rollouts/
    projects/
      <project_key>/
        <session_id>/
          transcript.jsonl
          meta.json
```

### Project key

Determine the `project_key` as follows:

1. If a git root is detected from the session `cwd`, use the canonical absolute path of the git root.
2. Otherwise, use the canonical absolute `cwd`.
3. Normalize the path into a safe key:
   - Lowercase ASCII
   - Replace path separators with `__`
   - Replace non-alphanumeric characters (except `_` and `-`) with `-`
   - Trim repeated separators/dashes

Example:

```
/Users/alex/work/monorepo
=> users__alex__work__monorepo
```

This ensures stable grouping across sessions without relying on date-based directories.

### Session identity

Each session has:

- `session_id`: a UUID generated when the session starts (string, hyphenated).
- `created_at`: RFC 3339 timestamp.
- `cwd`: the working directory at session start.
- `project_root`: optional git root path when available.

`meta.json` stores this information for listing and summary purposes. `transcript.jsonl` stores the complete transcript.

## Transcript schema (JSONL)

Each line is a JSON object using the legacy-style rollout envelope:

```json
{
  "timestamp": "2025-01-15T12:34:56.789Z",
  "type": "assistant_message",
  "payload": { /* item-specific */ }
}
```

### Required line fields

- `timestamp`: RFC 3339 timestamp when the client observed the event.
- `type`: item type (string, snake_case).
- `payload`: item-specific object.

### Item types

The Nori ACP rollout uses the following item types, designed to mirror Codex rollout semantics while recording only client-visible events:

| Type | Purpose | Payload fields |
| --- | --- | --- |
| `session_meta` | First line in the file with session metadata. | `session_id`, `created_at`, `cwd`, `project_root?`, `cli_version`, `originator`, `model_provider?` |
| `user_message` | User inputs as seen by the client. | `id?`, `content` (string or structured), `attachments?` |
| `assistant_message` | Assistant outputs as seen by the client. | `id?`, `content` (string or structured), `finish_reason?` |
| `tool_call` | Tool invocation as streamed or finalized to the client. | `id`, `name`, `arguments`, `state` (`streaming`/`final`) |
| `tool_result` | Tool output returned to the client. | `tool_call_id`, `content`, `is_error` |
| `checkpoint` | Streaming or staged partial outputs. | `source` (`assistant`/`tool`), `delta`, `sequence` |
| `event` | Non-message user-visible events (warnings, notices). | `kind`, `message`, `details?` |

Notes:

- `content` should match the ACP client-visible content structure, not internal agent structures.
- `checkpoint` records streaming deltas in the order delivered to the client. This ensures the assistant transcript can be re-rendered without replaying the agent.
- `tool_call` and `tool_result` entries are always stored, even if the user did not ask for verbose output, because they are part of the client-visible transcript.

### Ordering and completeness

- Lines are appended in the exact order delivered to the client.
- `session_meta` must be the first line.
- The assistant transcript can be reconstructed using all `assistant_message` + `checkpoint` entries in order. Tool outputs are included in-place where observed.

## Listing previous sessions

A session picker uses `meta.json` files under `rollouts/projects/<project_key>/` to list sessions by project. The list is sorted by `created_at` descending within each project. No cross-project grouping is required; the UI can present a project list first, followed by sessions for that project.

## Integration points (codex-core replacement)

The ACP rollout recorder in `codex-core` should be replaced so that:

- Storage uses `find_nori_home()` from the ACP config loader (Nori-specific home), never `CODEX_HOME`.
- Recording is enabled only for ACP/Nori sessions (not for the legacy Codex backend).
- The recorder writes `transcript.jsonl` + `meta.json` using this schema and layout.
- Existing helper logic for event filtering, streaming item ordering, or summary extraction can remain in the codebase if it will be useful later, even if unused initially.

## Backwards compatibility

- Nori ACP rollout files are distinct from Codex rollouts and must not share directories or filenames.
- Legacy Codex rollouts remain unchanged and are not read by Nori ACP sessions.

## Open questions

- Should the project key use a hashed path instead of a sanitized path for privacy? If needed, the key can be an opaque hash while preserving the ability to map sessions to a project.
- Should `meta.json` be updated for late-discovered git metadata (e.g., when git becomes available later)?
