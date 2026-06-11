# Nori Cloud Lifecycle Implementation Plan

**Goal:** Finish `nori cloud` lifecycle operations so users can start, list, and resume cloud sessions from the CLI while keeping broker/session ownership inside `nori-handroll`.

**Architecture:** `nori cloud` should continue to treat `nori-handroll cloud-acp` as the cloud agent adapter. Handroll owns broker authentication, remote session lifecycle, WebSocket transport, and ACP routing. The CLI/TUI owns local UX, command gating, and picker rendering, but it must not reintroduce a broker client or WebSocket transport into `nori-rs/acp`.

**Tech Stack:** Rust TUI and CLI in `nori-rs`, ACP subprocess integration through `nori-handroll cloud-acp`, nori-sessions broker APIs behind handroll, existing TUI `SelectionViewParams` picker infrastructure, existing slash command availability infrastructure.

---

## Status

Close the existing PR that targeted the previous `nori cloud` architecture.

That PR assumed the CLI and `nori-rs/acp` still owned broker authentication, broker HTTP calls, session acquisition/resume/release, and direct WebSocket connection setup. That is no longer the intended architecture.

Current `main` has moved cloud ownership into `nori-handroll`:

- `nori-rs/cli/src/cloud.rs` only resolves the `nori-handroll` binary and builds a pinned `nori-cloud` agent entry.
- `nori-rs/cli/src/main.rs` handles `nori cloud` by forcing the TUI agent to `nori-cloud` and launching `nori-handroll cloud-acp`.
- The CLI comment is explicit: broker, auth, and transport concerns live in `nori-handroll`.

This spec replaces the old PR direction.

## Product Outcomes

1. `nori cloud` can start a new cloud session.
2. `nori cloud` can show resumable cloud sessions before or inside the TUI.
3. A user can resume sessions that originated from CLI, Slack, Discord, or other broker-supported sources.
4. Local-only CLI/TUI commands are unavailable or adapted in cloud mode.
5. The CLI does not regain direct broker or WebSocket lifecycle responsibility.
6. The implementation reduces accidental complexity by reusing picker and command-availability infrastructure instead of creating parallel UI systems.

## Current Local Resume Behavior

The current local `nori resume` and `/resume` flows are transcript based.

The existing picker in `nori-rs/tui/src/nori/resume_session_picker.rs` builds rows from local transcript metadata. It filters by local agent because different local agents may have incompatible resume formats. Selecting an item sends `AppEvent::ResumeSession`.

`AppEvent::ResumeSession` in `nori-rs/tui/src/app/event_handling.rs` then loads a local transcript with `TranscriptLoader::load_transcript()` and calls `ChatWidget::new_resumed_acp(...)` with the transcript's ACP session id.

That is correct for local sessions and insufficient for cloud sessions. Cloud sessions are broker-owned remote sessions. They may have no local transcript, and sessions from Slack or Discord cannot be discovered by scanning local `*.jsonl` transcript files.

## Ownership Boundaries

### Handroll Owns

Handroll owns all remote cloud lifecycle operations:

- Broker authentication and token refresh.
- Broker base URL and environment handling.
- Listing cloud sessions from the broker.
- Acquiring a new cloud session.
- Resuming a selected cloud session.
- Releasing or gracefully detaching from cloud sessions.
- Mapping broker sessions to ACP router state.
- Connecting to the broker/session transport.
- Normalizing cloud session metadata returned to the CLI.
- Ensuring sessions from CLI, Slack, Discord, and future sources can be listed consistently.

Handroll should expose only a small CLI/TUI-facing interface:

- list cloud sessions as structured metadata
- start new cloud session
- resume selected cloud session
- optionally report cloud capabilities/status

The exact transport for this interface can be a handroll subcommand, an ACP extension, or a small router control surface. The key rule is that the CLI receives generic session rows and user-facing errors; it does not speak broker HTTP directly.

### CLI / TUI Owns

The CLI/TUI owns local interaction and local-policy concerns:

- Detect that the current TUI session is cloud mode.
- Render a cloud session picker using existing selection UI primitives.
- Dispatch "start new cloud session" or "resume this cloud session" to handroll.
- Restart or reconfigure the cloud ACP child when the selected session changes, if that is the selected handroll integration model.
- Disable local-only slash commands and startup flows in cloud mode.
- Show clear disabled-command reasons.
- Avoid local transcript dependency for cloud session listing.
- Preserve local transcript resume behavior for non-cloud sessions.

