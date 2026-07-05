# Goodbye Card Session Stats Implementation Plan

**Goal:** Make the Nori goodbye card accurately summarize ACP session messages, tool calls, skills, and subagents for the common Claude/Codex paths.

**Architecture:** Keep the stats logic in the TUI where the goodbye card already reads `SessionStats`. Feed `SessionStats` from normalized `nori_protocol::ClientEvent` values, especially `ToolSnapshot` and `PromptCompleted`, so the implementation depends on common ACP event shapes instead of backend-specific transcript schemas. Add a small, generic transcript fallback only for subagent launches that are known to be missing from visible ACP events.

**Tech Stack:** Rust, `nori_protocol::ClientEvent`, `nori_protocol::ToolSnapshot`, existing `nori-tui` chat widget tests, `insta` snapshots, `tui-pty-e2e`.

---

**Testing Plan**

I will add behavior tests before implementation.

I will add `nori-rs/tui/src/chatwidget/tests/part8.rs` and register it from `nori-rs/tui/src/chatwidget/tests/mod.rs`. These tests will drive `ChatWidget::handle_client_event` with real `nori_protocol::ClientEvent::ToolSnapshot` and `PromptCompleted` values, then assert the rendered exit card or `session_stats()` behavior. This tests the same boundary the ACP backend feeds during real TUI sessions.

The first test will send pending and completed `ToolSnapshot` updates for the same `call_id`, then verify the tool is counted once. It will include a completed `read` snapshot for `/tmp/repro-skill/SKILL.md` and a completed `execute` snapshot for `printf`, then verify the stats include `read: 1`, `execute: 1`, and skill `repro-skill`. This proves dedupe, grouping by normalized name, and `SKILL.md` path detection without testing only a helper.

The second test will send a Claude-shaped agent tool snapshot using the real generic ACP-normalized shape: title `Agent`, kind `Other("Other")` or another generic `Other(...)`, and `raw_input.subagent_type = "nori-task-runner"`. It will verify the stats fall back to the title, count the `Agent` tool once, and include subagent `nori-task-runner`. If this can be constructed through the existing ACP-to-`ClientEvent` normalization path instead of hand-building `ToolSnapshot`, prefer that boundary.

The third test will send ACP answer deltas followed by `PromptCompleted { last_agent_message: Some(...) }`, then verify the exit card shows `Assistant: 1`. This covers the captured bug shape where the card showed `Assistant: 0` after a visible assistant answer.

The fourth test must cover streamed ACP answers where the completion has no final message payload: send `MessageDelta { stream: Answer, delta: "Done" }` followed by `PromptCompleted { last_agent_message: None, ... }`, then verify the exit card/session stats show `Assistant: 1`. Existing chat widget tests already exercise this visible-stream/no-final-message shape, so the goodbye-card stats tests should force it too.

I will add focused unit tests in `nori-rs/tui/src/session_stats.rs` only for path/string extraction behavior that is awkward to exercise through `ChatWidget`, such as recursively scanning JSON string values for `*/SKILL.md` and extracting subagent names from `subagent_type`, `agentType`, or `agentId`.

If the transcript fallback is implemented in the same PR, I will add tests in `nori-rs/acp/src/transcript_discovery.rs` that create small JSONL fixtures containing Codex-style `function_call` records named `spawn_agent`. The test will verify the parser returns one subagent usage per unique call id and ignores unrelated function calls.

I will add or update `insta` snapshot coverage for a goodbye card containing ACP-derived `read`/`execute` tools, a detected `repro-skill`, a detected subagent, and `Assistant: 1`. Replacing false `(none)` values is the user-visible bug, so this snapshot coverage is required rather than optional.

NOTE: I will write _all_ tests before I add any implementation behavior.

## Current Code Path

Relevant files in this worktree:

- `nori-rs/tui/src/session_stats.rs`: owns `SessionStats`, current extraction helpers, and an older standalone `SessionStatisticsCell`.
- `nori-rs/tui/src/nori/exit_message.rs`: renders the actual goodbye card from `SessionStats`.
- `nori-rs/tui/src/chatwidget/event_handlers.rs`: receives `ClientEvent` and core Codex events, updates history, and currently updates some stats.
- `nori-rs/tui/src/chatwidget/user_input.rs`: creates the exit message cell from `self.session_stats().clone()`.
- `nori-rs/nori-protocol/src/lib.rs`: defines normalized `ClientEvent`, `ToolSnapshot`, `ToolKind`, `ToolPhase`, locations, invocations, raw input, and raw output.
- `nori-rs/acp/src/transcript_discovery.rs`: already discovers external agent transcript files and parses token usage; this is the least invasive place for the optional `spawn_agent` fallback.

