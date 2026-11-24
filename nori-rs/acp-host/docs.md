# Noridoc: nori-acp-host

Path: @/nori-rs/acp-host

### Overview

`nori-acp-host` is Nori's agent-agnostic, client-side ACP host. It owns the ACP
SDK connection, subprocess and wire lifecycle, agent registry, delegated client
requests, MCP forwarding, and ACP error categorization. It owns no TUI or
session-product state.

It also owns the remote ACP transport (`remote/`) that the background host
(`nori host`) serves: a WebSocket server implementing the WebSocket profile of
the upstream ACP "Streamable HTTP & WebSocket Transport" RFD. Its authenticated
`/acp` route (with `/sessions` as an alias) carries standard ACP, the
`_nori/session/*` commands, and full manager envelopes with controller/watcher
ownership (see [the wire contract](../../docs/remote-control.md)); the same
face is also served over stdio and in-memory pipes. Managed launches serve no
`/acp`; they keep `/sessions` and expose a scoped ACP transport for machine
adapters on `/managed`. An earlier, unrelated unauthenticated `/acp` endpoint,
which served one interactive TUI session, was removed; the path now belongs to
the background host.

### How it fits into the larger codebase

```text
remote client -- authenticated WebSocket /acp (alias /sessions), stdio, or pipe
      |
      v
remote::RemoteAcpServer (this crate)
      |  calls
      v
remote::ManagedAgent (trait, this crate)
      ^
      |  implemented by (ManagedRemoteHost over SessionManager)
nori-harness ---------------------------------+
      |
      v
nori-acp-host <----> ACP agent subprocess
      +---- nori-protocol (schema and public envelopes)
      +---- nori-config (agent and MCP configuration)
      +---- codex-rmcp-client (stored MCP credentials)
```

The host is the only client-side product crate that directly uses the
`agent-client-protocol` SDK. Schema values are still imported through
`nori_protocol::acp`, never through the SDK or schema crate directly.

The remote server inverts the runtime call direction (a remote client drives
the harness through this crate) without inverting the crate dependency:
[`remote/managed.rs`](src/remote/managed.rs) defines the downward-facing
`ManagedAgent` boundary for session-addressed JSON-RPC requests and atomic
replay/live subscriptions, and `nori-harness` implements it. The harness owns the session registry and the in-memory replay
used by attached terminals. The managed machine transport uses a separate
[durable journal](src/recovery/journal.rs) for acknowledged delivery to the
broker. One process owns a journal directory at a time through a
`flock` on `journal.lock`. Opening waits up to two seconds for that lock,
because a child spawned on another thread holds an inherited copy until it
execs or exits; a writer still holding it after that is refused. The dependency direction stays `nori-harness -> nori-acp-host`.

### Core Implementation

- `connection/` spawns an agent, performs ACP initialization, exposes typed ACP
  methods, and emits one source-ordered `ConnectionEvent` stream.
- ACP notifications, delegated requests, and method responses become raw
  `nori_protocol::AcpEvent` values with their `RequestId` intact.
- Initialization can route its raw response to the harness independently of
  connection construction, preserving schema errors even when construction
  fails without duplicating successful initialize events.
- Spawn transfers the child to its watcher and immediately installs a
  `ChildHandle` cancellation guard before awaiting ACP initialization. If the
  initialization future is aborted, dropping that guard kills the process
  group and lets the watcher reap it instead of orphaning a half-initialized
  agent.
- Each prompt call issues exactly one ACP `session/prompt` request and publishes
  its transport-assigned ID. Optional top-level prompt metadata is preserved on
  that request. Cancellation does not trigger a hidden resend or cancel-tail
  absorption loop. A successful empty `EndTurn` response is a terminal result.
- The typed SDK connection delegates permissions and handles filesystem
  read/write requests locally. The managed machine connection forwards agent
  requests to its ACP client.
- A private `SessionUpdate` copy feeds the harness reducer after the matching
  raw notification; it is implementation state, not another public protocol.
