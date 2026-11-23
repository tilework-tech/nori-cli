# Noridoc: Background session host

Path: @/nori-rs/cli/src/host

### Overview

- Implements `nori host`, its administrative commands (`ls`, `stop`, `new`),
  and `nori attach` discovery so agent sessions can outlive every terminal.
- Local sessions use the host's start folder or a folder supplied by the client.
  A machine can enable [managed launch admission](#managed-launch-control-v1)
  to run each binding's pinned adapter in its provisioned workspace and home.
  A dedicated managed machine can also serve one launch as plain ACP on the
  host's own [stdin and stdout](#managed-launch-on-stdio).
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
- The [standard ACP face](acp/mod.rs) answers `session/list`,
  `session/load` and `session/resume` on `/sessions` in local mode. The
  socket loop in [acp-host](../../../acp-host/src/remote/host_wire.rs) hands
  each initialized connection to `HostService` as a `HostConnection` (an
  outbound queue, its role, and a close token), and runs load and resume
  concurrently so a slow agent start does not stall the connection.
  [Listing](acp/list.rs) merges manager records with
  [stored sessions](transcripts.rs) (transcripts the host is not running,
  read through `TranscriptLoader::list_stored_sessions`); its cursor is
  `live:<index>` or a keyset over file modification time and ID, so new
  transcripts do not shift later pages. [Attaching](acp/attach.rs) forwards
  one session's journal events to one connection as `session/update`
  notifications under the conversation ID. Stored sessions restart through
  `SessionManager::resume_stored`. `_nori/session/resume` keeps accepting
  stored IDs as an alias. [Session configuration](acp/config.rs) composes the
  host-owned `agent` and `mode` options around the adapter's options, maps
  modes onto the harness approval policy through
  `nori_harness::permission_mode`, and keeps clients' choices in
  `host/session-settings.json` so a restarted session gets them back. A
  changed agent restarts the session's process through
  `SessionManager::replace_agent`. Wire details:
  [remote-control.md §12](../../../../docs/remote-control.md#12-standard-acp-v1-on-the-background-host).
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
  The host applies the agent's token-provider binding from its own config, so
  the key variable must be set in the host's environment, not the caller's.

#### Mid-turn follow-ups

Direct `nori` and the controller in `nori attach` send ordinary Enter immediately when idle. With a composed follow-up and an observed working turn, Enter requests steering and Tab requests the next turn. Completion, modal permission choices and focus navigation retain their own keys.

An external client calls `_nori/session/follow_up` with a fresh JSON-RPC request ID. An example inner request is

```json
{"jsonrpc":"2.0","id":31,"method":"_nori/session/follow_up","params":{"session_id":"SESSION","expected_turn":"TURN","message_id":"correction-1","intent":"steer","prompt":[{"type":"text","text":"Use the blue sample."}]}}
```

| Transport | Session and turn identifiers | Authenticated sender in receipt |
| --- | --- | --- |
| Local host `/sessions` | Initialize with the host bearer and acquire the controller role. Check `result.capabilities.nori_sessions.follow_up == 1`. Use the conversation's `session_id` and the harness `request_id` from prompt admission or its prompting event. | `host-controller`. Observers receive the read-only error. |
| Machine host `/managed` | Discover `nori.dev/followUp.version == 1` in the control initialize result’s `agentCapabilities._meta`. After ensure and ACP initialization, wrap the request in the same `{bindingId,generation,message}` envelope as the original prompt. Use the regular ACP session ID and that prompt's JSON-RPC ID, retaining its string or integer type. Allocate unique prompt IDs throughout the logical session. | `machine-client`, authenticated by the machine bearer and scoped to the admitted generation. |

Send exactly the fields shown. Choose `steer` or `queue` for `intent`, supply nonempty ACP content and assign a stable, nonblank `message_id` of at most 256 bytes per submission. The host supplies the sender from the authorized path. It identifies the calling principal at this transport boundary. The response is a delivery receipt such as `{"message_id":"correction-1","sender":"host-controller","status":"injected"}`.

| Receipt status | What the caller can report |
| --- | --- |
| `injected` | The adapter accepted input for the observed turn. The original prompt still owns output, terminal result, usage and cancellation. |
| `queued` | The host admitted explicit next-turn input to its FIFO. |
| `unsupported_queued` | The host admitted input for the next turn because guarded steering was unavailable. |
| `idle_queued` | The observed turn finished during admission or the guard returned `promptRequired/noRunningTurn`. The host owns the next prompt. |
| `rejected` | The host or adapter refused this delivery, for example because a different turn already owns the session. |
| `unknown` | Delivery could not be confirmed. Keep the message ID for reconciliation. |

The local harness and the machine host use the [same steering guard](../../../harness/src/follow_up.rs). The live ACP v1 initialize result must advertise `_meta.steering.supported == true` and report an exact verified `agentInfo.name` and `agentInfo.version` pair. Verified versions are `@agentclientprotocol/claude-agent-acp` 0.102.0, 0.108.1 and 0.109.0, and `@agentclientprotocol/codex-acp` 2.7.0 and 2.8.0. The standalone CLI follows the upstream releases. A newly resolved adapter version must pass the steering checks before Nori admits it. Antigravity, Grok Build and Pi queue active-turn input for the next turn. Explicit managed launch specifications supply their own adapter path, arguments and version. An unverified version gets `unsupported_queued` until its [wire behavior is checked](../../../../scripts/verify-host-follow-up/README.md#checking-a-claude-adapter-upgrade).

The host sends `_session/steering` with `_meta.steering.idleBehavior = "promptRequired"`. A pending permission still needs its own response. Normal turn completion and settlement of pending permission requests allow the FIFO to drain. With prompt echo enabled, `/managed` emits `_nori/session/follow_up/submitted` before writing a steer. Use its message ID to place the pending input ahead of provider output and update it from the delivery receipt. For host-started queued prompts on `/managed`, correlate `_nori/session/follow_up/started` and `_nori/session/follow_up/finished` using `message_id` and `turn_id`. The finished notification carries the result or error. Local-host clients follow the ordinary managed session envelopes.

When reconnecting, first reestablish the same live runtime. A machine client can reattach to a surviving logical ACP stream when the binding negotiated durable recovery. Repeat the identical request with its original message ID, turn, intent and content to reconcile delivery. The live host returns its retained receipt for an admitted ID. Reuse with different parameters fails. Identical text with distinct IDs represents distinct input. If the host incarnation, session runtime or binding generation changed, retain an uncertain outcome for outstanding input. Reissuing it under a new ID can duplicate delivery. Receipt deduplication is live state. Durable checkpointing waits for admitted queued work to settle. The [independent transport verifier](../../../../scripts/verify-host-follow-up/README.md) exercises both routes against the pinned adapters with loopback provider endpoints.

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
bindings root. Both shared and dedicated machines use the positive
`max_sessions` limit. Each binding's adapter and each rebuilt side
process occupy a slot. Admission checks the machine-wide count and spawns
under the same lock. A rebuilt side frees its slot once its whole sandbox
has exited. Local mode retains its own session cap and attach protocol.

A dedicated machine can retain its existing home and workspace. Configure
one named binding and omit `bindings_root`. Before
publishing its endpoint, the host validates the configured binding ID, checks
that the absolute paths resolve to distinct existing directories, and checks
the home configuration directories described below. An invalid layout fails
startup. Ensure must supply the configured binding ID and resolve to the same
canonical pair. The workspace may be inside the home.

```toml
[host]
managed = true

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
matches `source.hostIncarnation` on new durable records produced by this
host. The header and token change on host restart. Replayed records keep
the original producing incarnation for deduplication and ACK.

Each text frame carries one JSON-RPC 2.0 object. Send
`{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"observe":true}}`
first. Require `result.agentCapabilities._meta["nori.dev/managedHost"]`
to contain `{"version":1,"maxSessions":2,"standardAcp":1}` for the shared configuration above
(or `maxSessions: 8` for the dedicated configuration).
The ordinary local host omits that key. The `standardAcp` field advertises version 1 of the scoped ACP transport at
`/managed`. The control socket accepts `_nori/session/ensure`,
`_nori/session/list`, `_nori/session/status` and `_nori/host/stop` from any authenticated,
initialized connection. The transport also implements its existing
`_nori/control/status` and `_nori/control/take` role operations.

**List admitted sessions.** `nori host ls --json` observes the authenticated
`/sessions` endpoint. Its output contains only the host’s stable conversation
IDs and public credential reference labels. A binding named `sample` could produce this row.

```json
{"sessions":[{"session_id":"00000000-0000-4000-8000-000000000001","credential_refs":["sessions:token-connection:sample:v1"]}]}
```

The [managed inventory](../../../harness/src/managed_launch/listing.rs) reads
the current host’s launch registry, including resident exit and spawn-failure
records. Release removes the row. After a cold import, the row carries the
same conversation ID and the labels from the destination launch specification.
The recovery journal alone does not create an inventory entry.

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
      "adapter": {
        "path": "/opt/fixture/1.2.3/bin/fixture-acp",
        "version": "1.2.3",
        "args": ["--profile", "Review team"],
        "wrapper": {"path": "/srv/fixture/bindings/sample/home/runtime/launch", "version": "1.0", "args": ["--"]}
      },
      "cwd": "/srv/fixture/bindings/sample/work",
      "home": "/srv/fixture/bindings/sample/home",
      "runtimeDir": "/srv/fixture/bindings/sample/home/runtime",
      "additions": {"promptEcho": false, "statusEvents": false, "recording": false, "clientMcp": false}
    }
  }
}
```

- The caller supplies a stable conversation UUID, a nonblank command ID, a
  positive integer generation, and a binding ID containing ASCII letters,
  digits, `.`, `-`, or `_`. Empty, `.`, and `..` binding IDs are invalid.
  The conversation ID names the hosted conversation. ACP session IDs belong
  to the downstream adapter for regular sessions. A binding can carry several
  regular and side ACP IDs.
- The provisioner installs the versioned executable, protects it from
  replacement, resolves credential references and prepares binding-owned
  paths. The host runs the installed absolute `adapter.path` with
  `adapter.args` (default empty). For a wrapped launch, set
  `adapter.wrapper` to an object with an installed absolute `path`, a
  nonblank `version`, and optional `args`. A managed outer wrapper requires
  a prepared `runtimeDir`. The host resolves the wrapper path before launching
  the parent or a side and rejects a target outside that directory, including
  a symlink that points outside it. The wrapper receives its prefix arguments,
  the adapter path, then the adapter arguments. Each array element stays one
  argument. Version strings identify the provisioned installations. The
  provisioner selects and installs both executables before admission. An
  ordinary ACP client connects to an admitted session through its binding.
- The existing session-folder validator canonicalizes the absolute `cwd`
  and `home`. Both must exist and be distinct directories. In the shared
  layout both must be strictly beneath `bindings_root/bindingId`, and the
  binding directory must resolve to its designated location. In either
  layout existing `.codex`, `.nori`, and `.nori/cli` entries under the home
  must be directories. The host rejects symlinks at those entries, including
  dangling links. The provisioner must control directory
  replacement during launch. Same-UID filesystem access remains subject
  to OS permissions.
- The broker may set `spec.runtimeDir` to the absolute path of an existing
  directory it prepared for this binding. Its canonical path must lie strictly
  inside the shared binding root or the dedicated home. It must be disjoint
  from the workspace and cannot contain or equal the binding home. The
  adapter receives this path as `NORI_AGENT_RUNTIME_DIR`. Rebuilt sides
  revalidate the canonical path pinned at admission. A confined side using
  the private Codex home refuses a prepared runtime. The provisioner owns
  staging and cleanup outside checkpointed worktrees. It stages the files
  again at a recovery destination.
- The broker supplies public labels in `credentialRefs`. The inventory
  recognizes `sessions:codex-subscription-file:v1` and
  `sessions:token-connection:<id>:v1`. A connection ID matches
  `^[a-z0-9][a-z0-9-]{0,63}$`. Malformed token-connection labels fail
  admission. Older opaque references stay private. The host leaves
  credential resolution and provider configuration to the provisioner.
- Other optional fields inside `spec` are `model` (string or null), `env`
  (string-to-string map), `envRemove` (array of variable names) and
  `resumeArtifacts` (string array). Missing maps
  and arrays default to empty. Project, run, agent,
  model, credential and resume metadata stay with the immutable spec.
  The executable receives only its arguments, working directory and child
  environment. Unknown request or spec fields are rejected.
- The host clears its inherited environment, applies `spec.envRemove` before
  `spec.env`, and sets
  `HOME=home` and `NORI_HOME=home/.nori/cli`. The default `CODEX_HOME`
  is `home/.codex`.
  The host reserves those names, `USERPROFILE`, and `NORI_AGENT_RUNTIME_DIR`
  case-insensitively for both setting and removal. Admission also rejects a
  name present in both `env` and `envRemove`. Ordinary environment names are
  case-sensitive on Unix and case-insensitive on Windows. Adapter
  arguments and environment entries must be free of NUL characters.
  Supply variables such as `PATH` explicitly when the adapter needs them.
  Subprocesses inherit the binding environment. Host `.env` values stay
  in the host. Managed launches bypass host-configured hooks and MCP servers.
- `spec.workspaceReadOnly` selects OS-enforced read-only launch. It defaults
  to false. A confined rebuilt side forces this option when it reuses the
  parent's pinned executable, arguments, environment, cwd and home. A
  confined Codex side receives a reduced environment, described below. The host
  uses the spec's model when the side adapter reports a different current
  model. A read-only launch requires Linux with Bubblewrap and permission to
  create the required namespaces and mounts. The host uses `/usr/bin/bwrap`,
  or else `bwrap` on its own `PATH`. The launcher mounts the root
  read-only, supplies private device and process mounts, and sets
  `TMPDIR=/dev/shm` on a private writable tmpfs. Both the pinned workspace
  and persistent `HOME` are read-only. A confined rebuilt side also requires
  `pidfd_open`, `pidfd_send_signal`, and a host `/proc` view of Bubblewrap's
  reported child PID. Before the adapter can run, the host verifies that PID
  belongs to the supervisor's child and registers its pidfd. The side must
  then complete the sandboxed ACP handshake before the host returns its ID.
  [Read-only launch](../../../harness/src/managed_launch/side_sandbox.rs)
- `[host] side_confinement` is `"preferred"` by default. On Linux, the host
  probes Bubblewrap once at startup with the namespace and mount arguments of a
  read-only launch. A probe that fails or has not finished within five
  seconds marks the machine as unable to confine. When the probe succeeds, a rebuilt side
  is confined. On macOS, or when Linux cannot use Bubblewrap,
  the side starts as an ordinary adapter process with the parent's pinned
  executable, arguments, environment, cwd and home. Its workspace and home
  are writable at the OS level. The host still selects and confirms the
  restrictive adapter mode. The side takes one process slot, stops with its
  parent, and frees its slot when the process exits. With `"required"`, the
  host refuses a side the machine cannot confine. An explicit
  `workspaceReadOnly` launch always requires Bubblewrap. The side's
  `session/new` result [reports the outcome](../../../acp-host/docs.md#rebuilt-side-sessions-on-a-managed-binding).
- A confined rebuilt side pinned to codex-acp 2.7.0 gets a fresh
  `CODEX_HOME` and `CODEX_SQLITE_HOME` in its private tmpfs. Codex CLI
  0.166.0 opens its installation ID for writing during initialization.
  Each side creates its own installation ID and databases, which disappear
  at exit. Ordinary launches, including explicit `workspaceReadOnly`
  launches, unconfined sides and launches with side sessions disabled, use
  `home/.codex`. The Codex input checks below apply to the private home only.
  From the parent's `spec.env`, the side with a private home receives only
  `PATH`, `CODEX_PATH`, `TMPDIR`, `CODEX_SQLITE_HOME`, `LANG`, `LC_ALL`,
  `TERM` and `TZ`. The host then sets its own `TMPDIR`, `CODEX_HOME`,
  `CODEX_SQLITE_HOME`, `HOME` and `NORI_HOME`. Every other parent variable is
  left out, because it could name a credential or redirect the provider. No
  environment variable causes refusal. The parent launch keeps its full
  environment, and confined sides of other adapters receive all of it.
  An unconfined Codex side shares the parent's environment, provider home and
  credentials, including its MCP servers, and skips the side authentication
  checks.
- For a managed subscription, the host recognizes a single entry in the
  parent's admitted `spec.credentialRefs`, `sessions:codex-subscription-file:v1`.
  It opens the binding's `.codex` directory without following symlinks,
  checks that `auth.json` is a regular file, and mounts the open directory
  read-only. A symlink at the side's `$CODEX_HOME/auth.json` points to that
  filename in the mount. Opening the file after a broker rename reaches the
  replacement. The host never reads or copies token bytes. Codex CLI caches
  its credentials and reloads from the file on same-account 401 recovery or
  near expiry. A still-valid token can continue serving requests after
  replacement. [Codex input policy](../../../harness/src/managed_launch/codex_home.rs)
- The host mounts the exact validated binding config bytes and supported
  instructions read-only. When the config contains `mcp_servers`, the host
  mounts the validated configuration without that table and leaves the
  binding's file unchanged. Any other unsupported config key, enterprise
  settings whose effect it cannot establish, and ambiguous credential sources
  cause refusal. Unrecognised entries in the binding's `.codex` directory are
  the parent provider's own state, such as caches, plugins and lock files.
  The host does not mount them. It does not force a credential store or rewrite workspace or
  login restrictions. A binding with no credential references and no
  credential files can run the logged-out probe. [Sessions source contract](../../../acp-host/docs.md#codex-subscription-source)
- `[host] side_sessions` defaults to false. When enabled, the managed
  host advertises `rebuiltSideSessions: 1` and accepts marked `session/new`
  requests beside an open parent. The caller uses that parent's existing
  logical connection.
  [Sessions wire contract](../../../acp-host/docs.md#rebuilt-side-sessions-on-a-managed-binding)
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
  "specDigest":"7c9b31a083fc52411db88c9db7d3dcba09343cf810fafee2e009218b4aa5bf72",
  "state":"spawned"
}
```

The host computes `specDigest` as lowercase hex SHA-256 of its serialized
typed spec, including defaults. Treat it as opaque and store it with the
receipt. Matching concurrent and later ensures return the original receipt,
including at capacity and after exit. Reusing a binding with a different
generation, conversation ID or spec, or assigning
the conversation ID to another binding, returns a conflict. Without durable recovery, launch identity
is determined by the binding, generation, conversation UUID and spec digest,
independently of the command ID. A recovery-enabled generation admits one launch
command ID. Reuse that ID. Preserve the supplied paths on retry. The
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
says `spawned`. For a plain launch, receipts live in memory, so reconciliation after
a host restart requires external state. For a binding with durable recovery,
use `_nori/host/command` to reconcile admission after restart.

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
orphaned processes. For ordinary managed launches the host signals the
adapter's process group and waits for its direct child. Orphaned descendants can remain as zombies until their
adopting ancestor reaps them. See [process supervision](../../../acp-host/src/connection/child_lifecycle.rs).

#### Managed launch on stdio

A machine bridge that already pipes an ACP adapter's stdin and stdout can run
the managed host in the adapter's place. The bridge speaks standard ACP and
never opens the host's WebSockets. This mode requires `[host] managed = true`
and `[host.dedicated_roots]`. Startup fails without them.

```toml
[host]
managed = true
side_sessions = true

[host.dedicated_roots]
binding_id = "sample"
home = "/srv/fixture/home"
cwd = "/srv/fixture/home/org/workspace"
```

```sh
NORI_HOME=/srv/fixture/nori nori -c features.multi_session=true \
  host --stdio --launch /srv/fixture/launch.json
```

- The host runs in the foreground on an ephemeral loopback port. The
  [relay](stdio.rs) connects to that host's own `/sessions` and `/managed`
  sockets, so admission, side sessions, slot limits and process cleanup follow
  the [managed launch contract](#managed-launch-control-v1).
- The launch file holds one JSON object, the `spec` of `_nori/session/ensure`
  with the same required and optional fields. The host admits it once on the
  configured dedicated binding at generation 1 with a fresh conversation
  UUID, as a plain launch without durable recovery.
- stdin and stdout carry one JSON-RPC message per line. The relay adds and
  removes the `{bindingId, generation, message}` envelope of the
  [managed machine connection](../../../acp-host/docs.md#managed-machine-connection).
  The client sends `initialize` first and owns session setup. A marked side
  `session/new` or `session/fork` names the dedicated binding ID, generation 1
  and the parent ACP session ID, and travels on the same stream.
- Blank input lines are ignored. A line that is not JSON receives a JSON-RPC
  `-32700` error with a null `id` on stdout.
- `--inherit-env` adds the host process's environment to the launch's `env`.
  The relay excludes names in `envRemove` and keeps explicit values in `env`.
  The relay skips the host-assigned
  `HOME`, `USERPROFILE`, `CODEX_HOME`, `NORI_HOME`, and `NORI_AGENT_RUNTIME_DIR`. Entries
  that are not UTF-8 are skipped. Without the flag the adapter receives only
  `spec.env` and the host-assigned variables.
- stdout carries only ACP messages. Logs never go to stdout.

| Event | Outcome |
| --- | --- |
| stdin reaches EOF | The host stops, ends the adapter and its side sessions, removes its discovery state and exits 0. |
| The launch file is unreadable or the host refuses the launch | The host stops and exits non-zero with the reason on stderr. stdout stays empty. |
| The adapter's logical connection closes | The host stops and exits non-zero with the reason on stderr. |
| The host receives SIGTERM or SIGINT, with stdin open or closed | The host ends the adapter and its side sessions and exits non-zero. A bridge can stop it with a signal and keep its pipes. |

See the [stdio cases](../../tests/standard_acp/stdio.rs).

#### Managed recovery control

A machine supervisor can run the [managed host](#managed-launch-control-v1)
in the foreground with `NORI_HOME=/srv/fixture/nori nori host --foreground --port 0`.
After a restart, reread `host/state.json` and `host/token` from that home.
Initialize the privileged `/sessions` connection with `params: {}` to claim
control, or acquire control via `_nori/control/take`. Require both
`result.agentCapabilities._meta["nori.dev/managedHost"].standardAcp == 1`
and `result.agentCapabilities._meta["nori.dev/durableRecovery"].version == 1`.
The latter advertises the acknowledged journal, durable command admission,
checkpoint, import and release operations.

Add `recoveryVersion: 1` to the top-level **params** in the ensure example
above. Optionally include `transcriptId` there to associate the broker's
canonical transcript with the stable public conversation UUID. Later ensures
of that binding must continue to specify the recovery version. Recovery
persists unacknowledged transport data even with every host addition off,
including `spec.additions.recording`.
[Frame layout and ACK ordering](../../../acp-host/docs.md#managed-acp-delivery-and-recovery)

Every request below names `bindingId` and the current `generation`.

| Method on `/sessions` | Additional parameters | Result |
| --- | --- | --- |
| `_nori/host/command` | `commandId` | `status` is `inFlight`, `completed` (with the recorded `response`), or `interrupted`. The lookup works after restart. An unknown command returns an error. |
| `_nori/host/replay` | — | `records` contains unacknowledged `{source,sessionId,message}` records. |
| `_nori/host/ack` | `source` | Returns `{}` after durably advancing the cumulative cursor for that source stream. A future sequence or stale outer generation fails. |
| `_nori/host/checkpoint` | Optional nonempty string `attemptId` | Returns `{checkpoint,watermarks,fence}` at an idle boundary. Treat the checkpoint as opaque. Each watermark is a complete source identity. |
| `_nori/host/import` | `checkpoint`, `launchSpec` | Accepts the exported checkpoint and the destination machine's launch spec (same shape as ensure's `spec`). Requires a newer generation. Returns `{conversationId,sessionId,continuation}`. |
| `_nori/host/release` | `commandId` | Returns the final `{checkpoint,watermarks}` and stops this binding's process group. Other bindings, including independently bound descendants, continue running. |

Checkpoint returns an error while an ordinary ACP request is unfinished. A side
prompt waiting for permission can remain open during its parent’s checkpoint. Release
stops the binding's processes and waits for its rebuilt sides and parent relay
to finish before exporting the final checkpoint.

When `spec.additions.recording` is enabled, checkpoint flushes and syncs each
binding-owned recorder through its open file handle, then verifies that the
output watermarks still match. On checkpoint or release error, read
`error.data.savedThrough` when present. Its source cursors identify the last
durable output the host could verify. A known storage failure closes admission
with `-32020` before a fresh command enters the ledger. A failure during the
initial ledger commit can return `-32021`. Preserve the command ID and query
the recovered source journal as described in [managed ACP delivery and
recovery](../../../acp-host/docs.md#managed-acp-delivery-and-recovery).

After a private recorder sync error, retry `_nori/host/checkpoint` on the
same host, binding and generation once storage recovers. Fresh prompts still
receive `-32020` until every required recorder completes its durable flush.
After checkpoint success, resubmit the refused command with its original ID
and payload. The host admits it once and returns the recorded outcome on
later retries. If a recorder write had an uncertain outcome, its unsynced
bytes cannot be verified, or output failed to reach the journal, restore
from the last committed checkpoint under a newer generation. An ACK can free journal
space but cannot recover lost output. The broker captures the binding's
workspace and permitted provider state separately.

Give each checkpoint attempt a unique `attemptId`. If omitted, the host
assigns a UUID. For a valid attempt, success carries `result.fence` and
failure carries `error.data.fence`, both shaped
`{bindingId,generation,attemptId}` even with empty watermarks. The host
holds the binding's lifecycle lock through the bounded socket write of a
successful response. A release, import or later checkpoint on that host
therefore cannot overtake the handoff. A callback already buffered by the
adapter can arrive after a new generation or attempt became current.
Publish the checkpoint pointer in one conditional write that checks the
active binding, generation and pending attempt against the returned fence.
Discard a superseded completion. [Checkpoint control](recovery_control.rs)

Reuse the same release command ID when its reply is lost. A completed release
returns its saved export, including after restart. A busy release closes
admission for new work. Let its existing turn finish, or explicitly cancel
it, then retry. The host can finish a release after restart when its drained
export reached disk before the completion receipt. An interrupted release
without that final export requires the previous committed checkpoint.

An import with `continuation: "load"` means the source recorded a main ACP
session ID and its adapter advertised load support. The caller initializes
the destination adapter, checks its capabilities, then requests
`session/load` using the returned `sessionId`. The provider can reject the
load, for example if its backing state is unavailable. With
`continuation: "fresh"`, initialize and call `session/new`. This preserves
the public conversation identity with fresh provider context. In either
case, retrieve canonical conversation history through the broker. A
per-conversation host JSONL, when required for continuation, is a private
recovery artifact. The managed raw ACP export does not upload that JSONL.
Recovering model context never entails submitting an interrupted prompt.

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
