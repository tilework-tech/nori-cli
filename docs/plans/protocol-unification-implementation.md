# ACP-Canonical Protocol Unification Implementation Plan

**Status:** Design commit `baade242` was implemented through `919dbb39`.
Implementation is complete. Planned test coverage is only partially complete;
the exact checked-in coverage and gaps are recorded below. Final branch
verification is complete; draft-PR handoff remains.

**Goal:** Delete `codex-protocol`, make `nori-protocol` the single ACP schema entry point, and expose an ACP-faithful event boundary from the embeddable Nori harness.

**Architecture:** `nori-protocol` re-exports the ACP schema and defines a source-nested `SessionEvent::{Acp, Nori}` boundary. Raw ACP notifications, requests, and responses cross that boundary unchanged; the small Nori branch covers only harness-owned lifecycle and product concerns, while commands and queries use typed harness methods.

**Tech Stack:** Rust workspace, Tokio channels/tasks, `agent-client-protocol` SDK, `agent-client-protocol-schema`, Serde JSONL transcripts, Cargo nextest/tests, mock ACP agent, Ratatui TUI

---

## Implementation result

The hard cut deleted `codex-protocol`, `codex-app-server-protocol`, the generic
`Event`/`EventMsg`/`Op` bus, and the ACP-to-Codex translator. `nori-protocol` is
now types-only and the sole schema dependency. It re-exports
`nori_protocol::acp` and owns `SessionEvent::{Acp, Nori}` with raw ACP
notification/request/response envelopes and exactly fifteen Nori outer event
variants.

`nori-harness` exposes the following typed method families on `HarnessHandle`:

```rust
prompt(Vec<acp::v1::ContentBlock>) -> Result<acp::v1::RequestId>
respond_to_agent(acp::v1::RequestId, Result<acp::v1::ClientResponse, acp::v1::Error>) -> Result<()>
cancel() -> Result<()>
shutdown() -> Result<()>

add_history(String) -> Result<()>
history_entry(i64, i64) -> Result<Option<HistoryEntry>>
search_history(i64) -> Result<Vec<HistoryEntry>>
custom_prompts() -> Result<Vec<CustomPrompt>>
compact() -> Result<()>
undo() -> Result<()>
undo_snapshots() -> Result<Vec<UndoSnapshot>>
undo_to(i64) -> Result<()>
run_user_shell(String) -> Result<()>
set_approval_policy(nori_config::AskForApproval) -> Result<()>

goal() -> Result<Option<nori_protocol::ThreadGoal>>
set_goal(String, Option<nori_protocol::ThreadGoalStatus>) -> Result<nori_protocol::ThreadGoal>
set_goal_status(nori_protocol::ThreadGoalStatus) -> Result<nori_protocol::ThreadGoal>
clear_goal() -> Result<()>
get_session_config() -> Option<Vec<acp::v1::SessionConfigOption>>
set_session_config_option(String, String) -> Result<Vec<acp::v1::SessionConfigOption>>
list_sessions(PathBuf) -> Result<Vec<acp::v1::SessionInfo>>
close_session() -> Result<()>
```

The final implementation consolidated public boundary coverage in
`nori-rs/harness/tests/session_event_boundary.rs`. Transcript schema v3 writes
explicit user records and exact `SessionEvent` records; v2 compatibility remains
private to transcript storage. The TUI performs source-first dispatch and keeps
its ACP presentation projection private.

The final correlation/lifecycle commits establish these additional invariants:
non-idle phase IDs are the exact ACP wire `RequestId`; one Harness prompt issues
one ACP prompt request; all current setup responses precede `SessionStarted` and
remain outside replay brackets; an empty successful `EndTurn` is terminal rather
than a retry signal; and explicit shutdown aborts the retained reducer and relay
tasks. Setup drains through the actual response rather than a fixed event count;
raw bootstrap errors are not mirrored as Nori failures; `Prompting` precedes
prompt-attributable ACP traffic; failed-load Agent and Transcript histories use
separate replay batches; and child loss retains the active wire request ID.

### Checked-in behavioral coverage

- [x] Bootstrap preserves initialize and `session/new` responses before
      `SessionStarted`, even when the consumer starts draining late.
- [x] Bootstrap preserves raw initialize, new-session, and resume error
      responses before `SpawnFailed` without a duplicate Nori failure event.
- [x] Interleaved setup notifications and the default-model config response
      precede `SessionStarted` in transport order.
- [x] Prompt notifications and response retain correlation, and the
      `Prompting` phase carries the same wire `RequestId` returned by `prompt()`.
