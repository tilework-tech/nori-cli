# nori-client MCP and Goals

`nori-client` is the backend-owned MCP server through which an MCP-capable
agent reads and changes Nori-owned state. Nori owns `/goal` state in the
session harness; `nori-client` and the `_session/goal` extension are the only
two ways an agent may take part in it. Terms follow the
[glossary](glossary.md).

Code: `nori-rs/harness/src/backend/nori_client_mcp.rs`,
`nori-rs/harness/src/backend/nori_client_context.rs`,
`nori-rs/harness/src/backend/thread_goal.rs`,
`nori-rs/harness/src/backend/goal_ext.rs`.

## Rules

- Tools must only read or change Nori-owned live state. Resources must be
  durable read-only facts. Prompts must package reusable workflows.
- Resources and prompts must come from the fixed catalog in
  `nori_client_context.rs`. The server must never become a filesystem reader,
  a second goal store, a substitute for capabilities ACP already provides, or
  a way to change user configuration without explicit user action.
- The goal store (`ThreadGoalState`) is the single source of truth. Every
  successful mutation, from any path, must emit `GoalChanged`.

## Server Surface

When the agent's initialize response advertises HTTP MCP
(`mcp_capabilities.http`), the harness spawns one server per backend and
appends it to the session's MCP servers as `McpServer::Http` named
`nori-client`. Agents without HTTP MCP do not receive it.

| Kind      | Name                                                                               |
| --------- | ---------------------------------------------------------------------------------- |
| Tools     | `get_goal`, `create_goal`, `update_goal`                                           |
| Tools     | `delegate` (only in sessions the session manager runs)                             |
| Resources | `nori://context/cli`, `nori://context/repo`                                        |
| Resources | `nori://help/custom-acp-agent`, `nori://help/acp-wire-logs`                        |
| Prompts   | `register_custom_acp_agent`, `debug_acp_wire_protocol`, `answer_nori_cli_question` |

Tool contracts:

- `get_goal` returns `{"goal": <snapshot>|null}`. Snapshots report
  `token_budget` and `tokens_remaining` as `null`; budgets are not
  implemented.
- `create_goal {objective}` fails with a tool error when a goal already
  exists. Agents must only create goals when the user or system instructions
  explicitly ask for one.
- `update_goal {status}` accepts only `complete` or `blocked`, and fails when
  no goal exists. Pause, resume, and limit transitions belong to the user or
  the harness, never the agent.
