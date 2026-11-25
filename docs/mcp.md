# MCP

This contract states how MCP servers reach the agent in a Nori CLI session:
the servers you configure, the servers a `nori host` client sends, the
`nori-client` server Nori runs itself, and MCP OAuth. Terms follow the
[glossary](../GLOSSARY.md).

Nori does not connect to MCP servers itself. It passes them to the agent in
the ACP `mcpServers` list of `session/new`, `session/load`, and
`session/resume`. The agent connects to them.

## Sources of the server list

For each session, Nori builds `mcpServers` from these sources, in this
order:

1. `[mcp_servers.<name>]` in `config.toml`, except entries with
   `enabled = false`. Sorted by name.
2. On `nori host`: the `mcpServers` the client sent on `session/new`. A client
   entry replaces a configured entry with the same name. The host owns this
   wire rule: `nori-monorepo: docs/specs/nori-cli/host-contract.md`
   ("The ACP face").
3. `nori-client`, when the agent supports HTTP MCP (see
   [The `nori-client` server](#the-nori-client-server)).

Nori builds the list again for every session it starts: new, loaded,
resumed, or swapped after compaction or fork. Read-only side sessions on a
managed host get an empty list.

The TUI's `/mcp` edits `[mcp_servers]`. Changes reach the next session, not
the running one.

## `[mcp_servers.<name>]`

The name `nori-client` is reserved. A config that uses it fails to load.

### Stdio servers

Set `command`.

| Key | Type | Default | Sent to the agent as |
|---|---|---|---|
| `command` | string | required | `command` |
| `args` | list of strings | `[]` | `args` |
| `env` | table of strings | none | `env` entries |
| `env_vars` | list of variable names | `[]` | `env` entries, with values read from Nori's environment at session start. A missing variable is skipped with a warning. |

### HTTP servers

Set `url`. The transport is streamable HTTP.

| Key | Type | Default | Sent to the agent as |
|---|---|---|---|
| `url` | string | required | `url` |
| `http_headers` | table of strings | none | `headers` |
| `env_http_headers` | table: header name to variable name | none | `headers`, with values read from Nori's environment at session start. A missing variable is skipped with a warning. |
| `bearer_token_env_var` | variable name | none | `Authorization: Bearer <value>` header. A missing variable is skipped with a warning. |
| `client_id` | string | none | Not sent. Used by MCP OAuth sign-in for servers without dynamic client registration. |
| `client_secret_env_var` | variable name | none | Not sent. Holds the OAuth client secret for MCP OAuth sign-in. |

When `bearer_token_env_var` gives no token, Nori adds the stored MCP OAuth
token for the server, if one exists, as `Authorization: Bearer <token>`. An
expired token is still sent, with a warning to sign in again through `/mcp`.

### Keys for every server

| Key | Type | Default | Effect |
|---|---|---|---|
| `enabled` | bool | `true` | `false` leaves the server out of `mcpServers` |
| `startup_timeout_sec` | number | none | No effect. Nori warns at load. |
| `startup_timeout_ms` | integer | none | No effect (alias of the key above). Nori warns at load. |
| `tool_timeout_sec` | number | none | No effect. Nori warns at load. |
| `enabled_tools` | list of strings | none | No effect. Nori warns at load. |
| `disabled_tools` | list of strings | none | No effect. Nori warns at load. |

ACP `McpServer` has no fields for timeouts or tool filters, so the agent
never sees these keys.

### Errors at load

- An entry with neither `command` nor `url` fails ("invalid transport").
- `bearer_token` (an inline token) fails for both transports. Use
  `bearer_token_env_var`.
- A stdio entry with `url`, `bearer_token_env_var`, `http_headers`,
  `env_http_headers`, `client_id`, or `client_secret_env_var` fails.
- An HTTP entry with `args`, `env`, or `env_vars` fails.

## Transports

| Transport | Configured servers | `nori host` client servers | ACP type sent |
|---|---|---|---|
| stdio | yes | yes | `McpServerStdio` |
| streamable HTTP | yes | yes | `McpServerHttp` |
| SSE | no | no (`-32602`) | none |

Nori sends HTTP servers whether or not the agent advertises
`mcpCapabilities.http`. Whether the agent connects to them is up to the
agent.

## MCP OAuth

`/mcp` in the TUI manages MCP servers. In its list, `l` starts OAuth sign-in
for the selected HTTP server.

- Sign-in opens the browser to the server's authorization URL, with a
  callback server on `http://127.0.0.1:<random port>/callback`.
- When `client_id` is set, Nori uses it and skips dynamic client
  registration. The secret comes from `client_secret_env_var`; a missing
  variable is skipped with a warning.
- The list shows each server's auth status:

  | Status | When |
  |---|---|
  | `Bearer token` | `bearer_token_env_var` is set |
  | `OAuth` | A stored token exists |
  | `Not logged in` | The server advertises OAuth and no token is stored |
  | `Unsupported` | A stdio server, or the server does not advertise OAuth |

### Where tokens live

Nori stores MCP OAuth tokens per server name and URL. The headers do not
count.

| Store | Location |
|---|---|
| OS keyring (first choice) | Service `Nori TUI MCP Credentials`, account `<server name>\|<first 16 hex of SHA-256 of {"type":"http","url":<url>,"headers":{}}>` |
| Fallback file (no keyring) | `$NORI_HOME/.credentials.json`, mode 0600 on Unix. Nori deletes the file when it holds no tokens. |

When the keyring fails, Nori writes the fallback file instead. When it reads,
it tries the keyring, then the file when the keyring has no entry or fails.

Nori does not refresh MCP OAuth tokens. It passes the stored access token to
the agent as a header.

## The `nori-client` server

`nori-client` is an MCP server that Nori runs for each session. It gives the
agent Nori-owned state and context that ACP does not carry.

### When the agent gets it

- **TUI, `nori exec`, `nori host`:** when the agent's `initialize` answer has
  `agentCapabilities.mcpCapabilities.http: true`.
- **Managed host:** when the launch spec enables the `clientMcp` addition,
  the agent supports HTTP MCP, and the client did not already send a server
  named `nori-client`.

The first prompt of a session carries a context block once. Nori picks one
of two versions: one for agents with HTTP MCP, which can read `nori-client`,
and one for agents without it.

### Transport

- Streamable HTTP at `http://127.0.0.1:<random port>/mcp`.
- Every request needs `Authorization: Bearer <token>`. The token is a random
  UUID per server. Other requests get HTTP 401.
- MCP protocol version `2024-11-05`.
- Server info: name `nori-client`, version the CLI version.
- A session swap after compaction or fork keeps the same server, URL, and
  token. Nori starts a new server only when the session had none.

### Tools

| Tool | Arguments | Effect |
|---|---|---|
| `get_goal` | none | Returns `{"goal": ...}`: the session's goal with status, token use, and elapsed time, or `null` |
| `create_goal` | `objective` | Starts an active goal. Fails when a goal exists. |
| `update_goal` | `status`: `complete` or `blocked` | Marks the goal achieved or blocked. Other transitions belong to the user and Nori. |
| `delegate` | `agent`, `task` | Starts a child session. Listed only in sessions the session manager runs. Rules are in [turns](turns.md). |

Goal rules are in [turns](turns.md). On a managed host the goal tools work on
state that only the server holds; the session runtime does not see it.

### Resources

All resources are Markdown.

| URI | Name |
|---|---|
| `nori://context/cli` | Nori CLI operating context |
| `nori://context/repo` | Answering with Nori CLI source |
| `nori://help/custom-acp-agent` | Custom ACP agent configuration reference |
| `nori://help/acp-wire-logs` | ACP wire logs reference |

### Prompts

| Prompt | Points the agent to |
|---|---|
| `register_custom_acp_agent` | `nori://help/custom-acp-agent`: add or check an `[[agents]]` entry |
| `debug_acp_wire_protocol` | `nori://help/acp-wire-logs`: build a timeline from ACP wire logs |
| `answer_nori_cli_question` | `nori://context/repo`: answer from the Nori CLI source |

## Code

- `nori-rs/nori-config/src/config_types.rs` (`McpServerConfig`): the schema.
- `nori-rs/nori-config/src/loader.rs` (`resolve_mcp_servers`): the reserved
  name.
- `nori-rs/acp-host/src/connection/mcp.rs` (`to_acp_mcp_servers`): config
  to ACP, OAuth header.
- `nori-rs/host-runtime/src/host/acp/session.rs` (`mcp_server`): client
  servers on `nori host`.
- `nori-rs/rmcp-client/src/oauth.rs`,
  `nori-rs/rmcp-client/src/perform_oauth_login.rs`: OAuth storage and sign-in.
- `nori-rs/harness/src/backend/nori_client_mcp.rs`,
  `nori-rs/harness/src/backend/nori_client_context.rs`: `nori-client`.
- `nori-rs/harness/src/backend/managed_additions.rs` (`prepare_mcp`):
  `nori-client` on a managed host.

## Where `main` differs

- The MCP OAuth fallback file is `$CODEX_HOME/.credentials.json`, else
  `~/.codex/.credentials.json`, not `$NORI_HOME/.credentials.json`
  (`rmcp-client/src/oauth.rs::fallback_file_path`). Nothing migrates it.
- Other uses of the old Codex home, besides reads of the Codex agent's own
  `~/.codex` for that agent:
  - The TUI session header lists `~/.codex/AGENTS.md` as an instruction file
    for every agent (`tui/src/nori/session_header/mod.rs`).
  - The Windows sandbox exec path resolves its state folder with
    `find_codex_home()` (`sandbox/src/exec.rs::exec_windows_sandbox`). The
    crate has no `config` module, so this path may not build.
  - The inherited `codex-core` config loader reads `$CODEX_HOME/config.toml`
    (`core/src/config/mod.rs::Config::load_with_cli_overrides`,
    `core/src/config/edit.rs`). Nothing in the `nori` binary calls it.
- Nori does not warn about `startup_timeout_sec`, `startup_timeout_ms`,
  `tool_timeout_sec`, `enabled_tools`, or `disabled_tools`. It accepts them
  silently.
- Unknown keys in `[mcp_servers.<name>]` are ignored silently.
- `nori host` accepts a client server named `nori-client` and does not
  reserve the name, so the agent can get two servers with that name.
- On `nori host`, an HTTP client server with its own `Authorization` header
  still gets a stored OAuth header added when a token exists for its name and
  URL.