- [x] `Prompting` is observable before the first prompt notification, response,
      or delegated permission request.
- [x] Delegated permission success and ACP error responses round-trip through
      the public boundary with the same request ID.
- [x] Typed session-list and close calls return directly while their ACP
      responses remain observable.
- [x] Successful `session/load` emits its response before `SessionStarted`,
      keeps that response outside replay brackets, and brackets the load-time ACP
      notifications.
- [x] Failed `session/load` and fallback `session/new` responses both precede
      `SessionStarted` in that order; partial Agent replay and local Transcript
      replay are emitted as separate source-owned batches.
- [x] Unexpected child loss correlates the fatal Nori failure with the active
      prompt's exact wire request ID.
- [x] V3 persistence writes user and exact `SessionEvent` records without a
      duplicate assistant or normalized-client record.
- [x] V3 replay projects explicit user input and preserves its order relative
      to stored ACP notifications.
- [x] A public v2 compatibility test covers legacy user, assistant, and
      thinking records without exposing the storage enum.
- [ ] No checked-in fixture covers every v2 top-level record and all 18 legacy
      normalized `ClientEvent` tags.
- [ ] No public-boundary test covers every Nori event variant or every typed
      query family listed in this plan.
- [ ] No dedicated black-box test proves host-handled filesystem requests are
      absent from the outward stream.
- [ ] No dedicated test sends an empty successful `EndTurn` or counts wire
      prompt requests to lock the one-call/one-request rule.
- [ ] No task-lifetime test directly proves explicit shutdown aborts both the
      retained reducer and relay tasks.
- [ ] The full request-race matrix and a separate headless example/test were
      not added.

The task bodies below are retained as the test-first execution checklist. Their
imperative language is historical, not a claim that every planned test landed.

## Historical testing plan

Add a black-box harness integration test that starts the real mock ACP agent,
submits a prompt, and observes raw ACP notifications followed by the correlated
ACP prompt response through `SessionEvent`. It must validate observable order,
request identity, stop reason, and content without inspecting reducer fields or
mocking the harness.

Add a black-box delegated-request integration test in which the mock agent asks
for permission. It must prove that the request reaches the consumer as a raw
`AgentRequest` and that a schema-native response with the same `RequestId`
reaches the agent. Preserve the current host-owned filesystem behavior and
prove it does not leak a duplicate outward request. Terminal and extension
request families are currently unadvertised; do not invent routing
configuration for them in this refactor.

Add transcript behavior tests that load a checked-in matrix covering every
version-2 top-level record and all 18 normalized `ClientEvent` tags through the
public transcript loader, replay a newly recorded session, and verify that
equivalent user-visible content and Nori-owned state are restored. New-format
tests must prove that client-to-agent user input survives independently of the
agent-to-client event stream and that assistant output has only one canonical
stored representation.

Add harness behavior tests for queue changes, replay brackets, lifecycle phase,
goals, undo, user shell, hook output, and transport failure. Each test should
exercise a real harness operation and assert only the public `SessionEvent`
sequence. Prompt/ACP failures must be observed as ACP responses; only failures
without an ACP response may be observed as `NoriEvent::RequestFailed`.

Add query-boundary tests that call history, prompt discovery, undo-list,
session-list, config, and close methods and receive their results directly.
They must also drain the event stream to prove the query result is not
duplicated as a correlated Nori event.

Keep the existing TUI PTY end-to-end suite as the frontend regression boundary
and add one minimal headless example/test that consumes the same public harness
API. Dependency and import checks supplement these behavioral tests but do not
replace them.

<!-- prettier-ignore -->
NOTE: I will write *all* tests before I add any implementation behavior.

For this phased plan, “all tests” means every test for the next behavioral
slice. Each task that introduces behavior begins by adding and observing its
own failing public-boundary tests before editing that behavior.

## Preconditions and constraints

- The active CLI configuration rework landed in #545. Implementation baseline:
  `45754205` on `origin/main`.
- The implementation worktree is
  `.worktrees/acp-protocol-unification` on
  `refactor/acp-protocol-unification`; do not edit the protected branch.
- Read the repository `AGENTS.md` and required skills before implementation.
- The user has approved removal of the `codex-protocol` dependency as a hard
  cut. Do not add, upgrade, or remove any other dependency without asking.
- The post-configuration audit is complete: 12 direct dependency edges, all 18
  `ClientEvent`, 20 `Op`, and 51 `EventMsg` variants, every schema/SDK import,
  and the version-2 persistence behavior are accounted for in the spec.
