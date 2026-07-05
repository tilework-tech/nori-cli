# Goodbye Card

The Nori goodbye card is rendered by `nori-tui` when the user exits the TUI. It is a presentation layer over `SessionStats`; it should not parse agent transcripts or infer behavior directly from rendered history cells.

## Session Stats Source

For ACP-backed Nori sessions, goodbye-card stats are driven primarily by normalized `nori_protocol::ClientEvent` values:

- `ToolSnapshot` records completed or failed tool calls once per `call_id`.
- `ToolSnapshot` surfaces are scanned for skill and subagent signals on every update, including duplicate updates for an already counted call.
- `MessageDelta { stream: Answer, .. }` plus `PromptCompleted` records one assistant message for streamed ACP answers.
- `PromptCompleted { last_agent_message: Some(..), .. }` records one assistant message when the final payload is present.

Tool calls are grouped by normalized ACP tool kind (`read`, `execute`, `edit`, etc.). Generic `Other("Other")` tool snapshots fall back to the snapshot title so agent-style tools can appear as `Agent`.

## Skill Detection

Skills are detected generically from paths or string values ending in:

```text
/<skill-dir>/SKILL.md
```

The extractor scans ACP snapshot locations, invocations, artifacts, raw input, and raw output. The card lists each skill once.

## Subagent Detection

Subagents are detected from common structured fields in ACP snapshot raw input or transcript fallback data:

- `subagent_type`
- `agentType`
- `agent_type`
- `agentId`
- `agent_id`

The card lists each subagent once.

## Transcript Fallback

Some agents do not expose every delegated subagent launch as a visible ACP tool event. When a transcript is already discovered for token footer data, `nori-harness` also scans the discovered JSONL transcript for the same subagent fields, including JSON-encoded argument strings such as Codex `spawn_agent` calls.

This fallback is opportunistic and narrow: it scans only the current discovered transcript and only contributes subagent names. Tool counts, skills, and assistant-message counts remain ACP-event driven.