- `delegate {agent, task}` starts a child session and returns
  `{"delegation": <record>}`. The route exists only when the session runs
  under the session manager (the TUI, `nori host`, and `nori exec`, which
  runs through a host); otherwise the tool is not listed. A child of the same agent kind
  starts immediately, a different kind waits for user approval, children may
  nest at most two levels below the top session, and children share the
  session cap. See [remote control](remote-control.md#delegation) for the
  delegation model.
- All tool requests reject unknown fields.

Resources are `text/markdown`. An unknown resource URI returns a
resource-not-found error; an unknown prompt name returns invalid params.

- `nori://context/cli` states three facts: Nori CLI is the current harness,
  ACP is the JSON-RPC protocol between Nori and the agent (with the source repo
  URL), and `nori-client` is an internal backend server, not a user-configured
  one.
- `nori://context/repo` is a short source map that tells the agent which area
  of the repo to read first for a Nori CLI implementation question and asks it
  to cite the files it used. `answer_nori_cli_question` points at it. Its
  directory list is maintained by hand in `nori_client_context.rs`; check the
  tree rather than trusting the map's paths.
- The help resources describe custom `[[agents]]` configuration and ACP wire
  recording; the matching prompts tell the agent to read them first.

Server `instructions` point agents at resources and prompts for context and
repeat the goal-control rules (`NORI_GOAL_CONTROL_INSTRUCTIONS` in
`thread_goal.rs`).

## Ownership and Safety

- The server binds `127.0.0.1` on an ephemeral port and serves streamable
  HTTP (rmcp) at `/mcp`.
- Each server instance generates a random bearer token and advertises it in
  the ACP MCP server `headers`. Requests whose `Authorization` header is
  missing or is not exactly `Bearer <token>` get HTTP `401` before they reach
  the MCP service. The token belongs to the server instance, so it survives
  `/compact` and `/fork` session swaps on the same backend.
- The serving task aborts when the owning `NoriClientServer` drops.
- `nori-client` is a reserved name: config loading rejects a user
  `[mcp_servers.nori-client]` entry (`nori-rs/nori-config/src/loader.rs`).
- Nori's built-in Codex launch sets `features.goals = false` in
  `CODEX_CONFIG` (`nori-rs/acp-host/src/registry.rs`) so Codex-native goal
  tools never compete with Nori's goal state.

### Background host relay

The managed host's `/managed` relay can inject its own `nori-client` on
`session/new`, `session/load`, `session/resume`, and `session/fork`. This
needs a managed launch (`[host] managed = true`) whose additions enable
`clientMcp`, and an agent that advertises HTTP MCP. A caller-supplied server
named `nori-client` suppresses injection
(`nori-rs/harness/src/backend/managed_additions.rs`).

That instance has its own goal store, which is discarded with the server: its
goal tools emit no `GoalChanged` events and do not reach any Nori goal state.
It never exposes `delegate`.

## First-Prompt Source Envelope

Only the TUI (local and cloud) supplies a source envelope. The background
host sends none, and so does `nori exec`: one-shot exec runs through a host,
and `nori exec --acp` is `nori host --stdio`. When the TUI
supplies one, the harness prepends exactly one `<context>` block to the first
prompt request and never repeats it. The TUI passes both variants
(`nori-rs/tui/session_context_http_mcp.md`, `nori-rs/tui/session_context.md`);
the harness picks one by the agent's HTTP MCP capability.

- MCP-capable agents get source attribution plus a goal-routing rule: when
  `<goal_context>` is present, read and update it only through `nori-client`.
  The rule is always included in this variant. They get no product
  explanation they can find through resources.
- Non-MCP agents get source attribution, a note that they operate over ACP,
  the `https://github.com/tilework-tech/nori-cli` source reference, and an
  explicit statement that MCP-backed affordances, including `update_goal`,
  are unavailable.

## Goal Control Paths

`/goal` needs a close-the-loop path. A session has one of three:

| Agent advertises              | Goal driver                               |
| ----------------------------- | ----------------------------------------- |
| `_session/goal` (± HTTP MCP)  | Agent's native loop, via the extension    |
| HTTP MCP only                 | Harness continuation loop + `nori-client` |
| Neither                       | None: `/goal` unavailable                 |

### Harness loop (`nori-client`)

- While a goal exists and the server is registered, every user prompt is
  prefixed with `<goal_context>` (status, objective, time, tokens) and
  `<goal_control>`.
- After a turn ends with `end_turn`, while the session is idle, the goal is
  active, and the queue is empty, the harness submits a hidden
  `GoalContinuation` prompt.
- Continuation rules:
  - The first continuation after a user turn needs only a registered
    `nori-client`.
  - A continuation after another continuation also needs the agent to have
    connected to `nori-client` (its MCP `initialize`). Until then the agent
    cannot mark the goal finished, so chaining stops.
- The agent must finish by calling `update_goal` with exact status `complete`
  (or `blocked` at a genuine impasse) and check the returned status.

### Extension bridge (`_session/goal`)

- The capability lives in the initialize response's top-level `_meta.goal`:
  `{version: 1, controlMethod: "_…", actions: [...]}`. It is ignored unless
  `version == 1`, `controlMethod` starts with `_`, and `actions` include both
  `set` and `clear`. Unknown actions are ignored.
- Setting an active goal sends `{sessionId, action: "set", objective}` to
  `controlMethod`. Goals created with a non-active status always use the
  `nori-client` loop.
- While the extension drives a goal, the harness suppresses its continuation
  loop and skips `<goal_context>`/`<goal_control>` injection.
- The harness mirrors `session_info_update` `_meta.goal` snapshots into the
  goal store (`null` clears; extension `limited` maps to `usage_limited`;
  malformed values are skipped).
- `/goal pause` and `/goal resume` fail with an explicit error unless the
  capability lists that action.
- If `set` fails and `nori-client` is registered, the harness falls back to
  the MCP loop; otherwise the error reaches the user.
- Before another path takes over, and on `/compact` or `/fork` session swaps,
  the harness stops mirroring and sends `clear` to the old native loop on a
  best-effort basis.

### No goal path

Agents with neither HTTP MCP nor a valid `_session/goal` capability:

- The TUI keeps `/goal` visible but disabled, with the reason, and rejects
  typed `/goal` submissions.
- `ThreadGoal*` backend operations fail with "goal management is unavailable
  for this session".
- Replayed goals stay visible as history but do not inject goal context or
  trigger continuations.
- Resuming a session whose goal is active, paused, blocked, or usage-limited
  emits a `/goal is unavailable` notice instead of resume hints.

## Capability Projection

`SessionCapabilitiesChanged` is a full snapshot, never a feature-specific
event. It carries:

- `agent`: raw ACP capabilities (HTTP MCP, `session/load`, list, resume,
  close, fork),
- `nori_client.advertised` and `nori_client.initialized` (set on the agent's
  first MCP `initialize`, which re-emits the snapshot), and
- `builtin_commands["goal"]`: enabled when `nori-client` is advertised or the
  goal extension is valid, with a reason string when disabled.

The harness emits it on spawn, load or resume, and every `/compact` or
`/fork` session swap. A swap re-advertises the existing server and registers
a new one only when none exists. A registration failure fails startup on
spawn or load, but only logs a warning on a swap. The TUI renders command
availability from `builtin_commands`, not from raw ACP capabilities.

## Verification

- `nori-rs/harness/src/backend/nori_client_mcp.rs` tests: tool contracts,
  authenticated real-HTTP-client round trips, resource and prompt discovery,
  `delegate` hidden without the session manager, `/goal` availability.
- `nori-rs/harness/src/backend/thread_goal.rs` tests: goal state, replay
  rehydration, prompt context, continuation, resume notices.
- `nori-rs/harness/tests/session_event_boundary.rs`: envelope sent once,
  MCP and non-MCP variants.
- `nori-rs/harness/tests/goal_ext_bridge.rs`: extension set/clear against the
  mock agent (`MOCK_AGENT_GOAL_EXT`, `MOCK_AGENT_GOAL_EXT_AUTOCOMPLETE`).
- `nori-rs/harness/tests/delegation.rs`: `delegate` over a real MCP call,
  approval, depth limit, and the shared session cap.
- `nori-rs/nori-config/src/loader.rs`: reserved-name rejection.
- `nori-rs/acp-host/src/registry.rs`: Codex native goals disabled.

Known gaps: no test asserts the `401` for a missing or wrong bearer token, and
no TUI test covers the disabled `/goal` popup or typed-command rejection.
