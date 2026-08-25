# ACP Connection and Session Lifecycle

Status: **accepted design**
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

1. Spawn the selected agent subprocess.
2. Complete ACP `initialize`.
3. Read capabilities from the initialized connection.
4. If advertised, issue and fully drain `session/list` on that connection.
5. Present the available choice without issuing a session directive.
6. After an explicit product decision, issue one of `session/new`,
   `session/load`, or `session/resume` on the same connection.
7. Only after session activation succeeds may that connection replace the
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
not a deferred agent name. Session-generation tags reject stale asynchronous
results.

No connection-management event belongs in `nori-protocol`. ACP initialize and
list responses remain raw ACP envelopes; candidate preparation is private TUI
orchestration. The existing Nori `SessionStarted` event is the successful
activation/commit boundary.

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

There is no "switch on next prompt" state. Prompt submission always targets
the active `HarnessHandle`.

## Startup and compatibility

Picker-first startup prepares an agent before rendering its session choices.
If listing is supported, an empty list and a non-empty list are both successful
results. If listing is unsupported, the product may retain its existing
compatibility policy, but any `session/new` remains a separate, explicit
harness transition rather than part of initialization.

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
the session-bound backend.

## Acceptance criteria

- One subprocess records `initialize -> session/list -> session/new|load|resume`
  for a prepared-and-activated agent.
- No session directive is recorded while the startup or switch picker waits
  for a choice.
- The current agent process remains alive throughout candidate preparation.
- Candidate failure leaves the current session promptable.
- Successful candidate activation reaps the replaced process only after
  `SessionStarted`.
- Cancelling or superseding a prepared candidate reaps only that candidate.
- Agents without `session/list` remain usable without conflating unsupported
  listing with an empty catalog.
- No new `nori-protocol` event is required.