- The user approved the hard cut and instructed implementation to proceed; the
  refreshed ledger in the spec is the deletion gate for this branch.
- Do not add a compatibility facade, deprecated aliases, or a second normalized
  ACP vocabulary.
- Prefer one atomic implementation PR (with reviewable commits) so `main` never
  has two endorsed protocol boundaries.

The normative design and current deletion inventory are in
`docs/specs/protocol-unification.md`.

## Task 1: Refresh the post-configuration inventory and pass the deletion gate — complete

**Files to inspect:**

- `nori-rs/Cargo.toml`
- `nori-rs/Cargo.lock`
- every `nori-rs/*/Cargo.toml`
- `nori-rs/protocol/src/**/*.rs`
- `nori-rs/nori-protocol/src/**/*.rs`
- `nori-rs/acp-host/src/**/*.rs`
- `nori-rs/harness/src/**/*.rs`
- `nori-rs/tui/src/**/*.rs`
- configuration files and crates changed by the prerequisite rework

**Step 1: Establish the clean baseline — complete**

The branch starts at `45754205`. `cargo build --bin nori`, the CI-profile mock
agent build, the selected package suite, and the real PTY end-to-end suite pass
before implementation.

**Step 2: Re-run dependency and import audits**

Use Cargo metadata/tree plus source search to answer all of these questions:

```bash
cargo tree --manifest-path nori-rs/Cargo.toml -i codex-protocol
rg -n 'codex-protocol' nori-rs -g Cargo.toml
rg -n '\bcodex_protocol\b' nori-rs -g '*.rs'
rg -n 'agent-client-protocol-schema' nori-rs -g Cargo.toml
rg -n '\bagent_client_protocol_schema\b' nori-rs -g '*.rs'
rg -n 'agent-client-protocol' nori-rs -g Cargo.toml
rg -n '\bagent_client_protocol\b' nori-rs -g '*.rs'
```

List each direct dependency edge and each importing source file. Classify each
use as ACP agent/client semantics, Nori-owned domain, configuration/runtime
policy, presentation, persistence compatibility, or dead code.

**Step 3: Re-run producer/consumer audits**

For every `ClientEvent`, `Op`, and `EventMsg` variant in the normative spec,
identify:

- all production constructors/producers;
- all production matches/consumers;
- all persistence readers/writers;
- all tests and fixtures; and
- whether the configuration refactor added or removed any usage.

Do not classify a variant as dead solely because one frontend ignores it.

**Step 4: Resolve configuration ownership**

Identify the post-refactor homes of login, model, MCP, approval, sandbox, and
shell-environment types. In particular, decide the concrete owners of
`AskForApproval` and `SandboxPolicy` from actual runtime/API use. They must not
move into `nori-protocol` merely to avoid a cycle.

**Step 5: Resolve the two live Nori extension gaps — complete**

The smallest truthful additions are
`PromptSummaryUpdated(PromptSummary)` and `Notice(Notice)`. Do not overload
`RequestFailed`, `ContextCompacted`, or `HookOutput`.

**Step 6: Pass the deletion gate — complete**

The exact refreshed deletion/rehoming list is recorded in the spec. The user
approved the hard cut and explicitly authorized implementation after #545.

## Task 2: Lock the new public boundary with failing behavioral tests — partially complete

**Files:**

- Create: `nori-rs/harness/tests/session_event_boundary.rs`
- Modify: `nori-rs/harness/src/backend/tests/mod.rs`
- Modify: relevant split files under `nori-rs/harness/src/backend/tests/`
- Modify: `nori-rs/acp-host/src/connection/acp_connection_tests.rs`
- Modify: mock-agent scenarios in the post-refactor `mock-acp-agent` crate

**Step 1: Write the bootstrap buffering test — covered**

Construct the harness through its public API, then begin reading the event
stream. Assert initialization and session-creation responses are present once,
in request order, even when the constructor awaited bootstrap before returning.
Use a consumer that intentionally starts late to prove bootstrap neither drops
early responses nor deadlocks on an unavailable receiver.

**Step 2: Write the prompt-stream test — covered**

Start the real mock ACP subprocess through the public harness constructor. Send
a prompt through the handle and collect outward events until its response. The
assertion should have this shape:

```rust
assert!(matches!(
    events.last(),
    Some(SessionEvent::Acp(AcpEvent::Response {
        request_id,
        response: Ok(acp::v1::AgentResponse::PromptResponse(_)),
    })) if request_id == &prompt_request_id
));
```

