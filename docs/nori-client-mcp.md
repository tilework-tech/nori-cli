# nori-client MCP and Goals

`nori-client` is the backend-owned MCP server through which an MCP-capable
[agent](glossary.md#actors) reads and mutates Nori-owned state. Nori owns
`/goal` state in the [session harness](glossary.md#nori-runtime-boundaries);
`nori-client` and the `_session/goal` extension are the only two ways an agent
may participate in it. Code: `nori-rs/harness/src/backend/nori_client_mcp.rs`,
`nori_client_context.rs`, `thread_goal.rs`, `goal_ext.rs`.

## Rules

- Tools must only read or mutate Nori-owned live state. Resources must be
  durable read-only facts. Prompts must package reusable workflows.
- Resources and prompts must come from the fixed catalog in
  `nori_client_context.rs`. The server must never become a filesystem reader,
  a second goal store, a substitute for capabilities ACP already provides, or
  a way to mutate user configuration without explicit user action.
- The goal store (`ThreadGoalState`) must be the single source of truth. Every
  successful mutation, from any path, must emit `GoalChanged`.

## Server Surface

When the agent's initialize response advertises HTTP MCP
(`mcp_capabilities.http`), the harness must spawn one server per backend and
append it to the session's MCP servers as `McpServer::Http` named
`nori-client`. Agents without HTTP MCP must not receive it.

| Kind      | Name                                                                  |
| --------- | --------------------------------------------------------------------- |
| Tools     | `get_goal`, `create_goal`, `update_goal`                              |
| Resources | `nori://context/cli`, `nori://context/repo`                           |
| Resources | `nori://help/custom-acp-agent`, `nori://help/acp-wire-logs`           |
| Prompts   | `register_custom_acp_agent`, `debug_acp_wire_protocol`, `answer_nori_cli_question` |

Tool contracts:

- `get_goal` returns `{"goal": <snapshot>|null}`.
- `create_goal {objective}` must fail with a tool error when a goal already
  exists. Agents must only create goals when the user or system instructions
  explicitly ask for one.
- `update_goal {status}` accepts only `complete` or `blocked`. Pause, resume,
  and limit transitions belong to the user or system, never the agent.
- Unknown request fields are rejected (`deny_unknown_fields`). Snapshots
  report `token_budget` and `tokens_remaining` as `null`; budgets are not
  implemented.

Server `instructions` must point agents at resources and prompts for context
and repeat the goal-control rules (`NORI_GOAL_CONTROL_INSTRUCTIONS`).

## Ownership and Safety

- The server must bind `127.0.0.1` on an ephemeral port and serve stateless
  streamable HTTP (rmcp) at `/mcp`.
- Each server must generate a random `Bearer` token, advertise it in the ACP
  MCP server `headers`, and reject any request without that exact
  `Authorization` value with `401`.
- The serving task must abort when the owning `NoriClientServer` drops.
- `nori-client` is reserved: config loading must reject a user
  `[mcp_servers.nori-client]` entry (`nori-config/src/loader.rs`).
- Nori's built-in Codex launch must set `features.goals = false` in
  `CODEX_CONFIG` (`acp-host/src/registry.rs`) so Codex-native goal tools never
  compete with Nori's goal state.

## First-Prompt Source Envelope

Every TUI session must prepend exactly one `<context>` block to the first
[prompt request](glossary.md#protocol) and never repeat it. The TUI supplies
both variants (`tui/session_context_http_mcp.md`, `tui/session_context.md`);
the harness selects one by the agent's HTTP MCP capability.

- MCP-capable agents get source attribution plus goal routing: when
  `<goal_context>` is present, read and update it only through `nori-client`.
  They must not receive product explanation they can discover via resources.
- Non-MCP agents get source attribution, "operating over ACP", the
  `https://github.com/tilework-tech/nori-cli` source reference, and an
  explicit statement that MCP-backed affordances, including `update_goal`,
  are unavailable.

## Goal Control Paths

`/goal` must have a close-the-loop path. A session has one of three:

| Agent advertises                 | Goal driver                                    |
| -------------------------------- | ---------------------------------------------- |
| `_session/goal` (± HTTP MCP)     | Agent's native loop, via the extension         |
| HTTP MCP only                    | Harness continuation loop + `nori-client`      |
| Neither                          | None: `/goal` unavailable                      |

### Harness loop (`nori-client`)

- Every user prompt must be prefixed with `<goal_context>` (status, objective,
  time, tokens) and `<goal_control>` while a goal exists and the server is
  registered.
- When the session goes idle with an active goal and an empty queue, the
  harness must submit a hidden `GoalContinuation` prompt. Continuation is
  gated on server registration, not on the agent having initialized it.
- The agent must finish by calling `update_goal` with exact status `complete`
  (or `blocked` at a genuine impasse) and verify the returned status.

### Extension bridge (`_session/goal`)

- The capability lives in the initialize response's top-level `_meta.goal`:
  `{version: 1, controlMethod: "_…", actions: [...]}`. It is ignored unless
  `version == 1`, `controlMethod` starts with `_`, and `actions` include both
  `set` and `clear`. Unknown actions are ignored.
- Setting an active goal must send `{sessionId, action: "set", objective}` to
  `controlMethod`. Goals created with a non-active status always use the
  `nori-client` loop.
- While the extension drives a goal, the harness must suppress its
  continuation loop and skip `<goal_context>`/`<goal_control>` injection.
- The harness must mirror `session_info_update` `_meta.goal` snapshots into
  the goal store (`null` clears; extension `limited` maps to
  `usage_limited`; malformed values are skipped).
- `/goal pause` and `/goal resume` must fail with an explicit error unless the
  capability lists that action.
- If `set` fails and `nori-client` is registered, the harness must fall back
  to the MCP loop; otherwise the error surfaces to the user.
- Before another path takes over, and on `/compact` or `/fork` session swaps,
  the harness must stop mirroring and best-effort send `clear` to the old
  native loop.

### No goal path

- `ThreadGoal*` backend operations must fail with "goal management is
  unavailable for this session".
- Replayed goals must stay visible as history but must not inject goal
  context or trigger continuations.
- Resuming a session whose goal is active, paused, blocked, or usage-limited
  must emit the `/goal is unavailable` notice instead of resume hints.

## Capability Projection

`SessionCapabilitiesChanged` is a full snapshot, never a feature-specific
event. It carries:

- `agent`: raw ACP capabilities (HTTP MCP, `session/load`, list, resume,
  close, fork),
- `nori_client.advertised` and `nori_client.initialized` (set on the first MCP
  `initialize` from the agent, which re-emits the snapshot), and
- `builtin_commands["goal"]`: enabled iff `nori-client` is advertised or the
  goal extension is valid, with a reason string when disabled.

The harness must emit it on spawn, load/resume, and every `/compact` or
`/fork` session swap (re-registering `nori-client` for the new session). The
TUI must render command availability from `builtin_commands`, keep `/goal`
visible but disabled with the reason, and reject typed `/goal` submissions.

## Verification

- `nori-rs/harness/src/backend/nori_client_mcp.rs` tests: tool contracts,
  real-HTTP-client round trips (authenticated), resource and prompt
  discovery, `/goal` availability projection.
- `nori-rs/harness/src/backend/thread_goal.rs` tests: goal state, replay
  rehydration, prompt context, continuation, resume notices with and without
  goal automation.
- `nori-rs/harness/tests/session_event_boundary.rs`: first-prompt envelope
  once, MCP vs non-MCP variants.
- `nori-rs/harness/tests/goal_ext_bridge.rs`: extension set/clear against the
  mock agent (`MOCK_AGENT_GOAL_EXT`, `MOCK_AGENT_GOAL_EXT_AUTOCOMPLETE`).
- `nori-rs/nori-config/src/loader.rs`: reserved-name rejection.
- `nori-rs/acp-host/src/registry.rs`: Codex native goals disabled.

Known gaps: no test asserts a `401` for a missing bearer token, and no TUI
test covers the disabled `/goal` popup or typed-command rejection.
