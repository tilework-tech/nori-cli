# ACP Connection and Session Lifecycle

Status: **implemented**
Created: 2026-08-25

This specification defines how Nori owns ACP agent processes, initialized
connections, and sessions. It complements `crate-layering.md`: the ACP host
owns the wire connection, the harness composes it into session behavior, and
the TUI chooses which prepared connection becomes active.

The Agent Client Protocol does not require a session to exist after the
`initialize` handshake. Capabilities belong to the initialized agent
connection, and `session/list` discovers sessions without restoring or
creating one. Nori must preserve those distinctions in its runtime types and
subprocess lifetime.

## Lifecycle states

An agent process moves through these states:

```text
spawned -> initialized -> prepared -> session-active -> closed
                         |              |
                         +-> abandoned  +-> detached/shutdown
```

- **Initialized** means the ACP `initialize` handshake completed. No ACP
  session is implied.
- **Prepared** means Nori owns the initialized connection and has inspected its
  capabilities. It has attempted `session/list` when advertised unless the
  recognized Nori remote-control marker already selects the active session.
  The same connection remains open until activation.
- **Session-active** means exactly one explicit session directive succeeded:
  `session/new`, `session/load`, or `session/resume`.
- **Abandoned** means a prepared candidate was cancelled, superseded, or
  dropped before it acquired a session. Nori closes its connection and reaps
  its subprocess.

Unsupported `session/list` is a valid prepared state, distinct from a
successful empty list and from connection/list failure.

## Required ordering

Every subprocess agent startup, including a registered
`nori-handroll acp --type remote` adapter, uses this ordering:

1. Resolve the selected agent and inject its persisted default model into the
   spawn configuration when that agent supports an out-of-band model channel.
2. Spawn the selected agent subprocess from that configuration.
3. Complete ACP `initialize`.
4. Read capabilities from the initialized connection.
5. If advertised, issue and fully drain `session/list` on that connection,
   except for the Nori remote-control automatic-attachment rule below.
6. Unless automatic attachment applies, retain the prepared connection without
   issuing a session directive. The composer remains usable while advertised
   listing completes in the background.
7. After an explicit product decision or recognized automatic selection,
   issue one of `session/new`, `session/load`, or `session/resume` on the same
   connection.
8. Only after session activation succeeds may that connection replace the
   current active agent.

The implementation must not use a disposable probe process followed by a
second session process. It must not call `session/new` merely because
`initialize` succeeded.

### Nori remote-control automatic attachment

Only Nori's remote-control agent surface may advertise:

```json
{
  "nori": {
    "remoteControl": {
      "version": 1,
      "activeSessionId": "<stable outward Nori conversation ID>"
    }
  }
}
```

This object lives in `InitializeResponse._meta`. The version marker remains
present when no session is active, but `activeSessionId` is omitted.

When version 1 is well formed, has a non-empty active ID, and the agent also
advertises `loadSession`, preparation skips `session/list`. Activation
immediately issues `session/load` for that ID on the same connection, without
a picker or `session/new`. A load error is terminal and leaves the client
unattached. Missing, malformed, unsupported, or non-loadable markers use the
ordinary lifecycle unchanged.

## Ownership and crate boundaries

`nori-acp-host` continues to expose `AcpConnection` as the low-level owner of
the ACP transport and subprocess. It already supports capabilities,
`session/list`, and session directives independently; it does not choose when
a session should exist.

`nori-harness` owns an opaque prepared-agent value containing the unique
`AcpConnection`, its ordered connection-event receiver, resolved backend
configuration, and session-list result. The value is intentionally not
cloneable. Activating a session consumes it and constructs the existing
session-bound `AcpBackend`; dropping it tears down the unused process.

`AcpBackend` and `HarnessHandle` remain session-bound. They must not gain an
optional session ID or expose prompt/config operations before a session is
active. This keeps invalid pre-session operations unrepresentable.

`nori-tui` owns at most one current active session and one candidate agent.
The candidate represents a real connecting, prepared, or activating process,
not a deferred agent name. Primary and candidate preparation tasks each carry
a session generation and cancellation handle. Session-generation tags reject
stale asynchronous results, and a late successful result must be shut down
rather than installed.

