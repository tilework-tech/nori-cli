# Noridoc: nori-acp-host

Path: @/nori-rs/acp-host

### Overview

`nori-acp-host` is Nori's agent-agnostic, client-side ACP host. It owns the ACP
SDK connection, subprocess and wire lifecycle, agent registry, delegated client
requests, MCP forwarding, and ACP error categorization. It owns no TUI or
session-product state.

It also owns the optional remote ACP transport (`remote/`): a WebSocket server
implementing the WebSocket profile of the upstream ACP "Streamable HTTP &
WebSocket Transport" RFD, serving the hosted harness session outward as an ACP
Agent. The optional `/sessions` endpoint carries managed session commands and
full manager envelopes (see [the wire contract](../../docs/remote-control.md)).
The background-host listener has a separate authenticated `/sessions` route
with controller/watcher ownership. Managed launches also expose a scoped ACP
transport for machine adapters on `/managed`.

### How it fits into the larger codebase

```text
remote ACP client -- WebSocket /acp --> remote::RemoteAcpServer (this crate)
                                              |  calls
                                              v
                                        remote::HostedAgent (trait, this crate)
                                              ^
                                              |  implemented by
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
`remote/hosted_agent.rs` defines the downward-facing `HostedAgent` trait in
`nori-protocol` types, and `nori-harness` implements it over `HarnessHandle`.
[`remote/managed.rs`](src/remote/managed.rs) defines a parallel `ManagedAgent`
boundary for session-addressed JSON-RPC requests and atomic replay/live
subscriptions. The harness owns the session registry and the in-memory replay
used by attached terminals. The managed machine transport uses a separate
[durable journal](src/recovery/journal.rs) for acknowledged delivery to the
broker. The dependency direction stays `nori-harness -> nori-acp-host`.

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
- `registry.rs` resolves built-in and configured agents to spawnable process
  definitions. The built-in Codex definition uses the maintained
  `@agentclientprotocol/codex-acp` adapter and disables Codex-native goals in
  its subprocess configuration. It merges a valid ambient `CODEX_CONFIG`,
  preserving unrelated top-level and feature settings while forcing only
  `features.goals = false`, leaving Nori-owned goal state to the `nori-client`
  MCP server. `error_category.rs` preserves structured ACP errors before
  falling back to message classification.
- The legacy remote surface serves ACP through one or more exact-address
  axum listeners. `RemoteAcpServer::bind_many` binds every socket before any
  begins serving; a zero port after the first address reuses the first
  listener's allocated port. The listeners share one active-connection slot
  and generation, so one connection is live across the whole surface and a
  newer connection on any address replaces it (last connect wins).
- `bind_many_managed` adds `/sessions` to those same listeners. Both endpoints
  share the controller slot and shutdown token, so a new connection replaces
  either protocol's prior controller. [`managed_wire.rs`](src/remote/managed_wire.rs)
  requires initialization before commands, sends a subscription response before
  replay, and drains replay before live envelopes. It rejects binary frames;
  bounded live queues and timed socket writes prevent a slow client from
  blocking manager publication. The frontend opts into this route only with
  runtime `multi_session` enabled.
- `bind_host` serves the authenticated attach and control endpoint `/sessions` using
  [`host_wire.rs`](src/remote/host_wire.rs) and
  [`host_control.rs`](src/remote/host_control.rs). The first interactive
  initializer controls; other clients watch until an explicit takeover.
  Observation-only initialization never claims control. A mutex serializes
  mutation admission with handoff, then releases before backend work. An
  admitted operation may finish after takeover, while later old-owner mutations
  are refused. Slow backend operations cannot lock out other clients' control
  or administrative requests. Role notifications preserve every connection.
  Watcher mutations fail before reaching `ManagedAgent`; listing,
  subscribing, taking control, starting a new session (it changes no
  existing session, though it becomes the host's selected one), and
  authenticated host stop remain
  available.
  Successful authenticated upgrades include `x-nori-host-instance`, a UUID
  that identifies the running host independently of the bearer token. On a
  managed host it matches the journal’s producing incarnation.
- [`ManagedClient`](src/remote/client.rs) authenticates the upgrade and exposes
  the optional host-instance UUID and HTTP 401/403 authentication classification,
  alongside streaming send/receive for frontends that retain notifications.
  Its sequential request helper intentionally ignores intervening notifications and is used by
  short-lived administrative commands. Reconnect policy and cursor state belong
  to the frontend, not this socket wrapper.
- Each `/acp` listener sends a fresh `Acp-Connection-Id` on a 101 upgrade and
  `426 Upgrade Required` for plain HTTP. One JSON-RPC message occupies each
  UTF-8 text frame, adapted into the SDK's `Lines` transport
  (`remote/wire.rs`); binary frames are ignored, and the bounded outgoing
  writer ends with a best-effort close frame. `initialize` must be the first
  message on a socket, otherwise it closes with code 1002. The initialize
  handler sends its response before it starts forwarding subscribed session
  events, preserving initialize as the first server message even when the
  hosted session is already active.
- The legacy `/acp` Agent advertises `loadSession` plus session list/resume/close
  capabilities. Its initialize `_meta.nori.remoteControl` marker has version 1
  and includes `activeSessionId` when a running outward Nori conversation is
  available. The server obtains that ID from the hosted session catalog; an
  empty catalog or catalog error omits the ID without failing initialization.
  `session/load` replays recorded history as `session/update` notifications
  ahead of its response. `session/new` attaches to the already-running session
  instead of creating one: it answers with the hosted session's id and then
  replays recorded history *after* the response (the client only learns the id
  from the response). This keeps baseline-only ACP clients such as Zed working,
  since they open new threads with `session/new`. Without a hosted session,
  `session/new` fails with `-32002`.
- A remote `PromptRequest` passes its top-level `_meta` through the
  `HostedAgent` interface with its content and session id. The harness
  implementation retains that metadata through prompt admission and forwards
  it to the downstream ACP agent. This preserves the shared
  `nori_protocol::PROMPT_ECHO_ID_META_KEY` correlation marker across composed
  Nori hosts without making the transport interpret it.
- Prompt registration and a following `session/cancel` preserve WebSocket wire
  order through the same correlation lock. The prompt handler acquires the lock
  before spawning its hosted call and holds it until the harness request id is
  registered with the remote responder; cancel waits for that boundary before
  calling `HostedAgent::cancel`. This prevents an immediately following cancel
  from overtaking the prompt state it is meant to target.
- Responses are correlated at the boundary: `HostedAgent::prompt` returns the
  harness-issued request id, and the turn's final response arrives in stream
  order through the hosted event subscription, so a turn's `session/update`
  notifications always precede its response. Delegated permission requests
  round-trip through the remote controller; the subscription channel closing
  (queue overflow or replacement) tears the connection down. Wire behavior is
  exercised in `@/nori-rs/acp-host/tests/remote_ws.rs` against a fake
  `HostedAgent`.
- `registry.rs` also owns spawn-time **model injection**. Runtime model changes
  normally go over the live ACP `session/set_config_option` RPC, but adapters
  advertise only a subset of the models they can run and reject anything outside
  that catalog. `ModelInjection` ({ `Env { var }`, `Arg { flag }`, `CodexConfig`,
  `None` }) forces a chosen model through the agent's own out-of-band channel at
  spawn, so the model becomes the adapter's advertised `currentValue` (which ACP
  always treats as valid), bypassing the catalog. `AgentKind::model_injection()`
  maps built-in agents to their channel (Claude → `ANTHROPIC_MODEL` env, Gemini
  → `GEMINI_MODEL` env, Codex → merge `model` into the `CODEX_CONFIG` JSON).
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

### Managed machine connection

Set `[host] managed = true` in `$NORI_HOME/config.toml`. Set `NORI_HOME`
to the host's absolute configuration directory. With the default binding root,
start the daemon on an available loopback port with

```sh
mkdir -p "$NORI_HOME/hosted-sessions"
nori -c features.multi_session=true host --port 0
```

The binding root must already exist when the host starts. To use another
root, set `[host] bindings_root` to its absolute path. Read the WebSocket URL
from the `endpoint` field of `$NORI_HOME/host/state.json`. Use that URL
(`/sessions`) for managed control and use `/managed` on the same host and
port for ACP traffic. Both upgrades require `Authorization: Bearer <token>`
using the private `$NORI_HOME/host/token` file. The `x-nori-host-instance`
response header on both routes identifies the host incarnation that stamps new
journal records. With `[host] managed = false`,
`nori attach` uses its local `_nori/session/*` protocol on `/sessions`, and
`/managed` is absent. [Host lifecycle and discovery](../cli/src/host/docs.md)

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
policy before opening a side thread. The host checks OS and adapter support on each launch. On the parent's existing
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
parent object in `result._meta["nori.dev/sideSession"]`. Use that new ID on
`session/prompt`, `session/cancel` and `session/close`. Updates and permission
requests identify the side in `params.sessionId` and the parent in
`params._meta["nori.dev/sideSession"]`. Reply to a permission request using
the JSON-RPC ID the host sent on that logical connection. The host maps
that ID back to the issuing process, whose own request IDs can repeat
across siblings. Several side sessions may have outstanding
requests at once, including while a parent prompt awaits permission.

For a marked side pinned to codex-acp 2.7.0 and Codex CLI 0.166.0,
the parent launch determines the expected `_auth/status_update` after
`initialize`. A managed subscription requires `authStatus.kind = account`
before `session/new`. The adapter's account label, email and plan do not
identify a workspace or select credentials. A credential-free probe requires
`kind = none` and ends at the adapter's authentication refusal. A conflicting
status or unexpectedly successful credential-free open issues no side ID.
[Codex launch policy](../cli/src/host/docs.md#managed-launch-control-v1)

The host initializes the side adapter with empty client capabilities and opens
one session in the pinned workspace. It selects the advertised mode ID
`read-only`, or `plan` if available instead. For ACP `configOptions`, the
adapter must acknowledge `session/set_config_option` with the requested
option ID and current value. If it also advertises legacy `modes`, its
catalog must contain the same ID. The host then returns that ID in both
selectors. For Claude Agent ACP 0.102.0 both
`configOptions[category=mode].currentValue` and `modes.currentModeId`
report `plan`. Workspace protection comes from the read-only launch.
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

Each rebuilt side holds a process slot until its Bubblewrap supervisor and
PID namespace have exited. At capacity, `session/new` returns `-32017`
with the message `machine full`. The parent and its outstanding turn continue.

A successful side close means its adapter and descendants have released
their process-held resources. A successful parent close gives this guarantee
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
  `HostedAgent` and `ManagedAgent` interfaces. The harness and CLI supply the
  session behavior behind those interfaces.
- Disabling native goals is a policy of Nori's built-in Codex launch only.
  User-defined agent processes retain their explicit command and environment;
  goal ownership and routing remain a harness concern.
- Ambient `CODEX_CONFIG` must be a JSON object whose `features` value, when
  present, is also an object. Invalid JSON or incompatible shapes fail built-in
  Codex configuration explicitly instead of silently discarding user settings.
  Codex model injection merges only the `model` key into that same object,
  preserving the goals-disabled flag.
- The Codex agent subprocess is spawned with `CODEX_HOME` stripped
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
- The legacy `--remote` WebSocket surface is unauthenticated. `parse_bind_addr` treats a
  bare port as a loopback bind and refuses non-loopback addresses without the
  caller's explicit opt-in flag. Exact-address selection, wildcard rejection,
  dangerous-exposure confirmation, endpoint reporting, and runtime listener
  lifecycle belong to the TUI manager; this crate owns only the requested
  sockets and protocol transport. Before a same-port replacement, that manager
  snapshots `local_addrs()` and consumes the old server; if the new bind fails,
  it uses `bind_many` to restore the previous exact surface. A failed restore is
  distinct from the original bind failure and leaves no server running.
- The background-host listener requires a nonempty bearer token, even on
  loopback, and returns HTTP 401 before upgrade for missing or invalid tokens.
  It exposes no `/acp` fallback. The CLI owns token generation, private file
  validation, daemon lifetime, and loopback bind policy; this crate owns
  authentication and connection roles. A disconnected controller leaves the
  slot empty without promoting an existing watcher.
- Every accepted upgrade receives a child of the server-wide cancellation
  token. Consuming `RemoteAcpServer::shutdown` cancels that token before the
  active controller, then aborts and awaits every accept task; an upgrade whose
  callback starts during shutdown therefore cannot register a late connection.
  `Drop` applies the same cancellation boundary and aborts the tasks when an
  async wait is unavailable. Neither path owns or stops the `HostedAgent`, so
  the TUI can disable and later re-enable listeners around one continuing
  harness session.
- A disconnect from `/acp` or `/sessions` detaches the controller and leaves
  the harness session running. Legacy `/acp` has no envelope replay; `session/load` from the
  transcript remains its recovery path. Managed `/sessions` sends full
  `SessionEnvelope` values through `_nori/session/event` and delegates cursor
  validation and replay to the harness. It preserves pending permissions on
  disconnect so a reconnecting client can answer them.
- `session/cancel` has no response envelope, so the connection must not treat
  successful notification dispatch as proof that prompt ownership was already
  registered. Its ordering lock covers only the correlation boundary; the
  hosted harness remains responsible for deciding whether the controller owns
  the target turn.
- [`nori exec --acp`](../exec/docs.md) owns a separate stdio lifecycle. Its
  flag-on path reuses `ManagedAgent` for extension requests and manager
  envelopes; its standard ACP projection and flag-off bounded facade remain
  separate from the WebSocket adapter.
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
