# Goodbye Card

The Nori goodbye card is a TUI presentation over `SessionStats`. It must not
define or depend on a public ACP normalization protocol.

## Session stats source

For ACP-backed sessions, the TUI's private presentation reducer observes raw
ACP session notifications and the relevant Nori lifecycle events. It records
completed or failed tool calls once per ACP call ID, assistant turns once per
completed answer, and token information when an agent exposes it. Tool labels
are presentation inferences and may use the ACP tool title as a fallback.

This state never crosses the harness boundary as a normalized `ClientEvent` or
Codex event. Headless embedders receive raw ACP envelopes and may compute their
own statistics without inheriting TUI assumptions.

## Skill and subagent detection

Skill detection scans presentation-visible ACP tool paths and structured values
for paths ending in `/SKILL.md`. Subagent detection recognizes common structured
agent identifier fields. Each discovered skill or subagent is listed once.

Some agents omit delegated subagent launches from visible ACP updates. When the
TUI has already discovered an agent transcript for token-footer data, it may
opportunistically scan that transcript for the same subagent fields. This
fallback contributes names only; tool counts and assistant turns remain driven
by the live presentation path.

## Invariants

- The card does not parse rendered history cells.
- The card does not write derived stats into `nori-protocol` or transcripts.
- Duplicate ACP updates for one call ID do not duplicate tool counts.
- Presentation fallback logic remains private to `nori-tui`.