Assert at least one preceding raw session notification carries the expected
assistant content. Do not assert private channel calls or reducer state.

**Step 3: Write delegated request round-trip tests — covered for permission**

Make the mock agent issue a permission request. Observe `AcpEvent::Request`,
respond through the public handle using the same `RequestId`, and assert the
agent proceeds. Include both a `ClientResponse` success and a schema-native
`Error` response. The current host auto-handles advertised filesystem requests
and does not advertise terminal/extension capabilities; test that truth rather
than adding a new policy surface.

**Step 4: Write the load-stream test — covered**

Have the mock agent emit representative session notifications during
`session/load` before returning its load response. Assert the public load method
returns a distinct `RequestId`, the notifications remain in source order, and
the final `AcpEvent::Response` carries that same ID and the ACP load response.

**Step 5: Write host-handled request tests — not covered at the public boundary**

Exercise the existing filesystem handler. Assert the agent receives a result
and no corresponding outward `AcpEvent::Request` is emitted. This catches
double dispatch and accidental leakage without inventing configuration.

**Step 6: Write error ownership tests — partially covered**

Cover:

- ACP error response → `AcpEvent::Response(Err(...))`;
- clean prompt completion/cancel → ACP prompt response;
- subprocess loss or transport failure before response →
  `NoriEvent::RequestFailed`; and
- orderly close versus unexpected connection loss → distinct
  `SessionEnded` reasons.

**Step 7: Write Nori-event ownership tests — partially covered**

Exercise real harness operations that cause session start/phase/end, queue,
replay, compaction, goal, capabilities, undo, user-shell, hook, prompt-summary,
and nonfatal-notice transitions. Assert their public `NoriEvent` order and
payload meaning, and assert that no Nori tool/message/plan/usage/mode/config/
permission mirror is emitted.

**Step 8: Write direct-query result tests — covered only for list and close**

Call history lookup/search, custom prompt discovery, undo listing, session
listing, config mutation, and close through the public handle. Assert each
caller receives its typed result without reading the event stream. Drain the
stream concurrently and assert no correlated Nori result event is emitted;
ACP-backed operations may still expose their raw `AcpEvent::Response`.

**Step 9: Write request-lifecycle race tests — partially covered**

Use deterministic mock-agent barriers—not sleeps—to cover:

- two same-content requests retaining distinct `RequestId` values;
- cancellation racing with the agent's final notification and response;
- subprocess disconnect while delegated requests are pending;
- a dropped public event receiver while the harness auto-handles a request; and
- a typed query completing concurrently with prompt stream events.

Assert terminal public outcomes: no response is delivered to the wrong request,
pending responders close once, auto-handled work does not stall, and query
results are neither lost nor duplicated as Nori events.

**Step 10: Run the tests and confirm the intended failure**

Run the narrowest package/test commands. Confirm failures occur because the
new public API/behavior is absent—not because fixtures, subprocess setup, or
test compilation are wrong.

## Task 3: Establish `nori-protocol` as the ACP schema choke point — complete

**Files:**

- Modify: `nori-rs/nori-protocol/Cargo.toml`
- Rewrite: `nori-rs/nori-protocol/src/lib.rs`
- Delete or move: `nori-rs/nori-protocol/src/session_runtime.rs`
- Modify: `nori-rs/Cargo.toml`

**Step 1: Re-export the schema**

Add the public re-export:

```rust
pub use agent_client_protocol_schema as acp;
```

Define the approved boundary without a normalized ACP mirror:

```rust
pub enum SessionEvent {
    Acp(AcpEvent),
    Nori(NoriEvent),
}

pub enum AcpEvent {
    Notification(acp::v1::AgentNotification),
    Request {
        request_id: acp::v1::RequestId,
        request: acp::v1::AgentRequest,
    },
    Response {
        request_id: acp::v1::RequestId,
        response: Result<acp::v1::AgentResponse, acp::v1::Error>,
    },
}
```

Add only the traits required by channel use, logging, persistence adapters, and
consumer tests. Do not derive a trait preemptively for symmetry.

**Step 2: Add the Nori-only branch**

Implement the approved first-pass outer variants exactly:

