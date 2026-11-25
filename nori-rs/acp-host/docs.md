# nori-acp-host

Path: `nori-rs/acp-host`

## Purpose

`nori-acp-host` is the client side of ACP toward agent servers, plus the wire
side of `nori host` toward its clients. It keeps the agent registry, builds the
argv and environment for each agent, spawns the agent server, and runs one
JSON-RPC connection over its stdio with the `agent-client-protocol` SDK. It
also owns the remote transport that `nori host` serves (`/acp` and
`/sessions`, stdio, and the managed machine socket) behind the `ManagedAgent`
trait, and the durable recovery journal for managed bindings. It knows nothing
about sessions, turns, or terminals. Terms follow
[`GLOSSARY.md`](../../GLOSSARY.md).

## Modules

| Module | Main items | Role |
|---|---|---|
| `registry.rs` | `AgentKind`, `RegisteredAgent`, `AcpAgentConfig`, `build_registry`, `get_agent_config`, `resolve_agent_slug` | Built-in agents (Claude Code, Codex, Antigravity as `gemini`, Grok Build, OpenCode, Cursor, Hermes, Pi), custom `[[agents]]`, providers |
| `adapter_launch.rs` | `COMMAND_ENV_PREFIX` | `NORI_ACP_COMMAND_<SLUG>` overrides, adapter version pins, the notice before an `npx`/`bunx` download |
| `launch.rs`, `launch/owned_runtime.rs` | | Literal argv for the stdio server and an optional outer launcher; lifetime of materialized runtime folders |
| `token_launch.rs`, `token_provider.rs` | | Apply the selected credential to the agent child only; write Pi's private provider profile |
| `connection/acp_connection.rs` | `AcpConnection` | Spawn, `initialize`, requests, notifications, graceful shutdown |
| `connection/child_lifecycle.rs` | `AgentProcess`, `ChildHandle`, `SHUTDOWN_GRACE` | Exit watcher, process-group sweep, stderr tail |
| `connection/mod.rs` | `ConnectionEvent`, `DelegatedRequest` | Raw events in source order, requests the client must answer |
| `connection/mcp.rs` | | `McpServerConfig` to ACP `McpServer`, env expansion |
| `connection/wire_log.rs` | | ACP wire recording when the ACP proxy config is enabled |
| `remote/` | `RemoteAcpServer`, `ManagedAgent`, `ManagedClient`, `ManagedReply`, `ManagedSubscription`, `HostConnection`, `SIDE_SESSION_META` | Host transport: WebSocket (`host_wire.rs`), machine socket (`standard_wire.rs`), controller admission (`host_control.rs`), client limit (`client_limit.rs`), client (`client.rs`) |
| `recovery/` | `Recovery`, `Checkpoint`, `SharedRecovery`, `journal` | Durable control and message correlation for an opted-in managed binding |
| `error_category.rs` | `categorize_acp_error`, `AcpErrorCategory` | Classify agent errors for display |
| `patch.rs` | `create_patch_with_context` | Unified diff with line context for an agent's edit |

```text
          nori host clients                       session runtime (nori-harness)
     (/acp WebSocket, stdio, machine socket)            |  implements ManagedAgent
                    |                                   |  drives AcpConnection
                    v                                   v
  remote::RemoteAcpServer --ManagedAgent trait--> [ HostService / ManagedRemoteHost ]
                                                        |
  registry + adapter_launch + launch + token_launch ---> AcpConnection::spawn
                                                        |  JSON-RPC over stdio
                                                        v
                                                 agent server process
```

The trait inverts the dependency. `nori-acp-host` defines `ManagedAgent` and
serves it. The session runtime implements it. So the transport never imports
the runtime.

## Dependency direction

```text
nori-harness --> nori-acp-host --> nori-config, nori-protocol,
                                   codex-rmcp-client, codex-utils-string,
                                   agent-client-protocol (SDK)
```

`nori-acp-host` must not depend on `nori-harness`, `nori-host-runtime`,
`nori-tui`, or any terminal crate. It is the only product crate that depends
on the ACP SDK (`nori-cli` uses it only as a dev-dependency for tests).

## Invariants

- Publish raw ACP as received. `ConnectionEvent::Acp` carries the exact
  agent traffic. Reduction happens in the session runtime.
- Shutdown closes the child's stdin first. The agent treats stdin EOF as its
  shutdown signal. After a grace period the host kills what is left, including
  the process group on Unix. The default is `SHUTDOWN_GRACE` (25 s). A
  stopping `nori host` gives its adapters 2 s. `nori-handroll cloud-acp` depends on
  this to release its broker session cleanly.
- Only the exit-watcher task owns the `Child`. Other code observes the exit
  through a watch channel and requests a kill through a `Notify`.
- Credentials go only to the agent child, after all reusable configuration is
  built. Never log a credential; Codex ACP logs its JSON config at startup.
- The spawned agent does not inherit `CODEX_HOME`
  (`acp_connection.rs` removes it), so the Codex agent uses its own home.
- Adapter pins and `NORI_ACP_COMMAND_*` precedence are user-facing facts owned
  by [`docs/agents.md`](../../docs/agents.md). Keep that table and
  `adapter_launch.rs` in step.
- The host wire (endpoints, auth, control roles, client limit, side sessions,
  error codes) is owned by the host contract
  (`nori-monorepo: docs/specs/nori-cli/host-contract.md`). Every `_nori/*`
  method and `nori.dev/*` key is registered in
  `nori-monorepo: docs/acp-extensions.md`. Change the contract first.
- A host has at most one controller. Watchers receive updates only. Mutation
  admission is serialized in `remote/host_control.rs`.
- MCP forwarding and OAuth behavior are owned by [`docs/mcp.md`](../../docs/mcp.md).

## Tests

| What | Where |
|---|---|
| Connection, spawn, shutdown, delegated requests | `src/connection/acp_connection_tests.rs` |
| Recovery journal | `src/recovery/tests.rs`, `src/recovery/journal/tests.rs` |
| Other unit tests | `#[cfg(test)]` modules in `src/` |
| Adapter launch, Cursor, Pi, provider model preferences, managed host | `tests/*.rs` |

Run:

```sh
cd nori-rs && cargo test -p nori-acp-host
```

Connection tests in `acp_connection_tests.rs` spawn `mock_acp_agent` and skip
when it is missing. Build it first with `cargo build -p mock-acp-agent`. End-to-end host
transport tests live in `nori-rs/cli/tests/host_lifecycle/`.

## Where `main` differs

- c08: MCP OAuth credentials fall back to `CODEX_HOME/.credentials.json`
  through `codex-rmcp-client`, not to a file under `NORI_HOME`.