CLI/TUI should not own:

- Broker auth.
- Broker endpoint paths.
- JWT persistence.
- Direct WebSocket URLs.
- Cloud session release semantics.
- Broker-specific error parsing beyond displaying a handroll-provided message.

## Desired User Journeys

### Journey A: Start A New Cloud Session

1. User runs `nori cloud`.
2. CLI resolves `nori-handroll`.
3. CLI launches the TUI in cloud mode with the pinned `nori-cloud` adapter.
4. If there are no resumable sessions, or if the user chooses "New session", handroll acquires a new cloud session.
5. The TUI starts with local-only commands gated.
6. On exit, handroll handles release or detach semantics.

### Journey B: Resume A Cloud Session From Startup

1. User runs `nori cloud`.
2. CLI asks handroll for cloud session rows.
3. CLI/TUI displays a picker before the conversation is started.
4. The picker includes source information such as CLI, Slack, or Discord when handroll provides it.
5. User selects a session.
6. CLI passes the selected opaque cloud session id to handroll.
7. Handroll resumes that broker session and presents it as the active ACP session.

### Journey C: Resume A Cloud Session From Inside The TUI

1. User is already in `nori cloud`.
2. User invokes `/resume`.
3. The TUI uses the cloud session provider, not `TranscriptLoader`.
4. User selects a remote cloud session.
5. The current cloud conversation is detached or shut down according to handroll lifecycle rules.
6. Handroll resumes the selected session.
7. The TUI shows the resumed conversation state streamed by handroll.

### Journey D: Local Resume Still Works

1. User runs local `nori resume`, `nori resume --last`, or `/resume`.
2. The existing transcript metadata provider remains the data source.
3. The existing local transcript load path remains intact.
4. No cloud broker or handroll session list is involved.

## Session Picker Design

Do not keep the old PR's stdin/stderr session selector as the final UX.

Use the existing TUI picker shape instead:

- `SelectionViewParams`
- `SelectionItem`
- searchable rows
- consistent footer hints
- async summary updates if needed

Create a provider boundary rather than a second picker implementation.

Suggested provider split:

- `LocalTranscriptSessionProvider`
  - backs local `nori resume` and local `/resume`
  - reads `TranscriptLoader`
  - returns local transcript resume targets
- `CloudSessionProvider`
  - backs `nori cloud` startup selection and cloud `/resume`
  - asks handroll for session rows
  - returns opaque cloud resume targets

The picker row type should be generic enough for both local and cloud sessions:

- display name
- optional description or first message preview
- searchable text
- last activity timestamp
- optional source label
- opaque target payload

Keep provider-specific behavior outside the view. The view should not know whether selecting a row will load a local transcript or ask handroll to resume a cloud session.

## Command Gating In Cloud Mode

Cloud mode needs an explicit signal in the CLI/TUI path.

Do not rely only on `agent == "nori-cloud"` or the presence of `extra_agents` as the long-term signal. Those are useful facts, but policy should be driven by a deliberate `cloud_mode` flag or equivalent session mode value carried through `Cli`, `App`, and `ChatWidget`.

Classify every slash command exhaustively.

### Disable Local-Only Commands

These should be disabled in cloud mode unless a future remote implementation exists:

- `/init`
- `/browse`
- `/diff`
- `/mention`
- `/memory`
- `/mcp`
- `/browser`
- `/switch-skillset`
- `/resume-viewonly`

Settings/config surfaces should be audited item by item. The rule is not "all settings are disabled"; the rule is "local-only settings are disabled." Session-safe or cloud-backed settings may stay available if they operate through handroll or the active remote session.

### Adapt Instead Of Disable

These commands should become cloud-aware rather than permanently disabled:

- `/resume`
  - local mode: show local transcript sessions
  - cloud mode: show handroll-provided cloud sessions
- `/new`
  - local mode: start a new local ACP session
  - cloud mode: ask handroll to start a new cloud session
- `/status`
  - local mode: existing session status
  - cloud mode: include cloud/session source status when handroll exposes it

### Needs Product Decision

These commands need explicit product/API decisions before final classification:

- `/agent`
- `/model`
- `/config`
- `/approvals`
- `/login`
- `/logout`

If these map to remote session configuration, they can stay enabled. If they mutate local-only configuration or bypass the pinned cloud adapter, they should be disabled or constrained in cloud mode.

