# Noridoc: Background session host

Path: @/nori-rs/cli/src/host

### Overview

- Implements `nori host`, its administrative commands (`ls`, `stop`, `new`),
  and `nori attach` discovery so agent sessions can outlive every terminal.
- Each host session works in its own folder: the host's start folder by
  default, or any existing folder a client names when starting it.
- Owns Unix process lifetime and private host files; protocol framing remains
  in [acp-host](../../../acp-host/docs.md), and session policy remains in the
  [harness](../../../harness/docs.md).

### How it fits into the larger codebase

- [CLI dispatch](../main.rs) resolves ordinary Nori configuration for startup
  and attach, first making the command-line `-C` and `--add-dir` paths
  absolute. Startup keeps those command-line overrides alongside the resolved
  configuration so the host can reapply them in another folder.
  Administrative list, stop, and new only need host discovery, so they
  remain available if the configuration file becomes invalid. Attach requires
  `multi_session` and invokes the dedicated
  [terminal frontend](../../../tui/src/attach/docs.md), bypassing the
  local process-owning TUI application.
- [HostService](service.rs) implements `ManagedAgent`, delegates envelope
  operations to `ManagedRemoteHost`, and adds lifecycle commands over the same
  `SessionManager`. Agent initialization and transcript recording follow the
  ordinary harness path.
- The host starts with an empty manager and retains it until explicit stop or
  process termination. Ending the last session never ends the host.

```text
host command -> detached host -> authenticated /sessions -> HostService
                                   ^                         |
attach TUI ------------------------+                         v
host ls / stop / new --------------+                   SessionManager
                                                             |
                                                       agent processes
```

### Core Implementation

- [Startup](mod.rs) launches the same executable with a private foreground
  argument, starts a Unix process session, disconnects stdin, and redirects
  output to the host log. The launching command waits for an authenticated
  readiness check before reporting success.
- [HostFiles](state.rs) creates the host directory beneath `NORI_HOME` with
  owner-only permissions. It validates ownership, rejects file symlinks and
  nonregular files, and holds an OS file lock for the entire host lifetime.
  Discovery state contains the endpoint and PID; the lock decides liveness.
- Each start generates a token and binds loopback before publishing discovery
  state. Administrative clients initialize with `observe: true`, so list and
  stop do not claim the controller slot. [List formatting](table.rs)
  sanitizes control characters and has two shapes:

  | Output | Shape |
  | --- | --- |
  | Terminal | The shared two-row session layout from [`session_table`](../../../tui-components/src/session_table.rs), the same as the local session list and the `nori attach` list: a `Session`/`Agent`/`Status` header, then per session a row with status glyph, tree guide and title, agent, status word, and header-less context (`8% / 475k`), and a faint row with the start of the session's short folder then ` · ` and the start of the agent's last reply (`no reply yet` when absent; a row whose own status is `waiting_on_you` shows its `waiting_request` instead; see `session_table::folder_and_reply`), the model, and `2m ago` from `idle_seconds`. No ID column; `nori host ls \| cat` prints full IDs. Column widths follow the terminal width as described in the [tui-components docs](../../../tui-components/docs.md). |
  | Piped | One row per session under `ID  AGENT  STATUS  AGE  CWD  TITLE`, with full IDs, full values (CWD is the full canonical folder, its column as wide as the longest), no width limit, and no colour. AGE comes from `age_seconds`. |

  On a terminal the glyph and status word are coloured, row 2 is dimmed, and
  each session's first row is shaded (its second keeps the terminal's own
  background, so every other line is striped) when the terminal reports its
  background and supports 24-bit colour; [dispatch](mod.rs) asks
  `nori_tui::session_list_shade` (the theme's `row_stripe` colour, matching the
  TUI session lists) for that colour only when stdin and stdout are
  both terminals (the query is answered on stdin), and with no known background no row is shaded. The
  status glyph and title are cut together, in the middle at grapheme
  boundaries, to the same room the shared picker gives its Session cell, so a
  title is cut at the same point here as in the TUI lists and titles sharing a
  long prefix still read apart; other cells are cut from the end with `…`.
- A top-level row (tree depth 0) shows the row's `family_status` when the host
  sends one, so a parent reads `waiting on you` while anything under it does;
  child rows always show their own `status`. Older hosts omit
  `family_status`, and the row falls back to `status`.