```rust
pub enum NoriEvent {
    SessionStarted(SessionStarted),
    SessionPhaseChanged(SessionPhase),
    SessionEnded(SessionEnded),
    QueueChanged(QueueSnapshot),
    ReplayStarted(ReplayStarted),
    ReplayFinished,
    ContextCompacted(ContextCompacted),
    GoalChanged(Option<ThreadGoal>),
    CapabilitiesChanged(NoriCapabilities),
    Undo(UndoEvent),
    UserShell(UserShellEvent),
    HookOutput(HookOutput),
    PromptSummaryUpdated(PromptSummary),
    Notice(Notice),
    RequestFailed(RequestFailure),
}
```

Do not infer any further variant from old Codex vocabulary.

Derive payload fields from audited public consumer needs. Keep the structs
small; do not place ACP capability, content, tool, plan, mode, config-option, or
usage fields in them.

**Step 3: Move implementation state out**

Move session phase machinery, active request state, open-message buffers,
queued prompt state, and reducer-owned persisted state into private modules
under `nori-rs/harness/src/backend/`. Delete `ClientEventNormalizer`; retain
only genuinely needed private reduction/presentation transformations in their
owner crates.

**Step 4: Run protocol and boundary tests**

Run `cargo test -p nori-protocol` and the failing harness boundary test. The
protocol crate should compile with minimal dependencies; behavior tests may
remain red until the relay is rewired in the next task.

## Task 4: Relay raw ACP envelopes through `nori-acp-host` and `nori-harness` — complete

**Files:**

- Modify: `nori-rs/acp-host/src/lib.rs`
- Modify: `nori-rs/acp-host/src/connection/acp_connection.rs`
- Modify: `nori-rs/acp-host/src/connection/mod.rs`
- Delete or shrink: `nori-rs/acp-host/src/translator.rs`
- Modify: `nori-rs/harness/src/backend/nori_client_context.rs`
- Modify: `nori-rs/harness/src/backend/nori_client_mcp.rs`
- Modify: `nori-rs/harness/src/backend/session.rs`
- Modify: `nori-rs/harness/src/backend/session_reducer.rs`
- Modify: `nori-rs/harness/src/backend/session_runtime_driver.rs`
- Modify: `nori-rs/harness/src/backend/spawn_and_relay.rs`
- Modify: `nori-rs/harness/src/backend/submit_and_ops.rs`
- Modify: `nori-rs/harness/src/runtime.rs`

**Step 1: Change connection output to raw schema envelopes**

At the point where the SDK supplies an agent notification, agent request, or
agent response, construct the matching `AcpEvent` while retaining its
`RequestId`. Delete translation into `codex_protocol::EventMsg` and normalized
`ClientEvent`.

**Step 2: Preserve request routing policy**

Keep the current host-owned filesystem handling and outward permission
delegation. Delegated requests go to the public stream once. Public response
methods route schema-native client success or error responses back to the
pending ACP request. Cancellation and disconnect must close pending responders
deterministically. Leave unadvertised terminal/extension families unadvertised.

**Step 3: Emit the small Nori branch**

Emit lifecycle, queue, replay brackets, compaction, goals, capabilities, undo,
user shell, hooks, the approved prompt-summary/nonfatal-notice resolution, and
no-response failures at their actual owning transitions. Do not infer Nori
duplicates from every ACP update.

**Step 4: Replace the generic operation bus with typed methods**

Replace `Op` submission sites with explicit handle methods. Return history,
custom prompts, undo snapshots, session list, config mutation, and close results
directly to their callers. Keep streamed progress on `SessionEvent` only where
it is genuinely asynchronous session output; retain raw ACP responses for
schema-complete observability without adding Nori result events.

**Step 5: Make boundary tests pass**

Run the tests from Task 2. Inspect complete output and fix the owning layer for
ordering, request-correlation, or lifecycle failures; do not normalize away
schema differences as a workaround.

## Task 5: Version transcript persistence without preserving the old Rust API — implementation complete, planned matrix incomplete

**Files:**

- Modify: `nori-rs/harness/src/transcript/types.rs`
- Modify: `nori-rs/harness/src/transcript/loader.rs`
- Modify: `nori-rs/harness/src/transcript/recorder.rs`
- Modify: `nori-rs/harness/src/transcript/project.rs`
- Modify: `nori-rs/harness/src/transcript/tests.rs`
- Add: compact private legacy-v2 decode module under
  `nori-rs/harness/src/transcript/`
- Add or update: versioned JSONL fixtures under
  `nori-rs/harness/tests/fixtures/`

**Step 1: Write legacy-load and new-round-trip tests — partially covered**