## Startup Flow

The preferred startup shape is:

1. CLI parses `nori cloud`.
2. CLI resolves `nori-handroll`.
3. CLI marks the TUI session as cloud mode.
4. CLI requests cloud session rows from handroll if the terminal is interactive and session selection is enabled.
5. CLI/TUI displays a picker using the normal TUI selection UI.
6. The selected target is passed to handroll as opaque data.
7. TUI launches the `nori-cloud` adapter.
8. Handroll owns acquire/resume and broker connection.

Non-interactive behavior should remain simple:

- skip the picker
- start a new cloud session unless a future explicit `--resume-cloud-session <id>` flag is provided
- report errors without requiring TUI interaction

## Handroll Interface Requirements

Handroll needs to expose a CLI/TUI-facing contract that keeps broker details private.

Minimum structured session row:

```text
id: opaque string
source: cli | slack | discord | unknown | future source string
status: active | idle | stopped | unknown
created_at: timestamp if available
last_active_at: timestamp if available
first_message_preview: optional string
title: optional string
workspace_hint: optional string
```

Minimum operations:

- list sessions
- start new session
- resume session by opaque id
- detach or release active session on shutdown

Error contract:

- unauthenticated
- broker unreachable
- session not found
- session not resumable
- unsupported broker version
- unknown error with display message

The CLI should display these errors but should not inspect broker HTTP status codes.

## Backwards Compatibility

- Existing local `nori resume` behavior must keep working.
- Existing local `/resume` behavior must keep using transcript metadata.
- Existing `nori cloud` without session selection must still start a cloud session.
- If handroll does not support session listing yet, `nori cloud` should fall back to starting a new cloud session with a concise warning.
- Existing `NORI_HANDROLL_BIN` and `NORI_BROKER_URL` behavior should be preserved.
- Do not re-add `nori-rs/acp/src/broker`.
- Do not re-add direct WebSocket transport for cloud sessions in `nori-rs/acp`.

## Edge Cases

- Handroll is missing from `PATH`.
- `NORI_HANDROLL_BIN` points at a missing file.
- Broker auth has expired.
- Broker is reachable but list/resume endpoints are unavailable.
- Session list is empty.
- Session list includes a source unknown to this CLI version.
- Session disappears between list and resume.
- Selected session is owned by a different source such as Slack or Discord.
- Current cloud session has pending work when the user picks `/resume`.
- TUI is non-interactive.
- The handroll list operation is slow.
- The cloud session provider returns malformed metadata.
- Local and cloud sessions share similar ids.
- User invokes local-only commands by typing them directly, bypassing the popup.

## Testing Plan

I will add behavior tests around the CLI cloud startup boundary that verify `nori cloud` still resolves and pins `nori-handroll cloud-acp`, and that cloud selection failures fall back to the documented behavior without the CLI speaking broker HTTP directly.

I will add TUI unit or snapshot coverage for cloud command gating. The tests should drive command availability through the same path used by the command popup and direct command dispatch, proving disabled local-only commands cannot be selected or typed in cloud mode.

I will add picker tests around a provider-neutral session row model. The tests should verify that local rows and cloud rows render through the same picker view while dispatching different target events.

I will add integration coverage for cloud `/resume` once the handroll contract exists. The test should fake the handroll-facing boundary, return realistic cloud rows, select one in the TUI, and assert that the selected opaque id is sent back to the cloud provider instead of loading a local transcript.

I will add an E2E test once handroll has a testable router mode. The test should run the real `nori` binary with a local fake or fixture-backed handroll command, open `nori cloud`, verify the picker appears, select an existing session, and confirm the TUI reaches an active prompt for that resumed session.

NOTE: I will write _all_ tests before I add any implementation behavior.

## Implementation Roadmap

### Phase 1: Retire The Old CLI Broker Direction

- Close the old PR.
- Preserve any useful tests or UX notes as references only.
- Do not port `BrokerClient`, `CloudSessionSummary`, or broker HTTP methods into `nori-rs/acp`.
- Keep `nori-rs/cli/src/cloud.rs` focused on handroll resolution and adapter configuration.

### Phase 2: Handroll Session Lifecycle

Handroll implements the lifecycle API behind its cloud ACP router:

- broker authentication
- session list
- new session
- resume selected session
- release or detach on shutdown
- normalized metadata
- structured errors