- Rows are tree-ordered with the shared
  [picker helpers](../../../tui-components/docs.md): `tree_order` builds a
  depth-first order from each row's `session_id` and `parent` (a missing
  parent makes the row top level), and `tree_guides` supplies `├─`, `└─`, and
  `│` prefixes plus the continuation used on row 2. The guides sit inside the
  title column, so nesting never shifts metadata. Piped output appends
  `  (parent <full parent ID>)` to every row that has a parent, including a
  child whose parent is no longer listed and so prints at top level, so scripts
  keep the exact link; terminal output omits it.

  | Status | Terminal color |
  | --- | --- |
  | waiting on you | yellow |
  | working | cyan |
  | ready | green |
  | failed | red |
  | done | default color, so a finished child reads apart from a closed one |
  | others (starting, closed) | dimmed |
- `nori attach <id>` accepts any unique start of a session ID.
  [Dispatch](mod.rs) passes an ID already in the host's spelling (lowercase,
  hyphenated UUID) through unchanged; anything else is resolved before the
  attach TUI starts by an observing `_nori/session/list` request (3 s timeout)
  to the same endpoint (local or `--host`) and
  [`session_lookup`](session_lookup.rs), which matches case-insensitively and
  normalises a full ID typed in any case, with or without hyphens. An
  exact ID wins; several matches fail with the candidates (ID, agent, title)
  listed; no match fails with a pointer to `nori host ls | cat` (full IDs) and
  to `nori attach` without an ID. The host itself only knows full IDs.
- Lifecycle requests prepare agents using the host's configuration.
  `_nori/session/new` takes optional `cwd`, `agent`, and `prompt` strings and
  replies `{session_id, cwd}`. A `cwd` must be absolute; [HostService](service.rs)
  canonicalizes it and refuses a relative path, a missing folder, or a
  non-directory with a readable error. For a requested folder the host reloads
  `NoriConfig` with that folder as cwd plus its own stored command-line
  overrides, so the folder's project trust and approval settings apply rather
  than the host's start folder's. Without `cwd` the host's own configuration is
  used. A `prompt` is sent as the first prompt from a spawned task after the
  session starts, so the reply does not wait for it; a rejected prompt is only
  logged. Resume reloads configuration the same way from the record's cwd,
  then applies the stored agent, retaining the conversation identity. Stop
  closes the server, shuts down the manager, removes discovery state, then
  releases the lock.
- The host advertises folder support by adding `session_cwd: true` to
  `capabilities.nori_sessions` in its `initialize` reply. Clients send `cwd`
  only when it is advertised; against an older host a plain start sends no
  folder and an explicit folder is refused client-side ("does not support
  session folders").
- `nori host new [--cwd DIR] [--agent AGENT] [--prompt TEXT] [--json]`
  connects as an observer (locally discovered, or `--host` with
  `--token-file`), starts one session, and prints its ID, or the reply object
  with `--json`. `--cwd` and its default (the current directory) are resolved
  against the caller's directory before sending, so scripts address remote
  hosts the same way; the host still checks the folder exists on its machine.

### Things to Know

- Tokens are mandatory even for loopback. Local attach reads the private token
  automatically and receives a callback that rereads it with the same private
  file validation after a reconnect authentication failure. Explicit endpoints
  require `--host` together with `--token-file` and receive no local credential
  rediscovery callback. Token contents never appear in startup output.
- The authenticated listener exposes only `/sessions`; it does not change the
  legacy interactive remote-control listener. See the
  [wire contract](../../../../docs/remote-control.md#11-background-host-and-attached-terminals).
- Delegation requests pass through `ManagedRemoteHost` to the same manager
  used by local sessions. Child sessions consume the ordinary session cap
  and inherit the parent's backend configuration, so they work in the
  parent's folder. Completing a child releases its process slot but preserves its listed record
  and transcript, and the row then reads `done` rather than `closed`. Closing a parent cancels descendants, while a parent crash
  preserves children and holds their results for stable-ID resume.
- The host has no persistent manager snapshot. Transcripts survive normally,
  but in-memory registry state and reconnect replay belong to the current host
  process. After a restart, earlier sessions, including finished delegated
  children, are not listed; `/resume` on a child's transcript reopens it as a
  live top-level `ready` session without its parent link. The token rotates when that process starts again.
- [Lifecycle tests](../../tests/host_lifecycle.rs) use isolated homes and
  bounded process waits to cover detached startup, privacy, lock ownership,
  occupied ports, and explicit stop.

Created and maintained by Nori.
