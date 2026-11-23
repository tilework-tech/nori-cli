# Glossary

Use these terms in code, docs, and reviews. Each term links to the guide that
owns its behavior. "Aliases to avoid" lists words that blur the boundary the
term names.

## Actors and Protocol Boundaries

### Actors

| Term | Definition | Aliases to avoid |
| --- | --- | --- |
| **[User](turn-lifecycle.md)** | The person whose intent and authorization a **client** mediates during agent work. | Client, account, ACP peer |
| **[Client](turn-lifecycle.md)** | An ACP client, such as Nori CLI or webchat, that mediates between a **user** and an **agent**. | Harness, peer, observer |
| **[Agent](turn-lifecycle.md)** | An ACP server that performs work and emits requests, responses, and session updates. | Model, provider, backend |
| **[Provider](turn-lifecycle.md)** | The external organization or service from which an **agent** obtains models or credentials. | Agent, model, ACP server |
| **[Token provider](../nori-rs/config.md#named-api-token-providers)** | A **provider** endpoint that Nori configures an **agent** to call, authenticated by a key the user keeps in a named environment variable. | Agent, preset, model |

### Nori runtime boundaries

| Term | Definition | Aliases to avoid |
| --- | --- | --- |
| **[ACP host](remote-control.md)** | The `nori-rs/acp-host` crate layer: owns the ACP SDK connection, agent process, wire lifecycle, and delegated-request routing, and serves ACP outward for the **host**. | Host, client, harness |
| **[Session harness](turn-lifecycle.md)** | Nori's headless runtime that composes the **ACP host** with product lifecycle and one ordered event stream. | ACP host, client, session broker |
| **[Session manager](remote-control.md)** | Nori's registry of live **sessions** keyed by **conversation ID**, running one **session harness** and agent process per session. It runs inside the TUI and the **host**. | Broker, session broker, harness |
| **[Host](remote-control.md#11-background-host-and-attached-terminals)** | The `nori host` process: a **session manager** that serves standard ACP v1 over `/acp` (alias `/sessions`) to attached clients and outlives any terminal. `nori host --stdio` (alias `nori exec --acp`) serves one client on stdin and stdout, joining the running host when it can, and one-shot `nori exec` runs as a client of a host. | ACP host, server, broker |
| **[Session broker](remote-control.md)** | The Nori Sessions service that shares a **session**, acting as agent downstream and client toward the upstream agent. Not part of this repository. | Host, provider, transport |

### Connections

| Term | Definition | Aliases to avoid |
| --- | --- | --- |
| **[Client connection](turn-lifecycle.md)** | One **client's** protocol relationship to a **session**, and the perspective from which ownership is classified. | Transport, socket, observer |
| **[Transport](remote-control.md)** | The channel that carries ACP JSON-RPC messages between a **client** and an **agent** without defining their meaning. | Protocol, connection, session |

- A **user** acts through a **client** and is not an ACP protocol participant.
- The **ACP host**, **session harness**, **session manager**, and **host** are
  parts of Nori, not additional ACP actors.
- "Host" alone means the `nori host` process. Say **ACP host** for the crate
  layer.
- A **session broker** has no single ACP role: it is the agent on downstream
  connections and the client upstream.
- An **agent** may call a **provider**, but provider-internal work crosses no
  ACP boundary unless the agent exposes it through ACP.
- A **token provider** replaces an **agent's** own sign-in for model calls.
  The agent stays the ACP server, and Nori stores only the name of the key's
  environment variable.
- [`nori auth`](../nori-rs/config.md#saved-api-keys) reports each **agent's**
  sign-in, and `nori auth set-key` / `remove-key` manage API keys saved per
  **provider** (for example `openai`). An **agent** receives a saved key only
  when it calls that provider and is opted in with `nori auth prefer-api-key`
  (`[auth.<agent>] prefer_api_key`); otherwise its own login wins. A saved
  provider key is not a **token provider**: it configures no endpoint and is
  never applied to an agent bound to a token provider.

## Turns and Ownership

### Turn ownership

| Term | Definition | Aliases to avoid |
| --- | --- | --- |
| **[Prompt turn](turn-lifecycle.md)** | The ACP v1 turn that begins with `session/prompt` and ends with its correlated response. | Request, task, conversation |
| **[Client-owned turn](turn-lifecycle.md)** | A **prompt turn** viewed from the **client connection** that sent its prompt request. | Local turn, owned turn, normal turn |
| **[Agent-owned turn](turn-lifecycle.md)** | A Nori extension turn established on a **client connection** by **agent-turn metadata** rather than a local prompt request. | Proactive turn, observed turn, remote turn |
| **[Turn ownership](turn-lifecycle.md)** | The connection-relative classification of a turn as client-owned or agent-owned. | Initiator, authority, control |
| **[Unowned update](turn-lifecycle.md)** | A non-metadata **session update** received without an **active local request**. | Orphan update, stray update, agent-owned turn |
| **[Unowned presentation](turn-lifecycle.md)** | TUI-only grouping that renders **unowned updates** without asserting that an ACP turn exists. | Proactive presentation, synthetic turn |

### Protocol

| Term | Definition | Aliases to avoid |
| --- | --- | --- |
| **[Prompt request](turn-lifecycle.md)** | The ACP `session/prompt` request that starts a **prompt turn**. | Prompt, user message |
| **[Active local request](turn-lifecycle.md)** | A prompt or load request sent by this connection that has not received its response. | Active turn, working state |
| **[Session update](turn-lifecycle.md)** | An ACP `session/update` notification carrying agent output or session state. | Turn, message, chunk |
| **[Prompt response](turn-lifecycle.md)** | The response to `session/prompt` that ends its **prompt turn** with a stop reason. | Completion event, idle event |

### Nori extension

| Term | Definition | Aliases to avoid |
| --- | --- | --- |
| **[Agent-turn metadata](turn-lifecycle.md)** | A `session_info_update` whose `_meta.nori.status` establishes or ends an **agent-owned turn** on the receiving connection. | Broker projection, synthetic request |
| **[Nori agent-turn status](turn-lifecycle.md)** | The `working` or `idle` value in `_meta.nori.status`. | Session status, ACP lifecycle event |
| **[Status-only frame](turn-lifecycle.md)** | Session metadata containing a recognized **Nori agent-turn status** but no displayable title or timestamp. | Prompt response, completion event |

- **Turn ownership** is relative to one **client connection**: the same work
  may be client-owned on the connection that sent the prompt and agent-owned
  on another.
- `working` establishes an **agent-owned turn**; `idle` ends it on that
  connection. Without that metadata, **unowned updates** are not a turn.
- ACP v1 defines no agent-initiated turn; **agent-owned turn** is Nori
  extension language.
- Ownership names provenance only, not authority, cancellation rights,
  permission, or queue semantics.
- "Prompt" conflates text, a user-message update, and an ACP request; use
  **prompt request** for `session/prompt`.
- "Completion" conflates a **prompt response** with `idle` agent-turn
  metadata; name the exact boundary.

## Follow-ups and Queue

| Term | Definition | Aliases to avoid |
| --- | --- | --- |
| **[Follow-up](turn-lifecycle.md#follow-ups)** | A message the user sends while a **prompt turn** is running, aimed at that turn. It is either **steered** or **queued**, and returns a delivery receipt. | Interrupt, managed follow-up |
| **[Steer](turn-lifecycle.md#keys)** | Enter during a running turn: inject the **follow-up** into that turn through `_session/steering`. Only allowlisted adapter versions support it; otherwise the message is queued. | Interrupt, append, send now |
| **[Queue](turn-lifecycle.md#prompt-queue)** | Tab during a running turn: hold the **follow-up** in the harness-owned FIFO and send it as a new **prompt turn** after the current turn ends with `end_turn`. `_nori/session/subscribe` clients see it through `QueueChanged` snapshots; standard ACP sessions on the **host** carry none, and `nori attach` does not render them. | Buffer, outbox, pending prompts |
| **[Paused queue](turn-lifecycle.md#drain-and-pause)** | A **queue** holding user prompts after a turn that did not end with `end_turn` (`interrupted` or `turn_failed`). It sends nothing until resumed; only the local TUI that owns the harness can resume it. | Stuck queue, cancelled queue |

- A **steer** adds input to the running turn; it does not start a new
  `session/prompt`, and the original turn still owns completion and
  cancellation.
- ACP v1 defines neither steering nor queueing; both are Nori behavior.
- The `/managed` relay keeps its own follow-up FIFO. It never shows a paused
  state, but after a turn ending other than `end_turn` it holds its items until
  a later turn ends with `end_turn`.
- "Queue" alone may mean the harness **queue** or the composer's prompt
  history; qualify it.

## Sessions, History, and Continuity

### Identity

| Term | Definition | Aliases to avoid |
| --- | --- | --- |
| **[Session](turn-lifecycle.md)** | An ACP conversation context with its own history and state. | Process, connection |
| **[ACP session ID](transcripts.md)** | The opaque agent-issued identifier used in ACP requests for one session. | Conversation ID |
| **[Conversation ID](transcripts.md)** | Nori's local UUID for a managed conversation, independent of its ACP session ID. For sessions run through the **session manager** it is also the transcript ID. | Session ID, ACP session ID |
| **[Continuity](transcripts.md)** | Nori's preservation of prior work across session and context changes. | Persistence, replay |

### Managed sessions

| Term | Definition | Aliases to avoid |
| --- | --- | --- |
| **[Session envelope](remote-control.md#10-managed-session-wire)** | A session event tagged with its **conversation ID**, agent, and per-conversation sequence number. | Frame, message |
| **[Session status](remote-control.md#10-managed-session-wire)** | A managed session's coarse state: starting, working, waiting on you, ready, failed, closed, or done (a delegated child whose task finished). | Phase, turn status |
| **[Session source](remote-control.md#10-managed-session-wire)** | How a managed session began: `user` (started by a client), `fork` (forked from its parent), or `session` (a **delegated child**). | Origin, kind |
| **[Session family](remote-control.md#10-managed-session-wire)** | A top-level **session** plus all of its descendants, whose **session statuses** roll up into one. | Thread, tree |
| **[Request resolved](remote-control.md#10-managed-session-wire)** | A **session envelope** announcing that a client answered a pending agent request, and which client. | Approval event |
| **[Active session](../nori-rs/config.md#several-sessions-in-one-tui)** | The one **session** the TUI shows; the prompt, terminal title, and scrollback belong to it. | Current tab |
| **[Parked session](../nori-rs/config.md#several-sessions-in-one-tui)** | A live **session** the TUI holds off screen with its own history view, input, and pending requests. | Hidden, paused |
| **[Session cap](../nori-rs/config.md#several-sessions-in-one-tui)** | The most managed **sessions** that may run at once, set by `[tui] max_sessions` (default 8) for both the TUI and `nori host`. `[host] max_sessions` is a separate limit that bounds only **managed launches**. | Limit, quota |
| **[Client limit](remote-control.md#11-background-host-and-attached-terminals)** | The most clients a background `nori host` serves at once over `/acp` and `/sessions`, set by `[host] max_clients` (default 16). A managed host has none. | Connection cap |

### Delegation

| Term | Definition | Aliases to avoid |
| --- | --- | --- |
| **[Delegation](remote-control.md#delegation)** | Starting a child **session** to work on a task for a parent, through `/delegate` or the `nori-client` MCP `delegate` tool. A same-kind child starts at once; a different agent kind waits for approval. | Subagent call, handoff |
| **[Delegated child](remote-control.md#delegation)** | The **session** a **delegation** starts, with **session source** `session`, named by the first line of its task. Its result returns to the parent as a queued prompt. | Subagent, worker |
| **[Delegation origin](remote-control.md#delegation)** | Who asked for a **delegated child**: the parent's agent (`agent`, via MCP) or the user or a client calling the manager (`user`). | Requester, source |

### Hosts, attach, and managed launches

| Term | Definition | Aliases to avoid |
| --- | --- | --- |
| **[Attach](remote-control.md#attached-frontend-and-recovery)** | `nori attach`: a terminal frontend connected to a **host**'s published endpoint (`/acp`, alias `/sessions`) instead of owning agent processes. | Connect, resume, remote control |
| **[Controller](remote-control.md#control-and-lifecycle-requests)** | The one client of a **host** that may prompt, send **follow-ups**, answer requests, and delegate. The first interactive client claims the empty role; another client takes it with `_nori/control/take`. | Owner, primary |
| **[Watcher](remote-control.md#control-and-lifecycle-requests)** | A client of a **host** without the **controller** role: it receives the same events and may call ACP **New**, **List**, **Load**, and **Resume** (loading or resuming a failed, closed, or stored session restarts it), but must take control before any other change to a session. A client that initializes with `observe: true` never claims control. | Observer, viewer, spectator |
| **[Managed launch](../nori-rs/cli/src/host/docs.md#managed-launch-control-v1)** | A host session run for a provisioned **binding** over the host's `/managed` endpoint, enabled by `[host] managed = true` (default false). A managed host serves `/sessions` and `/managed` and no `/acp`. | Hosted session, managed session |
| **[Binding](../nori-rs/cli/src/host/docs.md#managed-launch-control-v1)** | A provisioned workspace and home with a pinned adapter that a **managed launch** runs in, identified by binding ID and generation. | Workspace, sandbox |
| **[Side session](../nori-rs/cli/src/host/docs.md#managed-launch-control-v1)** | A secondary ACP session opened beside a parent session on the same **binding**, rebuilt or forked. A managed-host protocol for an external client, enabled by `[host] managed` and `[host] side_sessions`; Nori's own terminals cannot open one. | Subsession, child session |

### Session lifecycle

| Term | Definition | Aliases to avoid |
| --- | --- | --- |
| **[New](remote-control.md)** | ACP `session/new` creates an independent session and returns its ACP session ID. | Resume, reconnect |
| **[List](remote-control.md)** | ACP `session/list` discovers agent-known sessions without restoring them. On the **host**, it lists live **session manager** sessions, then stored transcripts the host is not running. | History, load |
| **[Load](transcripts.md)** | ACP v1 `session/load` restores a session and replays its conversation history before responding. | Resume, transcript replay |
| **[Resume](transcripts.md)** | ACP v1 `session/resume` restores a session without replaying prior messages. | Load, Nori resume |
| **[Nori resume](transcripts.md)** | The resume picker or `nori resume`: opens a transcript in a new conversation using **Load**, **Resume**, or **New** plus transcript **replay**, by agent capability. | `session/resume`, reload |
| **[Restart](transcripts.md#restarting-a-failed-session)** | Relaunching a failed managed session on a new agent process under the same **conversation ID** and transcript, through session-list Ctrl+R or `_nori/session/resume`. On the **host**, ACP **Load** or **Resume** of a failed or stored session restarts it the same way. | Nori resume, reload |
| **[Close](remote-control.md)** | ACP `session/close` cancels ongoing work and frees resources for an active session. | Quit, detach, delete |

### History and continuity operations

| Term | Definition | Aliases to avoid |
| --- | --- | --- |
| **[Conversation history](transcripts.md)** | The agent-maintained prior content and context of a session. | Transcript, session list, prompt history |
| **[Transcript](transcripts.md)** | Nori's versioned local JSONL record of metadata, user input, and ordered events. | Conversation history, ACP log |
| **[Replay](transcripts.md)** | Historical ACP notifications re-emitted between replay markers without re-executing requests or side effects. | Resume, rerun, transcript |
| **[Fork](transcripts.md)** | Unstable ACP `session/fork` creates a child from existing session context. | Rewind, Git branch |
| **[Branch at head](transcripts.md)** | Nori forks the active ACP session and transcript, activates the child, and marks the parent closed (no ACP **Close** is sent). | Rewind, undo |
| **[Rewind to message](transcripts.md)** | Nori's older-message `/fork` path starts fresh from a display summary and prefills that message. | ACP fork, undo |
| **[Compaction](transcripts.md)** | Nori reduces model context through agent-native compaction or summary-and-session-swap. | History deletion, replay |
| **[Undo](transcripts.md)** | Nori restores a Git ghost snapshot and appends an `Undo` event to the transcript, without changing agent context or rewriting earlier lines. | Rewind, rollback conversation |

### Relationships

- One **conversation ID** may outlive several **ACP session IDs**, notably
  after fallback **compaction** or a **restart**.
- On the **host**'s `/acp`, `sessionId` is the **conversation ID**; the
  agent's own **ACP session ID** stays inside the host.
- **Load** produces agent-sourced **replay**; Nori's fallback produces
  transcript-sourced replay after **New**.
- A **transcript** records selected session traffic; it is not the
  agent-maintained **conversation history**.
- **Nori resume** creates a new **conversation ID**; **restart** keeps it.
  The **session manager** carries the conversation ID, family position, and
  envelope sequence across the new agent process, so subscriptions and replay
  cursors keep using that ID.
- `/fork` exposes **branch at head** (which uses ACP **fork**) and **rewind to
  message** (which does not). Branch at head marks the parent closed (no ACP **Close** is sent); rewind to
  message starts a new agent process. Both add the child to the parent's
  **session family**.
- **Undo** changes files, not conversation state; **rewind to message**
  changes conversation direction, not files.
- A **session envelope's** sequence starts at 1 for each **conversation ID**
  and continues across a **restart**. **Branch at head** starts a new sequence
  for the child.
- **Session status** is derived from the event stream; it is not the harness
  phase or a **Nori agent-turn status**.
- Closing a local session in the TUI stops its agent process; it is not ACP
  **Close**. The session stays listed as closed until removed. `/close` in
  `nori attach` sends ACP **Close** to the host.
- Replacing the **active session** with `/new`, `/agent`, or `/resume` parks
  it instead of closing it; a **parked session** keeps running and receiving
  its events. A **delegated child** stays parked until the user switches to it.
- A **request resolved** envelope clears that request from "waiting on you";
  requests cleared by idle, cancel, or session end get none.
- A **delegated child**'s transcript has no link to its parent; lineage lives
  only in the running **session manager**.
- "Session ID" alone is unsafe; say **ACP session ID** or **conversation ID**.
- "Resume" alone may mean **Nori resume**, ACP **Resume**, or a **restart**;
  name which.
- "History" may mean **conversation history**, a **transcript**, the session
  list, or composer prompt history; qualify it.
- "Child session" may mean a **delegated child**, a **branch at head** child,
  or a **side session**; name which.

## Tools, Execution, and Permissions

### Actions and reports

| Term | Definition | Aliases to avoid |
| --- | --- | --- |
| **[Tool call](turn-lifecycle.md)** | An identified action a model asks its agent to perform and the agent reports through ACP. | Command, permission |
| **[Tool call update](turn-lifecycle.md)** | An ACP notification changing an existing **tool call** under the same ID. | New call, result |
| **[Tool call status](turn-lifecycle.md)** | The ACP value `pending`, `in_progress`, `completed`, or `failed` reported for a tool call. | Turn status |
| **[Shell command](turn-lifecycle.md)** | A command line or argument vector executed as a process, from a tool call or a user. | Tool call |
| **[Patch](turn-lifecycle.md)** | Structured file additions, deletions, updates, or moves. | Diff, edit |

### Permission and policy

| Term | Definition | Aliases to avoid |
| --- | --- | --- |
| **[ACP permission request](turn-lifecycle.md)** | A correlated agent-to-client request presenting options for one tool call. | Approval, authorization |
| **[Permission option](turn-lifecycle.md)** | An identified agent-supplied allow or reject choice, optionally remembered. | Outcome |
| **[Permission outcome](turn-lifecycle.md)** | An ACP response selecting an option or reporting prompt-turn cancellation. | Approval, grant |
| **[User approval](turn-lifecycle.md)** | A human's affirmative choice for one **ACP permission request**. | Outcome, policy |
| **[Approval policy](headless.md)** | Nori configuration deciding whether to surface a permission request or auto-select an allow option. | Sandbox policy, execution policy |
| **[Sandbox policy](../nori-rs/sandbox/docs.md)** | Resolved filesystem and network restrictions for Nori's local sandbox execution. | Approval policy, permission |
| **[Execution policy](../nori-rs/execpolicy/docs.md)** | Execpolicy rules classifying argument vectors as allow, prompt, or forbidden; not part of the live ACP permission path. | Approval policy |
| **[Cancellation](turn-lifecycle.md)** | A client signal to stop the active turn, cancel pending permission requests, and abort agent work. | Rejection, failure |

- A **shell command** or **patch** may be the payload of a **tool call** but
  is not itself one. External ACP agents execute their own tools; Nori's
  sandbox executor does not run them.
- A rejecting **permission outcome** is not a `failed` **tool call status**,
  and only an affirmative human selection is **user approval**.
- **Cancellation** completes when the correlated prompt response reports the
  cancelled stop reason; final tool call updates may arrive first. ACP v1's
  **tool call status** has no `cancelled` value; do not invent one.
- A diff can report file effects without proving Nori applied a patch.
- "Policy" alone is too broad; say **approval policy**, **sandbox policy**, or
  **execution policy**.