The CLI contract should be stable before TUI integration starts.

### Phase 3: CLI Cloud Mode Signal

Add an explicit cloud mode signal through the CLI/TUI initialization path.

Likely files:

- `nori-rs/tui/src/cli.rs`
- `nori-rs/cli/src/main.rs`
- `nori-rs/tui/src/lib.rs`
- `nori-rs/tui/src/app/mod.rs`
- `nori-rs/tui/src/chatwidget/mod.rs`
- `nori-rs/tui/src/chatwidget/constructors.rs`

This signal should be set by the `Cloud` subcommand and carried to the `ChatWidget`.

### Phase 4: Cloud Command Gating

Reuse existing command availability infrastructure.

Likely files:

- `nori-rs/tui/src/slash_command.rs`
- `nori-rs/tui/src/chatwidget/key_handling.rs`
- `nori-rs/tui/src/chatwidget/goal.rs`
- a new small module under `nori-rs/tui/src/chatwidget/`

Requirements:

- popup-level disabled state
- direct dispatch guard
- exhaustive command classification
- clear disabled reasons
- reapply local policy after backend capability updates

### Phase 5: Provider-Neutral Session Picker

Extract the picker view from the local transcript data source.

Likely files:

- `nori-rs/tui/src/nori/resume_session_picker.rs`
- `nori-rs/tui/src/resume_picker.rs`
- `nori-rs/tui/src/app_event.rs`
- `nori-rs/tui/src/app/event_handling.rs`

Keep the existing local transcript provider intact. Add a cloud provider once handroll exposes session rows.

### Phase 6: Startup Cloud Selection

Use the provider-neutral picker for `nori cloud` startup selection.

Constraints:

- interactive terminals can show the picker
- non-interactive terminals skip selection
- unsupported handroll list falls back to new session
- selected cloud session id remains opaque

### Phase 7: In-TUI Cloud Resume

Make `/resume` provider-aware:

- local mode uses transcript sessions
- cloud mode uses handroll cloud sessions

Selecting a cloud row should not call `TranscriptLoader`. It should ask handroll to resume or restart the cloud adapter with the selected target.

### Phase 8: Documentation And Manual Verification

Update docs after behavior lands:

- `nori-rs/cli/docs.md`
- `nori-rs/tui/docs.md`
- handroll docs in the relevant repo/package
- this spec's progress file if one is created later

Manual verification should cover:

- `nori cloud` starts new session
- `nori cloud` resumes CLI-origin session
- `nori cloud` resumes Slack-origin session if broker fixture supports it
- cloud `/resume` does not display local transcript-only sessions
- local `/resume` still displays local transcript sessions
- local-only commands are disabled in cloud mode

## Open Questions

1. What is the exact handroll-to-CLI contract for listing sessions: handroll subcommand, ACP extension, or router control request?
2. Should startup selection happen before the TUI launches, or inside an initialized TUI before the first ACP session starts?
3. Should cloud `/new` detach the current remote session or release it?
4. Which settings inside `/settings` or `/config` are local-only versus cloud-safe?
5. Should `/agent` and `/model` control remote session configuration, or should cloud mode keep the adapter fixed and expose remote configuration elsewhere?
6. Do Slack and Discord sessions need extra labels or permission checks before appearing in CLI?
7. Does handroll return enough history for the TUI to display resumed conversation state immediately, or does resume only affect future backend context?

**Testing Details** The main behavior tests will prove that cloud mode uses handroll as the lifecycle boundary, local-only commands are gated in both popup and direct dispatch paths, picker rendering is provider-neutral, cloud resume does not load local transcripts, and end-to-end `nori cloud` can resume a fixture-backed cloud session through the real binary.

**Implementation Details**

- Keep broker and WebSocket code out of `nori-rs/acp`.
- Use an explicit cloud mode signal rather than inferring policy from the agent slug.
- Reuse existing picker UI primitives.
- Split picker data providers from picker rendering.
- Treat cloud session ids as opaque strings.
- Preserve local transcript resume.
- Make command classification exhaustive.
- Prefer graceful fallback when handroll lacks list support.
- Keep non-interactive cloud startup simple.
- Update documentation once the behavior exists.

**Question** The main unresolved design choice is the handroll-to-CLI control contract for session listing and resume selection. Pick that first; the CLI work should adapt to it without learning broker internals.

---
