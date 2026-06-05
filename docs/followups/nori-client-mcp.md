# nori-client MCP Behavior Spec

This document specifies the desired behavior for the backend-owned
`nori-client` MCP server. It replaces the old append-only
`CURRENT-PROGRESS.md` log, which mixed completed implementation history with
obsolete investigation notes.

## Current State

Nori owns `/goal` state in the ACP backend. Capable ACP agents also receive a
backend-owned MCP server named `nori-client`, served as streamable HTTP on
`127.0.0.1`, so they can call `get_goal`, `create_goal`, and `update_goal`
against the same state the TUI uses.

The server is intentionally general-purpose: goal tools are the first tenant,
not the whole API. It should remain the narrow harness-side channel for
capabilities that ACP does not yet provide directly.

## Target Shape

`nori-client` should be the single structured way an MCP-capable ACP agent
learns about Nori-owned client context. The prompt stream should carry user
work, goal context when goal automation is valid, and one-time fallback context
only when MCP is unavailable. It should not carry repeated product explanation
that an MCP-capable agent could discover through `nori-client`.

The boundary is:

- MCP tools mutate or read Nori-owned live state.
- MCP resources expose durable read-only facts.
- MCP prompts package reusable workflows for the agent.
- Prompt fallback is reserved for agents that cannot receive the MCP server.

## MCP-Capable Agent Behavior

When the active ACP agent advertises HTTP MCP support, Nori should advertise one
loopback streamable-HTTP MCP server named `nori-client`.

The server should expose:

- Goal tools:
  - `get_goal`
  - `create_goal`
  - `update_goal`
- Context resources:
  - `nori://context/cli` - concise operating facts: the agent is running inside
    Nori CLI over ACP, `nori-client` is Nori's harness-side MCP channel, and the
    open source implementation lives at `https://github.com/tilework-tech/nori-cli`.
  - `nori://context/repo` - a compact source map for the Nori CLI repo: ACP
    backend, TUI, protocol normalization, config/agent registry, transcript
    discovery, and MCP support.
- Workflow resources/prompts:
  - `nori://help/custom-acp-agent` and `register_custom_acp_agent`
  - `nori://debug/acp-wire` and `debug_acp_wire_protocol`
  - `nori://source/nori-cli-map` and `answer_nori_cli_question`
  - `nori://skills/workflows` and `choose_nori_workflow`

The agent should be able to discover this context through MCP list/read/get
requests. Nori may include short MCP server instructions that point agents
toward the resources and prompts, but ordinary user prompts should not be
prefixed with the same static Nori operating text for MCP-capable agents.

Resources and prompts should be curated guidance, not an arbitrary filesystem
read API. Tools should remain reserved for Nori-owned state changes.

## Non-MCP Agent Behavior

When the active ACP agent does not advertise HTTP MCP support, Nori should not
advertise `nori-client`.

Non-MCP agents should receive a concise first-prompt-only `<context>` block that:

- says the agent is operating inside Nori CLI over ACP,
- includes `https://github.com/tilework-tech/nori-cli` as the stable source
  reference for implementation questions,
- says MCP-backed Nori affordances are unavailable in this session, and
- names `/goal` completion tools as unavailable rather than implying the agent
  can close the loop through `update_goal`.

That fallback block must be consumed once and must not be repeated on later user
prompts.

## Goal Behavior

`/goal` requires a close-the-loop path: the agent must be able to call the
backend-owned goal tools to mark work complete or blocked. When `nori-client` is
not available, goal automation should be unavailable as behavior, not merely
dimmed as UI.

Required behavior:

- The TUI keeps `/goal` visible but disabled with a clear reason.
- Direct typed `/goal ...` submissions are rejected by the TUI.
- Backend `ThreadGoal*` operations from user-facing paths are also rejected or
  made inert when HTTP MCP is unavailable.
- Replayed active goals should not inject `<goal_context>` or submit hidden goal
  continuations into a non-MCP session. The stored goal may remain visible as
  historical state, but it must not drive autonomous work the agent cannot
  complete.
- A resumed non-MCP session with an existing active goal should emit a clear
  user-visible notice explaining that goal automation is unavailable for the
  active agent.

MCP-capable agents may still delay hidden continuation chaining until the agent
actually initializes `nori-client`; advertising the server and connecting to it
are different states.

## Capability Projection

`SessionCapabilitiesChanged` should remain a snapshot of the current client
state, not a collection of feature-specific one-off events.

The projection should eventually distinguish:

- raw ACP capabilities such as HTTP MCP and `session/load`,
- whether `nori-client` was advertised,
- whether the active agent has initialized `nori-client`, and
- derived builtin command availability such as `/goal`.

Nori should re-emit the capability snapshot whenever a new ACP session is
created or loaded, including compact-created replacement sessions. The TUI
should render command availability from the derived builtin command map rather
than inferring it from raw ACP details.

## Server Ownership And Safety

`nori-client` is a reserved MCP server name. User-configured MCP servers should
not be allowed to shadow or duplicate that name.

Before the server surface grows beyond goal tools, it should use a per-session
loopback authentication mechanism. The expected shape is a generated bearer
token advertised in the ACP MCP server config headers and verified before
requests reach the streamable HTTP MCP service.

The server should stay loopback-only and abort when the owning backend session
drops.

## Verification

The behavior is correct when tests prove:

- HTTP-MCP-capable agents receive a `nori-client` server and can initialize it.
- MCP clients can list and read the Nori context resources.
- MCP clients can list and get the Nori workflow prompts.
- non-MCP agents do not receive `nori-client`.
- non-MCP first prompts receive the fallback `<context>` block exactly once.
- MCP-capable first prompts do not receive the fallback block once the MCP
  context surface exists.
- `/goal` is disabled in the TUI and inert in the backend when `nori-client` is
  unavailable.
- replayed active goals do not inject goal context or hidden continuations into
  non-MCP sessions.
- capability state is refreshed after spawn, resume, and compact-created session
  replacement.

## Non-Goals

This surface should not become:

- a general local file reader,
- a second store for goal state,
- a replacement for ACP capabilities that ACP already provides,
- a way for agents to mutate user configuration without explicit user action, or
- a broad tool bucket for anything that happens to be convenient to expose over
  MCP.

## Superseded Notes

The old `nori-goal` over `acp:<uuid>` failure is no longer the current
architecture. Nori now advertises a real loopback HTTP MCP endpoint named
`nori-client`, served by `rmcp`'s streamable HTTP server. References to
`nori-goal`, ACP pseudo-URLs, or hand-rolled MCP framing should be treated as
historical notes, not active work.
