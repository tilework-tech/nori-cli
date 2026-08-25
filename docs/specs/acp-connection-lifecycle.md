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
- **Prepared** means Nori owns the initialized connection, has inspected its
  capabilities, and has attempted `session/list` when advertised. The same
  connection remains open while the user chooses what to do.
- **Session-active** means exactly one explicit session directive succeeded:
  `session/new`, `session/load`, or `session/resume`.
- **Abandoned** means a prepared candidate was cancelled, superseded, or
  dropped before it acquired a session. Nori closes its connection and reaps
  its subprocess.

Unsupported `session/list` is a valid prepared state, distinct from a
successful empty list and from connection/list failure.

## Required ordering

Startup and agent switching use the same ordering:

1. Resolve the selected agent and inject its persisted default model into the
   spawn configuration when that agent supports an out-of-band model channel.
2. Spawn the selected agent subprocess from that configuration.
3. Complete ACP `initialize`.
4. Read capabilities from the initialized connection.
5. If advertised, issue and fully drain `session/list` on that connection.
6. Present the available choice without issuing a session directive.
7. After an explicit product decision, issue one of `session/new`,
   `session/load`, or `session/resume` on the same connection.
8. Apply the live `session/set_config_option` default-model fallback when the
   activated session still requires it.
9. Only after session activation succeeds may that connection replace the
   current active agent.

The implementation must not use a disposable probe process followed by a
second session process. It must not call `session/new` merely because
`initialize` succeeded.

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

The TUI keeps startup preparation and switch candidates in `App`. Candidate
state is private to the TUI and uses existing session-generation tags to route
asynchronous results. Primary preparation additionally retains the task abort
handle so explicit new/resume, close, candidate preparation, and exit can
invalidate in-flight work. Its event vocabulary therefore changes only inside
`nori-tui`; `nori-protocol` remains unchanged. See
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

TUI candidate orchestration must not retain a separate full `NoriConfig`
snapshot. Before activation, it refreshes session-time configuration from
current `App` state. The agent identity, cwd, ACP proxy settings, and default
model are fixed by preparation because they determine the already-running
process or transport. Refresh rejects drift without consuming or changing the
prepared connection; the TUI shuts down that stale candidate and requires a new
preparation. A successful `SessionStarted` commits only the new active-agent
identity, preserving all other current application settings.

Authentication targeting is independent of candidate ownership. Bare `/login`
may temporarily target the selected or just-failed candidate through a private
login-only override, but that override cannot route prompts or resurrect a
cancelled switch. It is cleared on cancellation or successful authentication;
after a successful switch, login naturally targets the new active agent.

When the process-wide remote ACP host is active, candidate activation remains
hidden from it until the same commit event. The committed candidate replaces
the old remote attachment with the already-observed `SessionStarted` data, so
cancellation or failure leaves both the local current session and its remote
controller attachment intact. Candidate launch input remains deferred until
the replacement attachment attempt completes. A successful attachment installs
the subscription before the automatic first turn can outrun the remote
observer; attachment failure is logged and does not strand the launch input.

There is no "switch on next prompt" state. Prompt submission always targets
the active `HarnessHandle`.

## Startup and compatibility

Picker-first startup prepares an agent before rendering its session choices.
If listing is supported, an empty list and a non-empty list are both successful
results. If listing is unsupported, the product may retain its existing
compatibility policy, but any `session/new` remains a separate, explicit
harness transition rather than part of initialization.

An initial positional prompt and image attachments survive every deferred
replacement path. Ordinary new/resume replacement takes the input before
retiring the old widget, including when it first cancels an in-flight primary
preparation. Candidate new/resume activation instead copies the input because
the active widget remains the rollback target until candidate `SessionStarted`;
candidate failure therefore leaves the original input intact. Automatic
submission from the committed widget waits for `SessionStarted` and, for a
remote candidate, completion of the commit-time host attachment attempt.

Local-transcript resume is one of those ordinary replacements. `/resume` may
open the local fallback while primary ACP preparation is still discovering
capabilities. After loading the transcript, the handler cancels preparation,
takes the deferred input, clears deferred-spawn state, and rebuilds the widget
with any `PreparedAgent` that already completed. Removing the owned generation
first makes a later result stale, so it is shut down instead of reopening the
ACP picker. See
[`event_handling.rs`](../../nori-rs/tui/src/app/event_handling.rs) and
[`session_setup.rs`](../../nori-rs/tui/src/app/session_setup.rs).

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

Dropping a prepared agent, cancelling a candidate, quitting from a picker, or
losing the receiving UI must close the ACP connection and reap its subprocess.
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
- No session directive is recorded while the startup or switch picker waits
  for a choice.
- The current agent process remains alive throughout candidate preparation.
- Candidate failure, including preparation timeout, reaps the candidate and
  leaves the current session promptable.
- Successful candidate activation reaps the replaced process only after
  `SessionStarted`.
- Cancelling or superseding a prepared candidate reaps only that candidate.
- Cancelling or superseding an in-flight primary preparation rejects any stale
  result and reaps a child that has already spawned.
- Deferred prompt text and image attachments survive ordinary replacement,
  including cancelled primary preparation, and candidate new/resume failure
  leaves the rollback widget's copy intact.
- Selecting a local transcript while primary preparation is in flight cancels
  preparation, reaps any spawned child, transfers deferred input, and cannot
  install a late ACP picker.
- Agents without `session/list` remain usable without conflating unsupported
  listing with an empty catalog.
- An advertised `session/list` failure is a preparation failure and never
  authorizes a fallback `session/new`.
- Candidate failure or cancellation preserves the current process-wide remote
  host attachment; it changes only after candidate `SessionStarted`.
- No new `nori-protocol` event is required.