Load version-2 records containing every top-level record kind and all 18
normalized `ClientEvent` tags through the public loader. The repository had no
fixture covering that matrix, so add it before changing the loader. Record the
ordered public message, plan/tool update, Nori state update, documented drop,
warning, or load error each value must produce. Lock the current public behavior
for unknown fields, malformed JSONL records, and now-obsolete variants unless
the deletion review explicitly approves a change.

Record/reload a new session and assert the exact outward sequence:
`ReplayStarted`, only ACP notifications in source order, then
`ReplayFinished`. Explicit user records are projected to ACP user-message
notifications at their original positions. Assert
restored goal and session metadata through their public query/event boundaries
rather than inspecting the decoder's state. Before changing persistence
behavior, add and observe failing tests proving user input is not lost,
assistant output is not stored twice, and replay cannot repeat a recorded
filesystem side effect or complete a live request from a historical response.

**Step 2: Define a persistence-owned version — complete**

Increment the transcript schema. Store persistence-owned user input, raw ACP
envelopes, and Nori events in an envelope that can redact or omit secrets,
attachments, and ephemeral request channels. Do not require the public enums
themselves to become a permanent storage ABI. Preserve request/response IDs for
a typed offline transcript reader without making those historical envelopes
replayable. Do not also store an assistant projection beside its raw ACP
notification.

**Step 3: Keep version-2 compatibility private — complete**

Keep only the legacy shapes needed to decode user-owned records. Translate them
into the private replay projections. Do not export storage or legacy types,
implement a runtime compatibility facade, or write new version-2 records. The
implementation keeps these shapes in the private transcript types/loader rather
than adding the separately named decoder module proposed above.

**Step 4: Preserve replay ordering — covered for v3 user/notification order and agent load notifications**

Emit `ReplayStarted`, historical ACP notifications in original order, and
`ReplayFinished`. Project persisted user inputs to ACP user-message
notifications. Do not place current setup responses or Nori lifecycle events
inside the brackets, re-emit historical delegated requests, or deliver
historical responses to live pending calls. The planned side-effect and
live-completion cases do not have dedicated public-boundary tests.

**Step 5: Run transcript and harness tests**

Run the transcript test module, harness package tests, and the new black-box
boundary test. Verify fixtures contain no credentials or machine-specific
paths before committing them.

## Task 6: Migrate the TUI and headless consumer without recreating protocol mirrors — implementation complete, planned headless test absent

**Files:**

- Modify: `nori-rs/tui/Cargo.toml`
- Modify: current event dispatch files under `nori-rs/tui/src/nori/`
- Modify: current event handlers under `nori-rs/tui/src/chatwidget/`
- Modify: tool/message/plan presentation modules under `nori-rs/tui/src/`
- Modify: `nori-rs/cli/Cargo.toml`
- Modify: relevant `nori-rs/cli/src/*.rs` harness entry points
- Add or update: a headless harness example/integration test in the harness or
  CLI crate

**Step 1: Write failing frontend and headless boundary tests — TUI coverage exists; separate headless example/test absent**

Add the smallest TUI regression cases that exercise representative raw ACP
message, plan, tool, permission, and completion events plus Nori lifecycle and
query behavior. Add the headless integration test that sends a prompt, consumes
both source branches, answers one delegated request, and awaits one direct
query. Run them against the old dispatch and confirm they fail for the expected
missing-boundary reason.

**Step 2: Change the top-level TUI dispatch**

Match `SessionEvent::Acp` and `SessionEvent::Nori` first. Within `AcpEvent`,
handle the ACP aggregate variants directly. Within `NoriEvent`, handle only
Nori-owned lifecycle/product behavior.

**Step 3: Keep projections private to presentation**

Move tool titles, icons, invocation summaries, diff rendering, message chunk
assembly for widgets, and mode/config labels into private TUI view models. They
must not be exported by `nori-protocol` or fed back into the harness.

**Step 4: Convert query call sites**

Replace event-response waiting for history, prompts, undo lists, session lists,
config mutation, and close with awaited typed method results. Keep UI request
IDs local only when the UI itself needs stale-result suppression.

**Step 5: Exercise both consumers**

Run focused TUI unit/snapshot tests, then the PTY end-to-end suite. Run the
headless integration/example against the same harness API to catch accidental
terminal dependencies.

## Task 7: Rehome residual Codex types and delete `codex-protocol` — complete

**Files:**

- Delete: `nori-rs/protocol/`
- Modify: `nori-rs/Cargo.toml` and `nori-rs/Cargo.lock`
- Modify: Cargo manifests and imports in every refreshed reverse dependency
- Modify or delete: residual type owners identified in Task 1

