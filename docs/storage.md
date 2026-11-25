# Storage reference

This page lists every file and folder the Nori CLI keeps under `NORI_HOME`,
with its mode and lifetime, and the transcript format. Terms follow
`GLOSSARY.md`: a transcript belongs to a **conversation**, the durable thread
that outlives a session or a process.

`NORI_HOME` is `$NORI_HOME` when set, else `~/.nori/cli`. Modes apply on
Unix. "umask" means the file is created with the process umask.

## Layout

```text
$NORI_HOME/
├── config.toml                    configuration
├── .config.toml.lock              configuration edit lock
├── .env                           extra environment variables
├── credentials.json               saved API keys (file store)
├── history.jsonl                  prompt history
├── nori-version.json              update check cache
├── .nori-install.json             install state
├── transcripts/by-project/<project-id>/
│   ├── project.json
│   └── sessions/<conversation-id>.jsonl
├── log/                           logs
├── acp-wire/                      ACP wire recordings
├── host/                          background host files
├── commands/                      custom prompts
├── browser-profile/               persistent Chrome profile
├── token-providers/               Pi agent profiles
├── .pi-launches/                  Pi agent launch folders
└── hosted-sessions/               managed host launches
```

## Files and folders

| Path | Mode | Written by | Lifetime |
|---|---|---|---|
| `config.toml` | `0600` when Nori creates it; an existing file keeps its mode | You; Nori's own edits ([configuration.md](configuration.md#keys-nori-writes)) | Permanent. Its absence marks the first launch. |
| `.config.toml.lock` | umask | Every configuration edit | Permanent; never removed. |
| `.config.toml.<pid>.<n>.tmp` | | Configuration edits | Renamed over `config.toml` at once. |
| `.env` | | You | Read at every start. Names starting with `CODEX_` are ignored. |
| `credentials.json` | `0600` | `nori auth set-key`, `remove-key` | Permanent. Written atomically. Used when `NORI_CREDENTIALS_STORE` is `file` (the default); with `keyring`, keys live in one OS keyring item, service "Nori Credentials". See [auth.md](auth.md). |
| `saved-keys.json` | | Older development builds | Not read. While it exists, every credential command fails and asks you to delete it. |
| `auth.json` | | Not written by `nori` | Read by the inherited login manager when present. |
| `history.jsonl` | `0600` | Each prompt sent | Append-only; Nori never trims it. Not written when `history_persistence = "none"`. |
| `nori-version.json` | umask | Update check | Rewritten after each check (at most every 20 hours). |
| `.nori-install.json` | umask | Every TUI launch | Permanent. Written atomically. |
| `transcripts/` | umask | Session recording | See [Transcripts](#transcripts). |
| `log/nori-acp.<YYYY-MM-DD>.log` | umask | Every `nori` invocation | One file per day. Nori never deletes old files. Filter: `RUST_LOG`. |
| `log/nori-tui.log` | `0600` | Created by every TUI start | Appended. Only `nori-tui` writes to it; in `nori`, TUI logs go to `nori-acp.<date>.log`. |
| `log/session-<YYYYMMDDTHHMMSSZ>.jsonl` | umask | TUI, when `CODEX_TUI_RECORD_SESSION` is set | One per TUI run. `CODEX_TUI_SESSION_LOG_PATH` moves it. |
| `acp-wire/<timestamp>-<pid>-<agent>.jsonl` | | Each agent process, when `[acp_proxy] enabled = true` | One per agent process. Holds raw ACP JSON-RPC, which can include prompts and secrets. Nori never deletes it. |
| `host/` | `0700` | Background host | See [Host files](#host-files). |
| `commands/` | | You | Custom prompts: `.md` files are prompt text; `.sh`, `.py`, and `.js` files are scripts limited by `[tui] script_timeout`. A host lists its own to every terminal on it and runs their scripts in the session's folder. |
| `browser-profile/` | | `/browser` with `browser_profile = "persistent"` | Permanent; keeps logins. The default `throwaway` profile is a temporary folder outside `NORI_HOME`, deleted on exit. |
| `token-providers/<name>-pi-acp/sessions/` | `0700` | Pi agent launches bound to a token provider | Permanent; the Pi agent's own session history. |
| `.pi-launches/` | `0700` | Pi agent launches bound to a token provider | One folder per launch, holding `models.json` and `settings.json` (`0600`), with a `.lease` lock file and a `.lock` file. A later launch deletes a folder once no process holds its lease. |
| `hosted-sessions/` | | The launcher, not Nori | Default `[host] bindings_root` of a managed host. It must exist; Nori does not create it. |

Nori writes nothing to the Codex home (`~/.codex`) except for the Codex
agent: the TUI's Codex sign-in writes the credentials that agent reads.
See [Where `main` differs](#where-main-differs).

## Host files

The background host keeps its files in `$NORI_HOME/host/`. The folder has
mode `0700`, must belong to the user, and must not be a symlink. Private files
have mode `0600` and are opened without following symlinks. Background hosts
need Unix.

The monorepo host contract owns the meaning of `lock`, `host.json`,
`state.json`, and `token`:
`nori-monorepo: docs/specs/nori-cli/host-contract.md#discovery`.

| Path | Mode | Lifetime |
|---|---|---|
| `lock` | `0600` | Permanent; never removed. See the host contract. |
| `start.lock` | `0600` | Permanent. Held by the `nori` process that starts or checks the host, so only one process starts it. |
| `host.json` | `0600` | Written at start through `host.json.tmp`; removed on stop. A managed host does not write it. See the host contract. |
| `state.json` | `0600` | Written at start; removed on stop and at the next start. See the host contract. |
| `token` | `0600` | Replaced at every start. See the host contract. |
| `host.log` | `0600` | stdout and stderr of a host started by `nori host`. Appended; never trimmed. |
| `session-settings.json` | `0600` | Mode, adapter options, and ACP `mcpServers` per conversation, reapplied on resume. Written atomically through `session-settings.json.tmp` by any host on this home, including the TUI's private host; the write fails with a log warning while `host/` does not exist. An entry is removed when its session is deleted. |
| `recovery/` | `0700` | Managed host only. Recovery journal: `journal.lock` and one `<sha256 of binding ID>.wal` file per binding. The managed wire is in the host contract. |

## Transcripts

Each conversation has one transcript file:

```text
$NORI_HOME/transcripts/by-project/<project-id>/
├── project.json
└── sessions/<conversation-id>.jsonl
```

- `<conversation-id>` is the conversation UUID, the ID `nori resume` takes.
- Transcript files have mode `0600`.
- A file lives until the session is deleted through the host (ACP
  `session/delete`), which removes only files under `transcripts/`. Nothing
  else deletes transcripts.

### Project ID and `project.json`

The project ID is 16 hexadecimal characters: a hash of, in order of
preference, the normalized Git remote URL, the Git root path, or the
canonical working folder. Treat it as an opaque folder name and read
`project.json` for the details.

| Field | Type | Value |
|---|---|---|
| `id` | string | The project ID |
| `name` | string | Folder or repository name |
| `git_remote` | string or null | Git remote URL |
| `git_root` | string or null | Git root path |
| `cwd` | string | Working folder |
| `created_at`, `updated_at` | string | ISO 8601 time |

Nori rewrites `project.json` each time a session starts in the project.

### Line format

A transcript is JSON Lines. Every line is one object with these fields plus
the fields of its record type:

| Field | Type | Value |
|---|---|---|
| `ts` | string | ISO 8601 UTC time with milliseconds |
| `v` | integer | Schema version. Nori writes `3`. |
| `type` | string | Record type |

The first line is `session_meta`. When reading, Nori skips blank lines,
unknown record types, and lines that do not parse, so older and newer files
stay readable.

### Record types

Nori writes `session_meta`, `user`, and `session_event` records. A
transcript seeded from a parent (ACP fork) starts with the parent's records,
so it can hold any record type. The other types are read for older files.

| `type` | Written | Fields |
|---|---|---|
| `session_meta` | yes | `session_id` (conversation ID), `project_id`, `started_at`, `cwd`, `cli_version`; optional `agent`, `git` (`branch`, `commit_hash`), `acp_session_id` (the agent's ACP session ID, for `session/load`), `forked_from` (parent conversation ID after an ACP fork) |
| `user` | yes | `id`, `content`; optional `attachments`: `{type: "file_path", path}` or `{type: "base64", data, mime_type}` |
| `session_event` | yes | `event` (the raw ACP or Nori session event); optional `observation_id`, `observed_at`, `historical_evidence`, `history_incomplete`, `response_kind` (`prompt`, `new_session`, `fork`) |
| `assistant` | read only | `id`, `content` (blocks `{type: "text", text}` or `{type: "thinking", thinking}`); optional `agent` |
| `client_event` | read only | `event` (a normalized client event) |
| `tool_call` | read only | `call_id`, `name`, `input` |
| `tool_result` | read only | `call_id`, `output`; optional `truncated`, `exit_code` |
| `patch_apply` | read only | `call_id`, `operation` (`edit`, `write`, `delete`), `path`, `success`; optional `error` |

## Prompt history

`$NORI_HOME/history.jsonl` holds one line per prompt:

```json
{"session_id":"<conversation-id>","ts":1767225600,"text":"<prompt>"}
```

`ts` is Unix seconds. Each line is written with one append, so concurrent
processes do not interleave lines. Nori resets the file mode to `0600` when it
differs.

A session on a host keeps its prompts in the host's `$NORI_HOME`, not the
terminal's: every terminal attached to that host records into, recalls from and
searches the same file, through `_nori/host/history/*`.

## Files outside `NORI_HOME`

| Path | Written by | Lifetime |
|---|---|---|
| `<repo>/.worktrees/<name>/` | `[tui] auto_worktree` | Git worktree; Nori does not remove it. |
| `~/.codex/` | The TUI's Codex sign-in | The Codex agent's own credentials. |
| `$CODEX_HOME/.credentials.json` (default `~/.codex/.credentials.json`) | MCP OAuth, when the OS keyring is not available | See [Where `main` differs](#where-main-differs). |
| OS keyring, service "Nori TUI MCP Credentials" | MCP OAuth | Until removed. |
| OS keyring, service "Nori Credentials" | `nori auth`, when `NORI_CREDENTIALS_STORE=keyring` | Until removed. |

## Where `main` differs

- The MCP OAuth fallback file is `$CODEX_HOME/.credentials.json` (default
  `~/.codex/.credentials.json`) instead of a file in `NORI_HOME`.
