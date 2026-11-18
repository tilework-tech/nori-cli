# Noridoc: Background session host

Path: @/nori-rs/cli/src/host

### Overview

- Implements `nori host`, its administrative commands (`ls`, `stop`, `new`),
  and `nori attach` discovery so agent sessions can outlive every terminal.
- Local sessions use the host's start folder or a folder supplied by the client.
  A machine can enable [managed launch admission](#managed-launch-control-v1)
  to run each binding's pinned adapter in its provisioned workspace and home.
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
- In local mode, [HostService](service.rs) implements `ManagedAgent`, delegates envelope
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

#### Local host and attach

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
  only when it is advertised; in `nori attach` against an older host a plain
  `/new` sends no folder and a named folder is refused client-side ("does not
  support session folders"). `nori host new` refuses to run against such a
  host, which would also refuse its observing client.
- A new session becomes the host's selected session (the target of requests
  that omit `session_id`), whoever started it.
- `nori host new [--cwd DIR] [--agent AGENT] [--prompt TEXT] [--json]`
  connects as an observer (locally discovered, or `--host` with
  `--token-file`), starts one session, and prints its ID, or the reply object
  with `--json`. `--cwd` is resolved against the caller's directory before
  sending, and the host checks the folder exists on its machine. Without
  `--cwd` the local host starts the session in the caller's directory; a host
  named with `--host` may be another machine, so it uses its own folder.

#### Managed launch control v1

**Start the machine host.** Set the following in the host's
`$NORI_HOME/config.toml`. The provisioner creates the absolute bindings
directory before startup. For the example below it also creates
`sample/work` and `sample/home` under that directory before ensure.

```toml
[host]
managed = true
max_sessions = 2
bindings_root = "/srv/fixture/bindings"
```

Run `NORI_HOME=/srv/fixture/nori nori host --port 0` to start the detached
loopback listener. Use the same `NORI_HOME` with `nori host stop` to stop
all its children. The defaults are `managed = false`, a managed capacity of
eight live adapter processes, and `$NORI_HOME/hosted-sessions` for the
bindings root. A configured capacity must be positive. Local mode retains
its own session cap and attach protocol.

A dedicated machine can retain its existing home and workspace. Configure
one named binding with `max_sessions = 1` and omit `bindings_root`. Before
publishing its endpoint, the host validates the configured binding ID, checks
that the absolute paths resolve to distinct existing directories, and checks
the home configuration directories described below. An invalid layout fails
startup. Ensure must supply the configured binding ID and resolve to the same
canonical pair. The workspace may be inside the home.

```toml
[host]
managed = true
max_sessions = 1

[host.dedicated_roots]
binding_id = "sample"
home = "/srv/fixture/home"
cwd = "/srv/fixture/home/org/workspace"
```

**Discover and authenticate.** Read `endpoint` from
`$NORI_HOME/host/state.json` and the bearer token from `$NORI_HOME/host/token`.
Connect to that WebSocket URL (`ws://127.0.0.1:<port>/sessions`) with
`Authorization: Bearer <token>` on the upgrade. Only the privileged machine
adapter should receive this credential. It authorizes operations on every
binding and host-wide stop. The upgrade response's `x-nori-host-instance`
identifies this host incarnation. Both that identity and the token change
when the host restarts.

Each text frame carries one JSON-RPC 2.0 object. Send
`{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"observe":true}}`
first. Require `result.agentCapabilities._meta["nori.dev/managedHost"]`
to contain `{"version":1,"maxSessions":2,"standardAcp":1}` for the shared configuration above
(or `maxSessions: 1` for the dedicated configuration).
The ordinary local host omits that key. The `standardAcp` field advertises version 1 of the scoped ACP transport at
`/managed`. The control socket accepts `_nori/session/ensure`,
`_nori/session/status` and `_nori/host/stop` from any authenticated,
initialized connection. The transport also implements its existing
`_nori/control/status` and `_nori/control/take` role operations.

**Ensure a process.** ACP session setup requires a running adapter, so the
machine sends this administrative request before the ACP client starts its
own handshake.

```json
{
  "jsonrpc": "2.0",
  "id": 2,
  "method": "_nori/session/ensure",
  "params": {
    "commandId": "launch-sample",
    "conversationId": "00000000-0000-4000-8000-000000000001",
    "bindingId": "sample",
    "generation": 1,
    "spec": {
      "projectId": "fixture-project",
      "runId": "fixture-run",
      "agent": "fixture",
      "adapter": {"path": "/opt/fixture/1.2.3/bin/fixture-acp", "version": "1.2.3"},
      "cwd": "/srv/fixture/bindings/sample/work",
      "home": "/srv/fixture/bindings/sample/home",
      "additions": {"promptEcho": false, "statusEvents": false, "recording": false, "clientMcp": false}
    }
  }
}
```

- The caller supplies a stable conversation UUID, a nonblank command ID, a
  positive integer generation, and a binding ID containing ASCII letters,
  digits, `.`, `-`, or `_`. Empty, `.`, and `..` binding IDs are invalid.
  The conversation ID names the hosted conversation. ACP session IDs belong
  to the downstream adapter and a binding can carry several of them.
- The provisioner installs the versioned executable, protects it from
  replacement, resolves credential references and prepares binding-owned
  paths. The host executes the absolute `adapter.path` directly with
  `adapter.args` (optional string array, default empty). The version string
  identifies that installation in the spec. The host performs no download
  or binary-version probe. Keep binary selection and launch requests in
  trusted machine code.
- The existing session-folder validator canonicalizes the absolute `cwd`
  and `home`. Both must exist and be distinct directories. In the shared
  layout both must be strictly beneath `bindings_root/bindingId`, and the
  binding directory must resolve to its designated location. In either
  layout existing `.codex`, `.nori`, and `.nori/cli` entries under the home
  must be directories. The host rejects symlinks at those entries, including
  dangling links. The provisioner must control directory
  replacement during launch. Same-UID filesystem access remains subject
  to OS permissions.
- Optional fields inside `spec` are `model` (string or null), `env`
  (string-to-string map), `credentialRefs` and `resumeArtifacts` (string
  arrays). Missing maps and arrays default to empty. Project, run, agent,
  model, credential and resume metadata stay with the immutable spec.
  The executable receives only its arguments, working directory and child
  environment. Unknown request or spec fields are rejected.
- The host clears inherited environment, applies `spec.env`, and sets
  `HOME=home`, `CODEX_HOME=home/.codex` and `NORI_HOME=home/.nori/cli`.
  Those names and `USERPROFILE` are reserved case-insensitively. Adapter
  arguments and environment entries must be free of NUL characters.
  Supply variables such as `PATH` explicitly when the adapter needs them.
  Subprocesses inherit the binding environment. Host `.env` values stay
  in the host. Managed launches bypass host-configured hooks and MCP servers.
- All four `additions` booleans are required. With `standardAcp: 1`,
  each independently controls its host service. See the [machine wire and
  switch inventory](../../../acp-host/docs.md#managed-machine-connection).

**Interpret replies.** A successful OS spawn returns a receipt immediately,
even if the child exits before the caller receives it. For the example above
the JSON-RPC `result` has the following fields.

```json
{
  "bindingId":"sample",
  "generation":1,
  "conversationId":"00000000-0000-4000-8000-000000000001",
  "specDigest":"be38062d7d4f3978fff3a223d10370562d2ff9ea813eaf46891b7b434a6b1662",
  "state":"spawned"
}
```

The host computes `specDigest` as lowercase hex SHA-256 of its serialized
typed spec, including defaults. Treat it as opaque and store it with the
receipt. Matching concurrent and later ensures return the original receipt,
including at capacity and after exit. Reusing a binding with a different
generation, conversation ID or spec, or assigning
the conversation ID to another binding, returns a conflict. Launch identity
is determined by the binding, generation, conversation UUID and spec digest,
independently of the command ID. Preserve the supplied paths on retry. The
digest covers their original spelling before filesystem canonicalization.

Send `_nori/session/status` with `{"bindingId":"sample","generation":1}`
to obtain the same identity and the observed state.

| Status result | Caller interpretation |
| --- | --- |
| `spawned` | OS spawn succeeded. The host has yet to observe exit. ACP initialization can still fail. |
| `exited` | OS spawn succeeded and the child subsequently exited. `exitCode` is present when the OS provided a numeric exit code. |
| `spawnFailed` | The admitted OS spawn failed. Ensure and identical retries return an error and preserve that outcome. |
| `-32000` for an unknown binding or generation | The host has no record for the requested pair. |

If the first acknowledgement is lost, reconnect, initialize and verify that
`x-nori-host-instance` still matches. Repeat the original ensure with its
stored parameters. The host checks the immutable identity and returns the
original outcome, or performs the launch if the first request never arrived.
Use status for the current process state, even when the recovered receipt
says `spawned`. Reconciliation after a host restart requires external durable
state because receipts live in memory.

Use the JSON-RPC `error.code` to handle an ensure refusal.

| Code | Meaning | Caller action |
| --- | --- | --- |
| `-32017` | The request is valid but all live process slots are occupied. This refusal creates no record. | Try another machine. |
| `-32602` | The ensure parameters failed decoding or admission validation. This refusal creates no record. | Correct the request or its provisioned paths. |
| `-32000` | Other launch failure, including an immutable identity conflict or a failed OS spawn. | Inspect the diagnostic and query status for the binding and generation. A failed spawn retains its record. |

**Handoff to the ACP relay.**
`nori_harness::managed_launch::ManagedLaunches::take_connection(binding, generation)`
hands the relay a `LaunchConnection` with the stable conversation ID,
original immutable spec, validated canonical `cwd` and `home`, and the
child's original stdin/stdout. It succeeds once for an admitted pair with a
live process. The logical ACP client sends the first downstream
`initialize` and owns session setup. The host keeps the process handle,
observes exit and terminates managed processes during shutdown. See the
[admission implementation](../../../harness/src/managed_launch.rs),
[loopback cases](../../tests/host_lifecycle/managed.rs), and
[pipe handoff test](../../../harness/tests/managed_launch.rs).

On Linux, provision a PID namespace init or ancestor subreaper that waits for
orphaned processes. The host signals the adapter's process group and waits
for its direct child. Orphaned descendants can remain as zombies until their
adopting ancestor reaps them. See [process supervision](../../../acp-host/src/connection/child_lifecycle.rs).

### Things to Know

- Tokens are mandatory even for loopback. Local attach reads the private token
  automatically and receives a callback that rereads it with the same private
  file validation after a reconnect authentication failure. Explicit endpoints
  require `--host` together with `--token-file` and receive no local credential
  rediscovery callback. Token contents never appear in startup output.
- The authenticated listener serves `/sessions` and adds `/managed` when
  managed launches are enabled. For the local attach protocol, see the
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