The current gap is that `handle_client_tool_snapshot()` records stats only for completed create/edit/delete/move tools. ACP `read`, `execute`, `fetch`, `think`, and agent-like `Other(...)` snapshots render in the transcript but do not update goodbye-card stats. ACP assistant answers also flow through `MessageDelta`/`PromptCompleted`, not the core `AgentMessage` path that currently increments assistant message stats.

## Implementation Tasks

1. Extend `SessionStats` with call-id dedupe.

   In `nori-rs/tui/src/session_stats.rs`, add a private `HashSet<String>` to remember tool call ids already counted. Keep the public display fields as they are unless a test forces a different shape.

   Add a method with this behavior:
   - Accept `&nori_protocol::ToolSnapshot`.
   - If the `call_id` is new, increment exactly one tool group.
   - Always scan the snapshot for skill and subagent signals, even if the `call_id` has already been counted, because later `tool_call_update` snapshots may add locations or raw input that the initial snapshot lacked.
   - Return nothing; callers should not need to know whether anything changed.

   Avoid adding a second stats type unless the implementation gets forced there. The existing `SessionStats` is already the state the goodbye card consumes.

2. Normalize tool names from `ToolSnapshot`.

   Add one local helper in `session_stats.rs` for display names:
   - For `ToolKind::Other(name)`, use `name` when non-empty and not generic.
   - For normal `ToolKind` values, use `crate::client_event_format::format_tool_kind(&snapshot.kind)`.
   - If the result is empty or generic, including `Other`, fall back to `snapshot.title`.
   - Trim whitespace.

   This keeps Codex-style tools grouped as `read`/`execute` while still allowing Claude-shaped `Agent` or `Task` snapshots to display as the actual agent tool name.

3. Extract skills from normalized snapshots.

   Replace the narrow "Skill tool raw input" path with generic extraction from common snapshot surfaces:
   - `snapshot.locations[*].path`
   - `Invocation::Read { path }`
   - `Invocation::Search { path: Some(...) }`
   - all string values recursively inside `snapshot.raw_input`
   - all string values recursively inside `snapshot.raw_output`
   - `Artifact::Text { text }`

   Any string/path ending in `/<skill-dir>/SKILL.md` records `<skill-dir>`. Keep names unique with the existing `record_skill()` behavior.

   Keep `extract_skill_from_read_path()` if it remains useful, but remove duplicate special-case helpers that become obsolete. This file already has some stale "RED PHASE" comments and a duplicate `SessionStatisticsCell`; clean only what is touched and clearly unused.

4. Extract subagents from normalized snapshots.

   Treat a snapshot as subagent-related when its normalized tool name is case-insensitively equal to one of:
   - `agent`
   - `task`
   - `spawn_agent`

   For the subagent display name, prefer these structured fields from `raw_input` and then `raw_output`:
   - `subagent_type`
   - `agentType`
   - `agent_type`
   - `agentId`
   - `agent_id`

   If no structured value is present, record the normalized tool name. Keep names unique with `record_subagent()`.

   Do not count every `think` tool as a subagent. That would inflate stats for ordinary reasoning/tool-planning events.

5. Update `ChatWidget` ACP event handling.

   In `nori-rs/tui/src/chatwidget/event_handlers.rs`, call the new `self.session_stats.record_tool_snapshot(&tool_snapshot)` near the top of `handle_client_tool_snapshot()`, before rendering logic can return early for duplicate/completed cells.

   Then remove or narrow the existing stats recording inside the completed create/edit/delete/move branch so those calls are not double-counted.

   Preserve the existing rendering behavior for `ClientToolCell`, pending execute buffering, and exploring-cell merging. The stats update should be independent from transcript rendering state.

6. Count ACP assistant messages.

   In `handle_client_prompt_completed()`, record one assistant message when `completed.last_agent_message.as_deref()` is non-empty.

   Also handle streamed ACP answers where `last_agent_message` is absent. Use a single boolean in `ChatWidget` to remember that an answer stream produced non-whitespace content during the current turn, then increment on prompt completion. Keep this state private to `ChatWidget` and reset it at task start/completion.

   Do not increment on every answer delta. The goodbye card wants assistant messages, not chunks.

7. Add the narrow transcript fallback only if the ACP-first tests pass cleanly.

   In `nori-rs/acp/src/transcript_discovery.rs`, add a generic parser for already-discovered transcript files that scans JSONL entries for function/tool calls named `spawn_agent`, `Agent`, or `Task`.

   Expected behavior:
   - Parse each line as `serde_json::Value`.
   - Look for call ids in common keys: `call_id`, `id`, or nested `message.content[*].id`.
   - Look for call names in common keys: `name`, `function.name`, or nested `message.content[*].name`.
   - Extract display names from argument fields: `subagent_type`, `agentType`, `agent_type`, `agentId`, `agent_id`.
   - Deduplicate by call id when present, otherwise by `(name, display_name)`.
   - Return a sorted `Vec<String>` of subagent names.

   Expose the result through `TranscriptLocation`, for example as `subagents_used: Vec<String>`, and populate it during `discover_transcript_for_agent_with_message()`.

   In `nori-rs/tui/src/chatwidget/helpers.rs`, when `apply_system_info_refresh()` receives a `TranscriptLocation`, merge `subagents_used` into `self.session_stats`.

   This fallback is intentionally opportunistic: it uses the same transcript discovery already needed for token footer data and should not block exit rendering.