The refreshed 12 reverse dependencies are `codex-app-server-protocol`,
`codex-common`, `codex-core`, `codex-linux-sandbox`, `codex-rmcp-client`,
`codex-sandbox`, `codex-windows-sandbox`, `nori-acp-host`, `nori-cli`,
`nori-config`, `nori-harness`, and `nori-tui`.

**Step 1: Rehome only live non-protocol values**

Move approval, trust, MCP server, resolved sandbox, and shell-environment policy
to `nori-config`; computed MCP authentication status to `codex-rmcp-client`;
UI formatting to the frontend; private parser helpers to the
harness/frontend; and persistence compatibility to the transcript loader.
Localize residual model/login/app-server values to their real private owners or
delete them. Prefer deletion over a new shared crate when a type has one
consumer.

**Step 2: Delete Codex/OpenAI model vocabulary**

Delete Responses API items, legacy rollout events, old content/plan/tool/event
mirrors, and no-op variants after the approved deletion audit. Localize any
genuinely live app-server-only value to that crate rather than exporting it
through Nori protocol.

**Step 3: Remove every dependency edge and the crate**

Remove `codex-protocol` from manifests and the lockfile, delete
`nori-rs/protocol/`, and update imports. Do not leave an empty shell crate or a
temporary alias.

**Step 4: Enforce schema imports**

Remove every direct `agent-client-protocol-schema` dependency except
`nori-protocol`. Keep the higher-level `agent-client-protocol` SDK dependency
only in `nori-acp-host` among client-side product crates. Agent implementations
and conformance fixtures such as `mock-acp-agent` may depend on the SDK directly
on the agent side. Import schema types through `nori_protocol::acp` even inside
the host.

**Step 5: Verify the hard cut**

```bash
cargo tree --manifest-path nori-rs/Cargo.toml -i codex-protocol
rg -n 'codex-protocol' nori-rs -g Cargo.toml
rg -n '\bcodex_protocol\b' nori-rs -g '*.rs'
rg -n 'agent-client-protocol-schema' nori-rs -g Cargo.toml
rg -n '\bagent_client_protocol_schema\b' nori-rs -g '*.rs'
rg -n 'agent-client-protocol' nori-rs -g Cargo.toml
rg -n '\bagent_client_protocol\b' nori-rs -g '*.rs'
rg -n 'agent_client_protocol::schema' nori-rs -g '*.rs'
```

The Codex searches must find nothing. Schema manifest matches are allowed only
for the workspace dependency declaration and `nori-protocol`; the sole schema
source import is its public re-export. SDK manifest/source matches are allowed
only for the workspace declaration, `nori-acp-host`, and explicitly agent-side
or conformance-test crates. The final `::schema` search must find nothing,
because even SDK consumers import schema values through `nori_protocol::acp`.

## Task 8: Update architecture and protocol documentation — complete

**Files:**

- Modify: `docs/specs/protocol-unification.md`
- Modify: `docs/specs/crate-layering.md`
- Modify: `docs/specs/acp-tui/*.md`
- Modify: `nori-rs/docs.md`
- Modify: `nori-rs/nori-protocol/docs.md`
- Modify: `nori-rs/acp-host/docs.md`
- Modify: `nori-rs/harness/docs.md`
- Modify: component `docs.md` files in changed source directories
- Modify: public README/headless embedding documentation where applicable

**Step 1: Convert design claims to implemented facts**

Update dependency diagrams, current-state descriptions, transcript version,
public examples, and ownership rules. Remove claims that `Event`/`EventMsg`/`Op`
are Nori's internal control plane or that two protocol vocabularies are
deliberate.

**Step 2: Document embedding and request delegation**

Show a minimal Rust consumer matching the two source branches, sending a prompt,
responding to a delegated ACP permission request, and awaiting a typed Nori
query. Explain that filesystem requests are currently host-handled and terminal
or extension requests are not advertised.

**Step 3: Run documentation consistency searches**

Search documentation for stale crate names, normalized `ClientEvent`, Codex
event bus terminology, direct ACP schema imports, and transcript version claims.
Update only statements invalidated by this implementation.

## Task 9: Final verification complete; branch handoff next

The implementation, focused black-box boundary coverage, documentation
consistency audit, and repository-wide verification are complete. The final
commands and results were:

- `cargo test --workspace --all-targets --no-fail-fast --quiet` — passed with
  no failures; 11 tests were ignored. This includes the black-box harness
  boundary, transcript compatibility, mock-agent, TUI, and PTY targets.