No connection-management event belongs in `nori-protocol`. ACP initialize and
list responses remain raw ACP envelopes; candidate preparation is private TUI
orchestration. The existing Nori `SessionStarted` event is the successful
activation/commit boundary.

## Implementation map

The harness runtime exposes three explicit boundaries:

| Boundary | Type or operation | Responsibility |
| --- | --- | --- |
| Prepare | `AgentPrepareSpec` -> `prepare_agent` -> `PreparedAgent` | Resolve backend state, initialize one connection, inspect capabilities, and optionally list sessions without creating one. |
| Refresh | `refresh_prepared_agent(&mut PreparedAgent, AgentPrepareSpec)` | Apply current session-time config before activation while rejecting changes to the prepared agent identity, cwd, ACP proxy settings, or default model. |
| Choose | `SessionCatalog` and `SessionStart` | Preserve unsupported-versus-listed catalog state and represent the product's explicit new/resume decision. |
| Activate | `SessionLaunchSpec` -> `launch_session` -> `LaunchedSession` | Consume the unique prepared owner and create the session-bound backend, typed handle, and ordered event stream. |

Headless callers that already chose a directive use
`prepare_and_launch_session`; it combines the calls without collapsing their
ownership boundary or respawning the child. Preparation remains inside the
harness runtime's bounded warning/abort and shutdown race rather than being
awaited externally before a controllable handle exists. The implementation is
rooted in
[`runtime.rs`](../../nori-rs/harness/src/runtime.rs) and
[`prepared.rs`](../../nori-rs/harness/src/backend/prepared.rs).

The TUI keeps primary startup preparation and switch candidates in `App`.
Primary state consists of the preparation generation, task abort handle,
current preparation intent, retained fork context, and any pending New or
Resume activation. `/new`, the first genuine user prompt, `/resume`, and
backtrack/fork update that state and consume the same connection when it becomes
ready; they do not cancel and respawn a valid preparation. Session-generation
tags reject stale asynchronous results after close, exit, failure, or
replacement. Candidate state is private to the TUI and keeps separate ownership
until its activation commits. This event vocabulary remains internal to
`nori-tui`; `nori-protocol` is unchanged. See
[`session_setup.rs`](../../nori-rs/tui/src/app/session_setup.rs) and
[`event_handling.rs`](../../nori-rs/tui/src/app/event_handling.rs).

## Switching behavior

Selecting another agent starts candidate preparation immediately while the
current process and session remain usable. The current agent is not shut down
when the picker selection is made, while the candidate initializes, or while
its session list is displayed.

Choosing New or an existing session starts that session on the candidate's
already initialized connection. The switch commits only after the candidate
publishes `SessionStarted`. A preparation or activation failure destroys the
candidate and leaves the current agent usable. Superseding one candidate with
another destroys only the older candidate.

Primary and switch-candidate preparation share the TUI's 20-second wall-clock
bound. Expiration is handled as preparation failure: the in-flight future is
dropped so connection ownership reaps any spawned child, while an existing
active session remains promptable.

TUI preparation must not retain a separate full `NoriConfig` snapshot. Before
every primary or candidate activation, it refreshes session-time configuration
from current `App` state, including mutable approval and sandbox policy. Agent
identity, cwd, ACP proxy settings, and default model are fixed by preparation
because they determine the already-running process or transport. A primary
identity mismatch reaps the stale prepared child and restarts preparation while
retaining its pending directive and fork context; a candidate mismatch destroys
the candidate and requires another switch attempt. A successful
`SessionStarted` commits only the new active-agent identity, preserving all
other current application settings.

Authentication targeting is independent of candidate ownership. Bare `/login`
may temporarily target the selected or just-failed candidate through a private
login-only override, but that override cannot route prompts or resurrect a
cancelled switch. It is cleared on cancellation or successful authentication;
after a successful switch, login naturally targets the new active agent.

The TUI's app-owned remote ACP host follows the same commit boundary whether
its listeners are enabled or disabled. Candidate activation remains hidden
from it until `SessionStarted`; then the committed candidate replaces the old
attachment using the already-observed start data. Cancellation or failure
therefore leaves both the local current session and its remote attachment
intact.

There is no "switch on next prompt" state. Prompt submission always targets
the active `HarnessHandle`.

## Startup and compatibility