8. Keep the goodbye card renderer mostly unchanged.

   `nori-rs/tui/src/nori/exit_message.rs` should continue rendering from `SessionStats`. Only change it if tests expose ordering or wrapping problems.

   Preserve `(none)` for genuinely empty sections.

9. Documentation update.

   If the goodbye-card spec docs from the previous spec worktree are merged into this branch, update `docs/specs/goodbye-card/initial-session-stats.md` with the final implementation status.

   If those docs are not merged yet, add a short note to the implementation PR body instead of creating a second competing spec path.

## Backwards Compatibility

The public user-facing behavior only changes when stats exist but were previously missed. Empty sessions should still show `(none)`.

`SessionStats` is internal to `nori-tui`; adding private dedupe state should not affect serialized data or external APIs. If direct struct literals exist, prefer adding a custom constructor/default path rather than making new fields public.

Transcript fallback data added to `TranscriptLocation` is internal between `nori-harness` and `nori-tui`. Use `#[serde(default)]` only if the type is serialized anywhere; otherwise no compatibility shim is needed.

## Edge Cases

Handle a `tool_call` arriving before locations/raw input and a later `tool_call_update` adding them. Count the tool once, but still extract skills/subagents from the later update.

Handle failed tool calls. They should count as tool calls if they reached the visible session, because the goodbye card summarizes activity, not only successful work.

Handle duplicate skill reads. A session that reads `repro-skill/SKILL.md` five times should list `repro-skill` once.

Handle relative, absolute, and home-relative-looking paths. The extractor only needs the trailing `/<skill-dir>/SKILL.md` shape.

Avoid counting arbitrary text mentions like "read SKILL.md" without a parent directory. Require the directory name before `SKILL.md`.

Avoid counting `think` as a subagent unless the title/name is explicitly `Agent`, `Task`, or `spawn_agent`.

Do not parse nested subagent transcripts for P0. The fallback should scan only the discovered visible-session transcript.

## Verification Commands

From `/home/clifford/Documents/source/nori/cli/.worktrees/plan-goodbye-card-session-stats/nori-rs`:

```bash
cargo test -p nori-tui session_stats
cargo test -p nori-tui chatwidget::tests::part8
cargo test -p nori-tui nori::exit_message
cargo test -p nori-harness transcript_discovery
cargo build --bin nori
cargo test -p tui-pty-e2e --test exit_statistics
just fmt
just fix -p nori-tui
```

If the transcript fallback touches `nori-harness`, also run:

```bash
just fix -p nori-harness
cargo test -p nori-harness
```

Before finalizing, close the loop with the TUI using the repo's tmux workflow:

1. Build `nori`.
2. Launch a persistent shell in tmux.
3. Run `./target/debug/nori --agent elizacp --skip-trust-directory`.
4. Submit a simple prompt.
5. Exit with `/exit`.
6. Verify the goodbye card still renders and still shows `(none)` only when the local test agent did not emit tool/subagent activity.

For the original bug shape, repeat with Claude/Codex fixtures or real agents only after the unit/integration tests pass.

**Testing Details** The main tests drive `ChatWidget` through real `ClientEvent` values, so they verify user-visible goodbye-card stats behavior instead of testing mocks or private data structures. Focused extraction unit tests cover only string/path parsing cases that are too small to justify full TUI event setup.

**Implementation Details**

- Add tool-call dedupe to `SessionStats`.
- Record every ACP `ToolSnapshot`, not just completed edit-like snapshots.
- Count tools once per `call_id`.
- Continue scanning duplicate snapshots for newly available paths/raw input.
- Detect skills from `*/<skill-dir>/SKILL.md`.
- Detect subagents from `Agent`, `Task`, and `spawn_agent`-like tool calls.
- Count ACP assistant messages on prompt completion, not per streamed chunk.
- Keep the exit card renderer as a consumer of stats, not a parser.
- Keep the Codex transcript fallback small and generic.
- Remove touched obsolete helper paths only when tests prove they are redundant.

**Question** The only meaningful uncertainty is whether Codex `spawn_agent` should be P0 if the visible ACP stream does not expose it. The lowest-risk plan is to land ACP-first stats first, then include the transcript fallback only if it stays small and uses the existing transcript discovery path.

---
