# ACP `session/load` Implementation Plan (Nori ACP Backend)

## Goal

Enable the ACP client (`codex-acp`) to request an explicit session ID from an agent and replay the full conversation over `session/update` notifications, so a user can resume an existing agent session. This plan is scoped to the ACP backend and the `nori` binary. It relies on the ACP spec requirement that agents replay the full conversation through `session/update` and only send the `session/load` response after the replay is complete.【F:codex-rs/acp/docs.md†L1-L23】【F:codex-rs/acp/src/backend.rs†L1-L18】

## Protocol requirements to implement

- Clients **must** check the agent’s `loadSession` capability before calling `session/load`.【F:/workspace/agent-client-protocol/src/agent.rs†L2389-L2445】【F:/workspace/agent-client-protocol/docs/protocol/session-setup.mdx†L88-L103】
- `session/load` requests require the `sessionId`, `cwd`, and optional MCP server list; the agent replays the full conversation as `session/update` notifications before completing the request.【F:/workspace/agent-client-protocol/src/agent.rs†L510-L563】【F:/workspace/agent-client-protocol/docs/protocol/session-setup.mdx†L103-L189】
- `session/update` replays user/agent messages (and tool call events) in the same streaming format used for `session/prompt`, so we can reuse the existing translation path in `codex-acp` that turns `SessionUpdate` into TUI events.【F:/workspace/agent-client-protocol/docs/protocol/session-setup.mdx†L137-L189】【F:codex-rs/acp/src/backend.rs†L818-L1035】

## Current state in Nori

- `codex-acp` only creates new sessions via `AcpConnection::create_session` and does not expose a `load_session` path.【F:codex-rs/acp/src/connection.rs†L241-L306】
- Session updates are already translated to `codex_protocol::Event` via `translate_session_update_to_events`, and are consumed through an update channel in `AcpBackend::handle_user_input`. This update-consumer pipeline can be reused for replayed updates from `session/load`.【F:codex-rs/acp/src/backend.rs†L430-L592】【F:codex-rs/acp/src/backend.rs†L818-L1035】
- MCP server configuration exists in `codex-acp` config types, but is not yet passed into ACP session creation/load requests.【F:codex-rs/acp/src/config/types.rs†L214-L312】

## Implementation plan

### 1) Protocol surface in `codex-acp`

- **Add a `load_session` command in `AcpConnection`:**
  - Extend `AcpCommand` with a `LoadSession` variant carrying `session_id`, `cwd`, MCP server list, and an `update_tx` channel for replayed updates.
  - Implement `AcpConnection::load_session(...)` alongside `create_session`, mirroring the `prompt` flow: send the command to the worker thread, stream `session/update` notifications into the provided `update_tx`, and only return once the `session/load` response arrives.
  - Gate the call with `self.agent_capabilities.load_session`; if false, return a clear error that is surfaced to the UI.
  - Capture `LoadSessionResponse.models` (when `unstable_session_model` is enabled) to update `AcpModelState`, keeping parity with how `NewSessionResponse.models` is handled today.【F:codex-rs/acp/src/connection.rs†L84-L149】【F:/workspace/agent-client-protocol/src/agent.rs†L567-L620】

### 2) Session start flow in `AcpBackend`

- **Add a “session start mode” to `AcpBackendConfig`:**
  - Extend config to accept an optional session ID (e.g., `session_id: Option<String>`). This can be wired from CLI flags or persisted config later.
  - On spawn, decide between:
    - `create_session(cwd, mcp_servers)` when `session_id` is `None`.
    - `load_session(session_id, cwd, mcp_servers)` when provided.
- **Replay handling:**
  - Factor the update consumer used in `handle_user_input` into a reusable helper (`spawn_update_consumer`) so it can be used for both replay and live prompts.
  - When loading a session, stream the replayed updates through the same translation pipeline so the TUI reconstructs chat history uniformly with the prompt path.【F:codex-rs/acp/src/backend.rs†L430-L592】【F:codex-rs/acp/src/backend.rs†L818-L1035】

### 3) MCP server mapping

- **Translate `codex-acp` MCP configuration into ACP `McpServer` structs** for both `session/new` and `session/load`:
  - Use `McpServerConfig` (stdio vs. HTTP transport) to build `acp::McpServer` values.
  - Include only `enabled` servers, and preserve tool allow/deny lists and timeouts when the ACP schema supports them.
  - Add tests for config-to-ACP conversion to avoid regressions.【F:codex-rs/acp/src/config/types.rs†L214-L312】【F:/workspace/agent-client-protocol/src/agent.rs†L360-L418】

### 4) CLI + TUI integration

- **CLI:** add a `--session <id>` (or equivalent) flag in the `nori` CLI that populates `AcpBackendConfig.session_id`.
- **TUI:** when in ACP mode, accept a session ID from the CLI or config and invoke the load flow on startup. Reuse the existing “Connecting to [Agent]” UI for the replay phase and show errors if `loadSession` is unsupported.

### 5) Mock agent + tests

- **Mock ACP agent (`mock-acp-agent`):**
  - Implement `load_session` to emit a deterministic replay via `session/update` (user/agent chunks plus tool call updates) before returning `LoadSessionResponse`, enabling repeatable tests.
  - Add env-gated behavior (e.g., `MOCK_AGENT_LOAD_SESSION_REPLAY=1`) to keep existing tests stable.【F:codex-rs/mock-acp-agent/src/main.rs†L211-L266】
- **codex-acp integration tests:**
  - Add a new test that uses the mock agent to load a session, asserts that updates are replayed, and verifies the backend emits the correct `codex_protocol::Event` sequence.
  - Validate error handling when `loadSession` capability is absent.
- **TUI E2E tests (ACP mode):**
  - Add a PTY test that launches with a session ID and confirms the chat history is rendered from replayed updates.

## Documentation updates (after implementation)

- Update the ACP module docs (`codex-rs/acp/docs.md`) to describe session loading support, configuration, and user-facing CLI flag usage.
- Extend the TUI/CLI docs if needed to mention the resume workflow and compatibility requirements.
