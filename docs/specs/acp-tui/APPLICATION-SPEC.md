# ACP TUI Rendering — Application Specification

## Goal

Render ACP messages, plans, tools, approvals, usage, modes, and config options
cleanly without turning presentation decisions into a second public protocol.

## Architecture

```text
ACP agent
  -> nori-acp-host
  -> SessionEvent::Acp(AcpEvent)
  -> nori-tui source-first dispatch
  -> private presentation projection
  -> history cells, overlays, footer, and pickers
```

The previous public normalization path through `nori_protocol::ClientEvent`,
`ToolSnapshot`, and Codex event variants was deleted by the ACP-canonical hard
cut. The implementation that infers friendly tool kinds, assembles streaming
messages, extracts invocations and artifacts, and groups tool cells now lives
privately in the TUI. It is allowed to optimize for terminal UX because no
embedder inherits that representation.

## Event handling rules

- Match `SessionEvent::Acp` and `SessionEvent::Nori` first.
- Project ACP notifications directly into private message, plan, tool, usage,
  mode, config, capability, and command presentation state.
- Render ACP permission requests from the raw `AgentRequest`; keep the original
  `RequestId` through overlay completion and call
  `HarnessHandle::respond_to_agent` with a schema-native response.
- Handle lifecycle, queue, replay, compaction, goals, undo, user shell, hooks,
  prompt summaries, notices, and no-response failures only from `NoriEvent`.
- Await history, prompt, undo-list, session-list, config, goal, and close queries
  as typed harness method results. Do not wait for correlated Nori events.

## Tool rendering

The TUI's private presentation reducer may infer semantic categories such as
execute, edit, read, search, fetch, or generic; extract commands and paths;
merge updates by ACP tool-call ID; and render compact cells. Those inferred
types are presentation details. Raw ACP notifications remain the canonical
public and persisted agent-to-client representation.

The existing compact rendering behavior remains the UX contract:

- execute cells show command, output, progress, and final success/failure;
- edit/delete/move cells show semantic headers and diff summaries;
- read/search-like calls may be grouped as exploration;
- incomplete calls are finalized at turn boundaries so they cannot block final
  agent text; and
- streamed updates preserve chronological placement without duplicate cells.

## Approval behavior

Permission options are sourced from ACP. The TUI may assign local keyboard
shortcuts and presentation labels, but it must not replace the ACP request with
a Nori or Codex approval enum. Filesystem requests are currently handled by the
host and therefore do not create an outward approval overlay. Terminal and
extension request families are currently unadvertised.

## Acceptance boundary

Unit and snapshot tests cover private projection and rendering. PTY tests cover
the real `nori` binary, mock ACP agent, raw event ordering, approval round trips,
and terminal output. Headless harness tests prove the same public stream works
without the TUI.