- `just fmt` — passed. Stable Rust emitted the repository's existing warning
  that `imports_granularity = Item` is nightly-only.
- `just fix -- -D warnings` — passed and changed no Rust source.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` —
  passed.
- `cargo check --workspace --all-targets --all-features` — passed.
- `cargo build --bin nori --bin mock_acp_agent` — passed.
- Changed Markdown files passed targeted Prettier formatting and
  `git diff --check`.
- Final dependency/import audits found no `codex-protocol` manifest edge, no
  `codex_protocol` source import, one schema source import (the public
  `nori-protocol` re-export), and direct ACP SDK source imports only in
  `nori-acp-host` and the agent-side mock.

**Step 1: Run formatting and static checks — complete**

Run the repository-prescribed Rust format, lint, and dependency checks. Inspect
all output; do not ignore unrelated failures. Root-cause and fix every CI issue
as required by `AGENTS.md`.

**Step 2: Run package and workspace tests — complete**

At minimum, run:

- `nori-protocol`, `nori-acp-host`, `nori-harness`, `nori-tui`, `nori-cli`, and
  every rehomed support crate's tests;
- the new black-box harness boundary and transcript compatibility tests;
- the mock ACP agent/conformance tests; and
- `cargo build --bin nori` plus `cargo test -p tui-pty-e2e`.

Use the repository's canonical commands if the configuration refactor changes
these package names or check entry points.

**Step 3: Review the diff for simplification — complete**

Confirm the implementation is net-negative where expected, no compatibility
facade remains, no public projection duplicates ACP, and no business logic sits
in `nori-protocol`.

**Step 4: Complete branch workflow**

Follow the finishing-development and documentation skills, update the PR with
the refreshed deletion inventory and exact verification results, and request
review only after the full suite is green.

## Backward compatibility notes

- Rust imports and enum matches intentionally break. The hard cut is approved;
  downstream embedders migrate to `SessionEvent`, raw ACP aggregates, and typed
  handle methods.
- There is no deprecation window for `codex_protocol`, `ClientEvent`, `Op`, or
  `EventMsg`.
- Existing user transcripts remain readable through a private version-2
  decoder. New writes use the new schema only.
- A headless JSON-RPC surface may version and correlate its own wire messages,
  but those concerns do not alter the Rust event API.
- ACP schema version changes are centralized at `nori-protocol`; this plan does
  not itself authorize an ACP dependency upgrade.

## Edge cases to cover

- an agent sends a notification before the corresponding prompt/load response;
- two requests have identical content but different ACP `RequestId` values;
- cancellation races with a final notification or response;
- the subprocess exits with delegated agent requests still awaiting responses;
- a frontend drops the event receiver while the harness is auto-handling a
  filesystem/terminal request;
- replay is empty, interrupted, or contains a version-2 event that no longer
  has a public equivalent;
- a legacy transcript contains unknown future fields or malformed records;
- an ACP extension aggregate is unknown to the current frontend but valid to
  the schema;
- queue and phase changes occur while replay is in progress;
- goals are cleared (`None`) while a prompt is active;
- a query completes concurrently with streamed events and produces no duplicate
  event response;
- config capabilities change after initialization; ACP and Nori capability
  sources must remain separate; and
- transport loss is not misreported as an ACP error response.

**Testing Details** Add real mock-agent/harness boundary tests for raw ACP ordering and request correlation, public-operation tests for Nori events and typed queries, transcript loader/replay tests for version-2 compatibility and the new write format, and TUI/headless end-to-end regressions. Tests observe public behavior and real channels/subprocesses, not private state, enum serialization snapshots, or mocked call counts.

**Implementation Details**

- Build from the landed #545 configuration baseline.
- Use the refreshed and approved exact deletion inventory.
- Re-export ACP schema only from `nori-protocol`.
- Preserve raw ACP notification/request/response aggregates and request IDs.
- Keep `NoriEvent` notification-only and outside ACP semantics.
- Return query results through typed harness methods.
- Move reducers and projections to their actual private owners.
- Read old transcripts privately; write only the new version.
- Remove every `codex-protocol` edge and delete the crate atomically.
- Verify both TUI and headless embedders against the same harness API.

**Resolved design gate** `nori-config` owns approval/sandbox policy; the
transcript domain owns Nori's persisted conversation identity; public payloads
are minimized against real consumers; and `PromptSummaryUpdated` plus `Notice`
close the two audited Nori extension gaps. No remaining design question reopens
the approved ACP/Nori boundary.

---