- The [registry](src/registry.rs) checks configured slugs before resolving built-in aliases. Claude Code and Codex launch through the preferred package runner (`NORI_MANAGED_BY_BUN`, then `NORI_MANAGED_BY_NPM`, then `bunx` on PATH, else `npx`) with upstream `@latest` resolution unless a version is pinned (see below). Grok Build always uses npm to run the upstream package’s native install step, because bun skips that postinstall hook. Antigravity runs `agy acp --disable-telemetry` and OpenCode runs `opencode acp` from the host’s `PATH`. Cursor uses `cursor-agent acp` when the packaged executable is on PATH, then falls back to the standalone command `agent acp`. Cursor reads its own saved login or the inherited `CURSOR_API_KEY` at startup. Hermes runs `hermes acp` and resolves its provider from its profile and process environment. Pi uses `npx` to install `pi-acp` and `@earendil-works/pi-coding-agent` together so the adapter finds `pi` on its PATH. Explicit runtime settings and custom distributions continue to control the launch. See the [provider table](../../README.md#providers) for commands and authentication.
- [Adapter launch control](src/adapter_launch.rs) runs after slug resolution
  and before anything is spawned, for built-in and `[[agents]]` slugs alike.
  Precedence per agent is `NORI_ACP_COMMAND_<SLUG>` (slug uppercased, `-` to
  `_`), then `[acp_adapters.<slug>] command`, then `[acp_adapters.<slug>]
  version`, then `@latest`. A command override is shell-split (shlex) and
  replaces only `command`/`args` and the install hint; a built-in keeps its
  identity, `CODEX_CONFIG`, model injection, and token-provider handling. A
  version replaces `@latest` on the adapter package only (for Pi, `pi-acp`;
  the Pi coding agent stays `@latest`) and adds `--prefer-offline` to npx so a
  pinned launch does not revalidate registry metadata each run. A version on
  an agent that Nori does not launch from an npm package (PATH-launched
  built-ins and every custom distribution) fails that agent's launch with an
  error naming `acp_adapters.<slug>.version`. Overrides live in a
  process-wide slot that the frontends set next to `initialize_registry` (the
  CLI's `exec`/ACP path, the TUI, and a non-managed `nori host`); managed
  launches carry their own pinned definitions and ignore it. Config keys may
  be aliases; they are matched after alias resolution, and the canonical key
  wins over an alias.
- `AcpAgentConfig::download_notice` is a "Downloading <specs>…" line set when
  the npx or bunx cache appears to lack the adapter package. It reproduces the
  npm 10 `_npx/<hash>` install-directory layout (under `$npm_config_cache` or
  `~/.npm`) and bun 1.x's `$TMPDIR/bunx-<uid>-<spec>` layout, so it is a
  heuristic: other installs or cache layouts can produce a spurious notice,
  and npx `@latest` updates or bunx's stale re-installs go unannounced. It is
  `None` for command overrides and PATH-launched agents. The
  [connection](src/connection/acp_connection.rs) logs it at info level just
  before spawning; it is not forwarded over ACP.
- Hermes v0.23.3 needs working loopback to reach its profile’s gateway during
  `session/new`. An auto-started gateway can outlive `nori host stop`. Its
  idle timeout defaults to 600 seconds (`gateway.unmanaged_idle_exit_seconds`).
  In a managed launch, an empty Hermes profile can still advertise Anthropic
  models when the child inherits `ANTHROPIC_API_KEY` and `ANTHROPIC_BASE_URL`.
  [Upstream idle-exit behavior](https://github.com/NousResearch/hermes-agent/blob/v0.23.3/gateway/run_idle_exit.py)
- The built-in Codex definition uses the maintained
  `@agentclientprotocol/codex-acp` adapter and disables Codex-native goals in
  its subprocess configuration. It merges a valid ambient `CODEX_CONFIG`,
  preserving unrelated top-level and feature settings while forcing only
  `features.goals = false`, leaving Nori-owned goal state to the `nori-client`
  MCP server. `error_category.rs` preserves structured ACP errors before
  falling back to message classification. A `session/load` refusal with
  code `-32011` means the agent is still finishing the original turn. The
  harness asks the user to retry that session after the turn ends, even when
  a local transcript is available for replay. Code `-32012` identifies a
  session the agent cannot resume.
- `RemoteAcpServer`'s entry points are the authenticated WebSocket listener
  (`bind_host`, or `bind_host_with_limit` with an explicit client limit;
  `bind_host` uses `nori_config::DEFAULT_MAX_CLIENTS`), the stdio server
  (`serve_host_stdio`, or `stdin_lines` plus `serve_host_stdio_lines` for a
  caller that must read the first lines itself), and `serve_host_pipe`.
  [`managed_wire.rs`](src/remote/managed_wire.rs) now holds only the shared
  WebSocket frame-send helper.
- `bind_host` serves the authenticated attach and control endpoint `/acp`
  and its alias `/sessions` (only `/sessions` plus `/managed` when the
  `ManagedAgent` reports `standard_acp_enabled`, i.e. a managed host) using
  [`host_wire.rs`](src/remote/host_wire.rs) and
  [`host_control.rs`](src/remote/host_control.rs). The first interactive
  initializer controls; other clients watch until an explicit takeover.
  Observation-only initialization never claims control. A mutex serializes
  mutation admission with handoff, then releases before backend work. An
  admitted operation may finish after takeover, while later old-owner mutations
  are refused. Slow backend operations cannot lock out other clients' control
  or administrative requests. Role notifications preserve every connection.
  Watcher mutations fail with `-32016` before reaching `ManagedAgent`;
  listing, subscribing, taking control, starting a new session (`_nori/session/new`
  or ACP `session/new`; it changes no existing session, though it becomes
  the host's selected one), ACP `session/list`, `session/load` and
  `session/resume`, and authenticated host stop remain available. The first
  controller's role watch reads `controller` from registration, and the
  initialize reply reports that role, so no `_nori/control/changed` is sent
  for the initial role.
  Each request is checked in this order: bearer token (401), WebSocket
  upgrade (426 with `Upgrade: websocket`), then the client limit (503 with a
  message naming the limit). The limit is a `ClientSlot` claimed from a
  shared counter in the upgrade handler and released by `Drop` when the
  connection ends or the upgrade never completes; it covers `/acp` and
  `/sessions` together and does not apply to a managed host or `/managed`.
  Binary frames are ignored, as the ACP remote transport requires.
  Successful authenticated upgrades include `x-nori-host-instance`, a UUID
  that identifies the running host independently of the bearer token. A
  `ManagedAgent` can choose it through `instance_id` (the local host does, so
  the same value appears in its `initialize` result and discovery file);
  otherwise a managed host uses the journal’s producing incarnation and other
  hosts a fresh UUID. Every upgrade also gets a fresh `Acp-Connection-Id`
  UUID header, per the RFD's WebSocket profile.
- `RemoteAcpServer::serve_host_stdio` serves one host client on the process's
  own stdin and stdout (the CLI's `nori host --stdio` without `--launch`, and
  `nori exec --acp`). `host_wire.rs` runs one frame loop (`serve_frames`)
  over any sink and stream of WebSocket `Message`s; stdio lines are adapted
  into text messages, so the stdio client gets exactly the methods and
  control policy of an authenticated `/acp` connection. A `Transport`
  value carries the only differences: on stdio there is no bearer token, the
  client always registers as controller (`observe` is ignored), and a
  managed subscription that falls behind is reported in band with
  `_nori/session/subscription_ended` `{"reason":"replay_required"}` rather
  than by closing the stream. stdin is read on a plain thread because a
  pending Tokio stdin read would keep the runtime from shutting down; blank
  lines are dropped and only text frames are written to stdout. The loop ends
  at stdin EOF or when the connection's close token is cancelled.
  `serve_host_stdio_lines` takes the line stream explicitly so the CLI can
  read a client's first `initialize`, try to relay it to a running host, and
  replay it here when it falls back.
- `RemoteAcpServer::serve_host_pipe` serves one in-process host client the
  same way over in-memory channels of JSON-RPC lines for one-shot
  [exec](../cli/src/host/docs.md#one-shot-exec). The
  [foreground launcher](../cli/src/host/docs.md#owned-tui-host) uses
  `serve_host_pipe_from` to assign foreground origin. Ordinary pipes,
  WebSocket and stdio clients have independent origin.
  Stdio and pipe share `serve_lines`, which owns the client-always-controls
  policy, blank-line skipping and the stop/lag handling; stdio only adds the
  stdin thread and maps non-UTF-8 lines to a parse error.
- `ManagedAgent` also has per-connection hooks that let the CLI serve standard
  ACP on `/acp` and `/sessions`. After a connection's first successful initialize,
  `connection_opened` hands over a `HostConnection`: the connection's bounded
  outbound queue (a client that cannot keep up is closed), its close token,
  and a watch of its role. `connection_closed` follows disconnect. Requests
  arrive through `request_from` with the client ID. Methods for which
  `runs_concurrently` is true are spawned, and their replies go through the
  outbound queue, so a slow agent start or a running prompt turn does not
  block the socket. Client notifications and responses to host-originated
  requests (such as `session/request_permission` answers) go to the async
  `message_from`, also spawned per message.
- [`ManagedClient`](src/remote/client.rs) authenticates the upgrade and exposes
  the optional host-instance UUID and HTTP 401/403 authentication classification,
  alongside streaming send/receive for frontends that retain notifications.
  An HTTP refusal is reported with its status and body text as context over
  the original tungstenite error, so `is_authentication_error` still
  downcasts it. `send_text`, `recv_text` and `close` move raw text frames
  without reinterpreting them, for the CLI's stdio relay.
  Its sequential request helper intentionally ignores intervening notifications and is used by
  short-lived administrative commands. Reconnect policy and cursor state belong
  to the frontend, not this socket wrapper.
- `registry.rs` also owns spawn-time **model injection**. Runtime model changes
  normally go over the live ACP `session/set_config_option` RPC, but adapters
  advertise only a subset of the models they can run and reject anything outside
  that catalog. `ModelInjection` ({ `Env { var }`, `Arg { flag }`, `CodexConfig`,
  `None` }) forces a chosen model through the agent's own out-of-band channel at
  spawn, so the model becomes the adapter's advertised `currentValue` (which ACP
  always treats as valid), bypassing the catalog. `AgentKind::model_injection()`
  maps Claude Code to `ANTHROPIC_MODEL`, Codex to the `model` key of `CODEX_CONFIG`, and Cursor to the `--model` argument.
  Antigravity, Grok Build, OpenCode, Hermes Agent, and Pi expose their supported models through ACP config options.
  Custom/BYO agents resolve their strategy from `[[agents]].model_override` in
  `nori-config`; the default is `None` (no channel → live RPC only).
  `AcpAgentConfig::inject_model` applies the resolved strategy to the spawn env
  or args and is a no-op for `None`, so callers (the harness) may invoke it
  unconditionally; `supports_model_injection()` reports whether a channel exists.
- `AgentKind::other_models()` returns a curated, per-agent
  `&'static [OtherModel]` (`{ id, label }`) of real models the adapter does *not*
  advertise but that generally run when forced through injection. It is the
  durable complement to the advertised catalog: the advertised set is
  agent/version/account-dependent and adapters do not hardcode it, so this static
  list plus render-time dedup against the live catalog is how the TUI populates
  the `/model` picker's "Other" section without a second source of truth.
  Selecting one routes through injection exactly like a `[default_models]` custom
  id. `OtherModel` is re-exported from `nori-harness` for that picker.

- A local distribution selects an ACP server executable and arguments. The
  resolved launch can prefix a wrapper executable and its arguments. The
  [local connection](src/connection/acp_connection.rs) and the
  [managed host](../harness/src/managed_launch.rs) apply their own environment
  and working-directory rules. The [child supervisor](src/connection/child_lifecycle.rs)
  owns the ACP pipes and process lifetime in both cases.
  [Launch configuration](../config.md#acp-server-launch)

### Managed machine connection

Set `[host] managed = true` in `$NORI_HOME/config.toml`. Set `NORI_HOME`
to the host's absolute configuration directory. With the default binding root,
start the daemon on an available loopback port with

```sh
mkdir -p "$NORI_HOME/hosted-sessions"
nori host --port 0
```

The binding root must already exist when the host starts. To use another
root, set `[host] bindings_root` to its absolute path. Read the WebSocket URL
from the `endpoint` field of `$NORI_HOME/host/state.json`. Use that URL
(`/sessions`) for managed control and use `/managed` on the same host and
port for ACP traffic. Both upgrades require `Authorization: Bearer <token>`
using the private `$NORI_HOME/host/token` file. The `x-nori-host-instance`
response header on both routes identifies the host incarnation that stamps new
journal records. With `[host] managed = false`,
`nori attach` uses its local `_nori/session/*` protocol on the published
`/acp` endpoint (`/sessions` is an alias), and `/managed` is absent. [Host lifecycle and discovery](../cli/src/host/docs.md)

1. On `/sessions`, send `initialize` and require
   `result.agentCapabilities._meta["nori.dev/managedHost"].standardAcp == 1`.
   Prepare distinct existing `cwd` and `home` directories under
   `<bindings_root>/<bindingId>/`. Call `_nori/session/ensure` with the
   command ID, public conversation UUID, binding ID, positive generation and
   immutable launch spec. That spec identifies the installed adapter by
   absolute path and pinned version and carries the four required
   `spec.additions` booleans below. A `spawned` receipt allows the logical
   client to initialize the admitted process. ACP describes sessions after
   process creation, so launch and generation fencing use host control.
   [Launch request and receipt](../cli/src/host/docs.md#managed-launch-control-v1)
2. Open one `/managed` WebSocket for the machine. For each admitted scope,
   send the logical ACP client's `initialize` as its first message. For
   example, a UTF-8 text frame for an admitted binding named `orchid` is

   ```json
   {"bindingId":"orchid","generation":1,"message":{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":1,"clientCapabilities":{}}}}
   ```

3. Deliver the inner reply to that client. Every ordinary request,
   notification, and reply in either direction uses this same envelope. The
   outer binding ID and integer generation identify an agent before ACP has a
   session ID. The adapter adds and removes the envelope. Each agent receives
   the client's initialization and returns its own capabilities, including
   `agentCapabilities.sessionCapabilities.fork` when supported.
   [Machine framing](src/remote/standard_wire.rs)

A bridge that pipes an adapter's stdin and stdout can use the same ACP stream
without the envelope. `nori host --stdio --launch <FILE>` admits one launch
on a dedicated binding and relays line-delimited ACP through these two
sockets. [Managed launch on stdio](../cli/src/host/docs.md#managed-launch-on-stdio)

For regular sessions, the adapter assigns the ACP IDs. Unmarked `session/new` and
`session/fork` return agent-issued IDs. An agent that advertises fork can
receive a `session/fork` for an inactive session from its `session/list`,
even before the client loads or resumes it. Use the admitted cwd for new,
fork, load and resume. Additional directories must resolve inside it.
An unadvertised fork returns `-32601`.
One prompt per session can be outstanding. A client can prompt one session
while another awaits permission. Agent `session/update` notifications, including
history before a load reply, arrive in agent order. Answer each agent
`session/request_permission` with the matching JSON-RPC ID on the same
logical connection. The host delegates that decision to the client.
`session/prompt` completes with the agent's turn result or error.
`session/cancel` is a notification. A repeated `session/close` reaches the
agent and returns its result or full error, including any `data`.
`session/set_mode` and `session/set_config_option` pass through
along with agent-provided modes, config options, models and metadata.
[Session routing](../harness/src/managed_acp.rs)

A machine client can submit a correction or queue input against that original prompt ID using the [host follow-up contract](../cli/src/host/docs.md#mid-turn-follow-ups). The request uses the same binding and generation envelope as the prompt.

The immutable launch controls the following additions independently for
regular sessions. Switching all four off preserves the agent's original updates, caller-supplied prompt
metadata and MCP servers. [Optional session services](../harness/src/backend/managed_additions.rs)

| Addition | Effect when enabled |
| --- | --- |
| `promptEcho` | Forward a correlation ID in `params._meta["nori.dev/promptEchoId"]` and emit one `user_message_chunk` per input block with the same ID in `update._meta`. This lets observers match input to its echo. |
| `statusEvents` | Emit `session_info_update` with `update._meta.nori.status` set to `working` on prompt and `idle` on its result or error. ACP v1 has no equivalent status event. |
| `recording` | Write per-session prompt and update records under `<spec.home>/.nori/cli/transcripts`, the binding’s private `NORI_HOME`. A successful session close flushes its recorder. |
| `clientMcp` | On session open, add an authenticated loopback `nori-client` descriptor when the agent supports HTTP MCP and the caller has not supplied that name. Its server lives with the session. |

When the host refuses to open a scope, it answers the initialize request with
`-32001` followed by `{"bindingId":"orchid","generation":1,"closed":true}`.
Once admitted, the logical connection owns the launch's single set of adapter
pipes. If the agent rejects `initialize`, deliver its error immediately. If
the agent keeps its pipes open, the client can retry on the same connection.
Agent EOF or a saturated scope queue emits the close frame. Fail outstanding
requests for that scope and continue serving its peers. For plain ACP bindings,
disconnecting the physical socket ends the logical connection and consumes the
pipes. Recovery-enabled bindings retain their relay across machine socket loss.

Missing or malformed scope produces a bare JSON-RPC error with `id: null`
(`-32600`, or `-32700` for malformed JSON). Within a valid scope, an inner
message with a valid `id` but no `method`, `result` or `error` receives a
scoped `-32600` carrying that ID.

#### Managed ACP delivery and recovery

Negotiate `nori.dev/durableRecovery` version 1 on the control connection
and select `recoveryVersion: 1` during ensure. Each main-session prompt then
carries a stable outer `commandId`. JSON-RPC `id` still correlates one call
and its response. The broker can change that ID when retrying the same
command. Scope, command identity and source cursors stay outside the ACP
message.

```json
{"bindingId":"fixture-binding","generation":1,"commandId":"turn-7","message":{"jsonrpc":"2.0","id":12,"method":"session/prompt","params":{"sessionId":"agent-session","prompt":[{"type":"text","text":"Show the fixture."}]}}}
```

- The host durably admits the command before dispatch. A retry with identical
  method and parameters can return its recorded response under the caller's
  current JSON-RPC ID. A conflicting command ID fails. If dispatch began and
  the host restarted before completion was recorded, reconciliation reports
  `interrupted`. A tool effect may already have happened.
- A recordable live message carries an outer `source` with `bindingId`,
  `generation`, `hostIncarnation` and `sequence`. The host commits the
  original message and source together before delivery. The broker appends
  both to its canonical transcript WAL, deduplicates by full source and ACKs
  only through its contiguous durable prefix for that stream. Replay after ACK
  loss has the original source, including after host restart. An ACK is cumulative within one producing
  generation and incarnation. After import its outer scope must name the
  destination generation, even when its `source` names an older producer.
- The first successful ordinary `session/new`, `session/load`, or
  `session/resume` selected for recovery supplies the main ACP session ID.
  A marked side open leaves that selection available. Side prompts and
  permission requests stay out of the journal and the exported checkpoint. History emitted during
  `session/load` stays out of the live journal, so loading an adapter cannot
  append that history to the broker transcript again.
- The checksummed append log is limited to 16 MiB per binding. Compaction
  can temporarily require space for a second bounded copy. A fresh prompt
  refused for a full journal or failed recorder barrier receives a scoped
  JSON-RPC error with its original request ID and code `-32020`. The refusal
  leaves the command ID available. Previously admitted IDs remain eligible
  for reconciliation while the ledger is readable.
- Code `-32021` means the host could not confirm persistence of the command's
  initial ledger entry. Keep the original ID and payload. Recover the source
  journal and query `_nori/host/command` for that binding, generation and ID.
  A later refusal on the failed store does not resolve the earlier attempt.
  Use the recorded outcome for a known ID. An interrupted command requires
  reconciliation before any new action. Only an explicit unknown-command
  result from the reopened source journal establishes that the ID was absent.
  An older checkpoint can predate this attempt.

**Reconnect a surviving relay.** After a machine socket disconnects, the
host continues to journal output from the original agent. On the replacement
`/managed` socket, send a standalone frame for each surviving scope. For
example, `{"bindingId":"fixture-binding","generation":1,"attachFrom":[]}`
starts each stream after its last durable ACK. To skip records already
committed by the broker, supply the full `source` of the last contiguous
commit for each such stream in `attachFrom`. Omitted streams start at their
ACK. Each supplied sequence must be between that stream's ACK and high
watermark, inclusive. Attaching does not advance an ACK.

The host takes the replay snapshot and switches live delivery under the
same lock. On success the socket receives retained
`{bindingId,generation,source,message}` frames in sequence order within each stream, then
`{bindingId,generation,attached:true}`, then live frames. Wait for the
scoped `attached` marker before submitting new work. A disconnect during
catch-up uses the same attach operation from the latest durable prefix.
A first ordinary ACP frame on a surviving relay also catches up from the
host's ACKs before dispatch, without the marker.

A malformed attach frame gets a scoped `-32600` when the scope is valid.
A duplicate, foreign, unknown or out-of-range cursor, an already attached
scope, or a scope without a live relay returns a scoped `-32001` with
`id: null`. Cursor rejection leaves the relay available to its owner.
After host restart, retrieve retained records and reconcile admitted commands
on `/sessions`. Restore a checkpoint into a newer generation and initialize
its logical ACP connection to continue.
[Cursor validation](src/recovery/journal.rs) · [Relay handoff](src/remote/standard_wire.rs)

See the [machine invocation and recovery control operations](../cli/src/host/docs.md#managed-recovery-control).

### Rebuilt side sessions on a managed binding

A machine provisioned with `[host] side_sessions = true` advertises
`rebuiltSideSessions: 1` alongside `standardAcp: 1` in its managed-host
capabilities. The default is off. Sessions checks this field and its rollout
policy before opening a side thread. The host checks adapter support on each launch and probes OS confinement support once, when the host starts. On the parent's existing
`/managed` connection, send the following frame after the regular ACP session
has opened. All identifiers in this example are synthetic.

```json
{"bindingId":"sample","generation":1,"message":{"jsonrpc":"2.0","id":12,"method":"session/new","params":{"cwd":"/srv/fixture/bindings/sample/work","mcpServers":[],"_meta":{"nori.dev/sideSession":{"bindingId":"sample","generation":1,"sessionId":"parent-0"}}}}}
```

The presence of `nori.dev/sideSession` requests a side session. Its
`sessionId` names the open parent ACP session. Both scope fields must match
the envelope. Use the parent binding’s canonical `cwd`. Omit `mcpServers` and
`additionalDirectories` or supply empty lists. A malformed or stale
parent reference receives `-32602`. The host refuses a marked open when
side sessions are disabled.

A successful response contains a new opaque `result.sessionId` and echoes the
parent object in `result._meta["nori.dev/sideSession"]`. It also carries
`result._meta["nori.dev/sideConfinement"]`, either `"bubblewrap"` or `"none"`.
Sessions reads this field to learn whether the OS read-only launch holds
the side. A fork-form side does not carry it. Use that new ID on
`session/prompt`, `session/cancel` and `session/close`. Updates and permission
requests identify the side in `params.sessionId` and the parent in
`params._meta["nori.dev/sideSession"]`. Reply to a permission request using
the JSON-RPC ID the host sent on that logical connection. The host maps
that ID back to the issuing process, whose own request IDs can repeat
across siblings. Several side sessions may have outstanding
requests at once, including while a parent prompt awaits permission.

For a confined rebuilt side pinned to codex-acp 2.7.0 and Codex CLI 0.166.0,
the parent launch determines the expected `_auth/status_update` after
`initialize`. A managed subscription requires `authStatus.kind = account`
before `session/new`. The adapter's account label, email and plan do not
identify a workspace or select credentials. A credential-free probe requires
`kind = none` and ends at the adapter's authentication refusal. A conflicting
status or unexpectedly successful credential-free open issues no side ID.
A confined Codex side starts with a fixed subset of the parent's environment.
The parent's other variables, such as integration tokens, do not reach the
side and do not cause refusal.
[Codex launch policy](../cli/src/host/docs.md#managed-launch-control-v1)

The host initializes the side adapter with empty client capabilities and opens
one session in the pinned workspace. It selects the advertised mode ID
`read-only`, or `plan` if available instead. For ACP `configOptions`, the
adapter must acknowledge `session/set_config_option` with the requested
option ID and current value. If it also advertises legacy `modes`, its
catalog must contain the same ID. The host then returns that ID in both
selectors. For Claude Agent ACP 0.102.0 both
`configOptions[category=mode].currentValue` and `modes.currentModeId`
report `plan`. When the result reports `"bubblewrap"`, workspace protection
comes from the read-only launch. When it reports `"none"`, the workspace and
home are writable at the OS level. The remaining guards are the confirmed
restrictive mode, the caller's permission policy and the caller's
instruction. With `[host] side_confinement = "required"`, the host refuses
a side the machine cannot confine.
Legacy-only adapters use `modes` and `session/set_mode`. A conflicting
mode update during selection or a failed setup ends the process and returns
an error before the client receives a side ID. If the adapter later reports
a different mode or drops its confirmed selector, the host closes the side,
fails pending requests with `-32002`, and releases its process slot.
[Side adapter setup](../harness/src/managed_acp/side_worker.rs)

The host uses the ACP `configOptions` model selector when advertised and
supports legacy `models`. A caller may reassert the selected permission mode
through the same selector. Changing the mode or setting other options
returns `-32602`.
Only the host supplies the side marker on frames delivered to Sessions.
The host forwards side updates, permission requests and correlated replies.
The host disables recording, prompt echo, status additions and client MCP for
this process. Side IDs have no delegation or restore path and are omitted from
session lists. [Side routing](../harness/src/managed_acp/side_sessions.rs)

A confined rebuilt side holds a process slot until its Bubblewrap supervisor
and PID namespace have exited. An unconfined side holds its slot until the
adapter process exits. At capacity, `session/new` returns `-32017`
with the message `machine full`. The parent and its outstanding turn continue.

A successful close of a confined side means its adapter and descendants have
released their process-held resources. For an unconfined side it confirms
the exit of the adapter process only. A successful parent close gives this guarantee
for every side belonging to that ACP session. Sides of other parents keep
running. If the parent adapter declines the close, its sides stay active
and the caller can retry. If the host cannot confirm a side's exit, its slot
stays reserved and the affected close returns an error. Loss of the parent
process, the logical ACP connection or the host also triggers side teardown.
[Admission and confinement](../cli/src/host/docs.md#managed-launch-control-v1)

Sessions supplies conversation context in the first prompt, correlates side
events separately from the parent transcript, answers permission requests
and closes idle side threads. The host keeps side identity in memory.

#### Codex subscription source

Sessions can opt an admitted parent into file-backed subscription reuse by
setting its entire `spec.credentialRefs` array to
`["sessions:codex-subscription-file:v1"]`. This unsigned marker derives
its authority from the privileged, immutable parent launch spec. Side ACP
requests cannot declare a credential source. A path, file contents or
generic credential access reference alone cannot establish the selection.

Before emitting the marker, the Sessions adapter must establish all of the
following for the binding generation and the lifetime of every active side.

- The parent actually uses the official file store at
  `home/.codex/auth.json`. Its effective provider, store, endpoints and
  login restrictions apply equally to the side's private provider home.
  The side keeps only the parent's `PATH`, `CODEX_PATH`, `TMPDIR`,
  `CODEX_SQLITE_HOME`, `LANG`, `LC_ALL`, `TERM` and `TZ` variables, so a
  provider or credential setting supplied through any other variable does
  not reach it.
  The side starts without the binding's `mcp_servers`. An unconfined side
  has no private provider home and uses `home/.codex` directly.
  Keep these settings fixed. A change of connection, account, workspace or
  policy requires closing the sides and admitting a new binding generation.
  Sessions must also prevent a parent ACP login or gateway switch from
  changing that selection in place. A subscription side rejects the
  adapter's local `/logout` command and closes if it reports loss of account auth.
- The publisher controls the stable binding directory and the auth pathname.
  Only the selected subscription principal and workspace can supply a
  replacement. Publish each complete, mode-0600 regular file by atomic rename
  in that directory. Keep the broker refresh token and service credentials
  outside the file. The worker document carries the broker-redacted refresh
  field. The side runs no vault or token-distribution client.
- For a native subscription, the bridge pulls the worker-bound Codex account
  from the vault and atomically renames a sibling into its configured
  destination. Its default destination is the worker home. For a scoped
  connection, the token-provider runtime resolves the machine and connection
  to its pinned account and selects a separate pairing home. Its file
  distribution uses `vm.writeFile`, whose atomicity depends on the backend.
  Sessions must publish the selected source into the binding file and keep it
  current before declaring this contract. Omit the marker when selection,
  refresh delivery, or effective file-store policy is ambiguous.
  [Native bridge](https://github.com/tilework-tech/nori-sessions/blob/477d4da9c52e3bb2d057945098be3eb35b3c6ffa/broker/server/bootstrap/scripts/sprite-acp-bridge/server.ts) ·
  [Vault selection](https://github.com/tilework-tech/nori-sessions/blob/477d4da9c52e3bb2d057945098be3eb35b3c6ffa/broker/server/src/inbound-events/http/endpoints/vault.ts) ·
  [Scoped runtime](https://github.com/tilework-tech/nori-sessions/blob/477d4da9c52e3bb2d057945098be3eb35b3c6ffa/broker/server/src/core-services/credential-management/token-provider-runtime.ts)

### Things to Know

- The dependency direction is `nori-harness -> nori-acp-host`, never the
  reverse. The remote module preserves it: its handlers call the
  `ManagedAgent` interface. The harness and CLI supply the
  session behavior behind those interfaces.
- Disabling native goals is a policy of Nori's built-in Codex launch only.
  User-defined agent processes retain their explicit command and environment;
  goal ownership and routing remain a harness concern.
- For a native Codex launch, ambient `CODEX_CONFIG` must be a JSON object whose `features` value, when
  present, is also an object. Invalid JSON or incompatible shapes fail built-in
  Codex configuration explicitly instead of silently discarding user settings.
  Codex model injection merges only the `model` key into that same object,
  preserving the goals-disabled flag.
- A named Codex token provider uses a process-local Responses provider definition.
  The adapter's startup configuration contains an environment reference. The
  child receives the selected key when it starts, and Nori disables its native
  API-key authentication retry for that binding.
  [Launch adapter](src/token_launch.rs) · [Codex ACP startup](https://github.com/agentclientprotocol/codex-acp/blob/e3a679f193458f8d4825803a4744063e7d4a5e80/src/index.ts)
- The native Codex agent subprocess is spawned with `CODEX_HOME` stripped
  (`.env_remove("CODEX_HOME")` in `connection/acp_connection.rs`), so it resolves
  its home to `$HOME/.codex` and reads its credentials from
  `~/.codex/auth.json`, ignoring any `CODEX_HOME` override. The invariant this
  enforces: whatever writes Codex credentials must target the same home the
  subprocess reads. Nori's in-app OAuth `/login`
  (`@/nori-rs/tui/src/chatwidget/login.rs`) therefore writes to `~/.codex` rather
  than `nori_home`; writing anywhere else means a successful login never
  authenticates the agent. Nori's own in-process `AuthManager` (which reads
  `nori_home`) is not consumed by the ACP path, so the login flow's
  `auth_manager.reload()` has no effect on the Codex agent.
- Pi has no spawn-time model injection. A bound Pi launch names its starting
  model in the generated `settings.json`. The matching entry in `models.json`
  carries the selected wire model, protocol, endpoint and key reference.
  Pi reads the key from the environment when the reference begins with `$`.
  Nori writes key references into a fresh private directory for each launch
  (directory mode `0700`, file mode `0600` on Unix). Cloned launch configurations
  and process supervisors retain ownership through child reaping. Local preparation
  collects abandoned directories with unlocked ownership records. Pi's `sessionDir`
  points at the connection's persistent session history across restarts.
- Model injection does not validate the model id: an invalid model is accepted at
  spawn and only fails at the first prompt. For env-based channels, nori's
  injected value takes precedence over the agent's own configured model (e.g.
  `~/.claude/settings.json`) for nori-spawned sessions — nori's `[default_models]`
  is authoritative here by design.
- `set_config_option` mirrors only a *successful* response onto the public event
  boundary. Its error is returned to the caller (the `/model` flow renders a
  friendly message and may persist-and-restart), so mirroring the raw JSON-RPC
  error too would surface a confusing duplicate "Internal error" cell. Raw wire
  errors are still captured by ACP wire recording, which taps stdio separately.
- The typed SDK connection does not advertise terminal and extension request
  families. Adding them requires an explicit host-policy decision, not a generic
  protocol mirror.
- The background-host listener is the only listener this crate serves. It requires a nonempty bearer token, even on
  loopback, and returns HTTP 401 before upgrade for missing or invalid tokens.
  There is no unauthenticated route. The CLI owns token generation, private file
  validation, daemon lifetime, and loopback bind policy; this crate owns
  authentication and connection roles. A disconnected controller leaves the
  slot empty without promoting an existing watcher.
- Every accepted upgrade receives a child of the server-wide cancellation
  token. Consuming `RemoteAcpServer::shutdown` cancels that token, then aborts
  and awaits every accept task; an upgrade whose callback starts during
  shutdown therefore cannot register a late connection. `Drop` applies the
  same cancellation boundary and aborts the tasks when an async wait is
  unavailable. Neither path owns or stops the `ManagedAgent` or its sessions.
- A disconnect from `/acp` or `/sessions` detaches the client and leaves the
  host's sessions running (the RFD's v1 durability rule); a client reconnects
  with a new `initialize` and `session/load` or `session/resume`. The
  `_nori/session/*` face sends full
  `SessionEnvelope` values through `_nori/session/event` and delegates cursor
  validation and replay to the harness. It preserves pending permissions on
  disconnect so a reconnecting client can answer them.
- `nori exec --acp` has no separate stdio implementation: the CLI either
  relays it to the running host over `/acp` or serves a private host through
  `serve_host_stdio_lines`
  ([Local host on stdio](../cli/src/host/docs.md#local-host-on-stdio)).
- The `agent-client-protocol` SDK is built with the `unstable` feature so the
  host can call `session/fork` (branch-at-head). See `fork_session` in
  `@/nori-rs/acp-host/src/connection/docs.md`.
- Shutdown closes stdin and optionally waits for a caller-selected grace period.
  The child owner then kills the process group before reaping its leader, so
  MCP servers and other descendants cannot survive a cooperative agent exit.
  A nonzero grace is reserved for bounded cooperative cleanup, including the
  short pre-session grace for an abandoned prepared connection and the longer
  cloud-detach path.
- The removed ACP-to-Codex translator must not be recreated. Display-friendly
  projection belongs privately in a consumer such as the TUI.

Created and maintained by Nori.

### Forking a read-only side session

Check `result.agentCapabilities._meta["nori.dev/managedHost"].forkSideSessions`
on the host control connection. A value of `1` means that the host accepts
marked fork requests. The admitted adapter must also support them. With
`[host] side_sessions` off, the host refuses marked forks. An ordinary
unmarked `session/fork` retains the adapter's standard ACP behavior.

On the parent's existing `/managed` connection, send `session/fork` with the
parent ACP ID in both `params.sessionId` and the existing side-session marker.
The example uses the admitted workspace as `cwd`.

```json
{"bindingId":"sample","generation":1,"message":{"jsonrpc":"2.0","id":13,"method":"session/fork","params":{"sessionId":"parent-0","cwd":"/srv/fixture/bindings/sample/work","mcpServers":[],"_meta":{"nori.dev/sideSession":{"bindingId":"sample","generation":1,"sessionId":"parent-0"}}}}}
```

**Adapter handshake.** In the adapter's `initialize` result, advertise
`agentCapabilities.sessionCapabilities.fork` and `.close` as objects and set
`agentCapabilities._meta["nori.dev/readOnlyFork"]` to
`{"version":1,"modeId":"inspect"}`, substituting the adapter's actual mode ID.
The host forwards the validated `nori.dev/sideSession` marker on the adapter's
standard `session/fork` request. An adapter advertising this guarantee keeps
marked forks ephemeral, omits them from durable history and restoration, and
waits for mode setup before performing any work in the fork. It can fork a running turn,
service prompts on each fork independently, and enforce the named mode on
one fork without changing the parent or its siblings. The mode must prevent
workspace writes and command execution, including actions the adapter performs
without asking permission. An adapter can omit this capability when it cannot
make those guarantees.

The adapter returns a fresh session ID and includes the advertised mode ID in
the fork's available modes. The host confirms the fork's read-only mode
before returning its public ID to the caller. A mode change uses
`session/set_mode` on the fork's adapter ID. An ID that aliases the parent or
a live sibling fails admission. When the adapter cannot supply a safe fork,
the host returns `error.data.fallback = "rebuilt"`. Sessions can then issue
the marked `session/new` described above. Codex launches use this rebuilt path.

Each successful fork shares the parent's adapter process and consumes zero
additional process slots. Use the returned public ID for prompts, cancellation
and close. Side turns stay out of the parent's transcript and session lists.
Updates and permission requests carry the same parent metadata as a rebuilt
side. The broker answers each permission using its side-session policy and the
JSON-RPC ID the host sent.

Keep receiving streamed updates and answering permissions while a fork's
`session/close` is pending. If the adapter declines, the same public ID and
outstanding permission requests remain usable. After the adapter confirms
closure, the host drops late callbacks for that fork. Its parent and siblings
can continue their turns.

If the adapter declines to close the parent, its forks remain open. Once it
confirms the parent is closed, the host refuses new sides for that parent and
returns success only after its native forks close and its rebuilt processes exit.
If a fork declines close or confirmation times out, the parent close returns
`error.data` containing `{"parentClosed":true,"retry":true}`. Keep answering
outstanding child permissions, then repeat `session/close` with the parent ID.
The host retries remaining cleanup and reuses confirmed close results. Other
regular sessions on the same adapter can continue.
[Loopback tests](../cli/tests/standard_acp/fork_side.rs) · [Synthetic adapter](../cli/tests/standard_acp/fork_fixture.py)