Ordinary startup prepares one agent immediately and leaves the frontend
sessionless. Preparation may advertise and run `session/list`, but listing does
not block typing and does not activate a session. If listing is supported, an
empty list and a non-empty list are both successful results. Unsupported
listing remains distinct from either result.

Sessionless user activation has three entry paths:

- `/new` records a pending New decision and issues `session/new` when the
  current preparation is ready.
- The first genuine user prompt without an active session records the same New
  decision. Its text and image attachments remain owned by the deferred widget,
  transfer to the activated widget, and are submitted exactly once after the
  session-configured (`SessionStarted`) boundary.
- `/resume` uses the catalog gathered during preparation when the agent can
  load or resume listed sessions. Selecting a row consumes the prepared
  connection through the existing load, live-resume, or transcript-replay
  policy. Agents without those catalog capabilities open the local transcript
  picker without first creating a session; selecting a transcript then follows
  the existing replay policy.

Initial positional prompts and image attachments follow the same deferred New
path as typed prompts. Slash commands remain local while sessionless; harness
commands such as `!cmd` report that no harness session is active. Neither path
implies `session/new`. Per-session skillset selection happens before preparation
so the initialized child observes the chosen workspace state.

Esc-Esc backtrack and transcript fork are also deferred New transitions. They
prepare before activation and retain the selected fork summary through both
preparation and the final configuration refresh.

If ordinary-agent preparation fails before activation, the TUI remains
sessionless and reopens the existing agent picker for recovery. Cloud keeps its
sessionless retry flow because its selected facade is not replaced through the
local agent picker. A primary failure cannot replace a live candidate's picker;
candidate ownership remains authoritative until it commits or is discarded.

Onboarding may automatically choose a broker-tagged onboarding session or the
documented compatibility fallback. That product decision happens after
preparation and reuses the prepared connection.

## Ordered events and teardown

The prepared owner must continuously account for the connection's ordered
event receiver while capability/list requests run. Paginated list responses
must not fill the bounded connection channel, and leftover list responses must
not be mistaken for the later session-directive response. Raw ACP setup events
that belong on the public session stream remain ordered ahead of
`SessionStarted`; inspection-only traffic may be consumed privately.

Dropping a prepared agent, cancelling a candidate, quitting from a picker,
timing out preparation, or losing the receiving UI must close the ACP
connection and reap its subprocess.
Once activation consumes the prepared value, teardown responsibility moves to
the session-bound backend. Explicit prepared shutdown gives stdin EOF a bounded
250 ms pre-session cleanup grace before forced process-group cleanup. Aborting
initialization is safe before a `PreparedAgent` exists because `AcpConnection`
installs a child cancellation guard immediately after transferring the child
to its watcher; dropping that guard kills and reaps the partially initialized
process.

## Acceptance criteria

- One subprocess records `initialize -> session/list -> session/new|load|resume`
  for a prepared-and-activated agent.
- Startup without input records no session directive, for ordinary subprocess
  agents and the Handroll remote adapter alike.
- `/new`, `/resume`, and the first prompt reuse an in-flight or completed
  primary preparation instead of spawning or initializing again.
- Primary activation refreshes mutable policy; identity mismatch reaps and
  reprepares while retaining pending activation and fork context.
- Backtrack/fork preserves its context across deferred preparation and New.
- A deferred text-and-image prompt is submitted once, unmodified, only after
  session activation commits.
- Slash commands and local shell commands do not implicitly activate a
  session.
- The current agent process remains alive throughout candidate preparation.
- Candidate failure, including preparation timeout, reaps the candidate and
  leaves the current session promptable.
- Successful candidate activation reaps the replaced process only after
  `SessionStarted`.
- Cancelling or superseding a prepared candidate reaps only that candidate.
- Cancelling or superseding an in-flight primary preparation rejects any stale
  result and reaps a child that has already spawned.
- Primary preparation timeout and application exit reap the sessionless child.
- Agents without `session/list` remain usable without conflating unsupported
  listing with an empty catalog.
- An advertised `session/list` failure is a preparation failure and never
  authorizes a fallback `session/new`.
- Candidate failure or cancellation preserves the current app-owned remote
  host attachment; it changes only after candidate `SessionStarted`.
- No new `nori-protocol` event is required.
