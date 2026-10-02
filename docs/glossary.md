# Glossary

## Actors and Protocol Boundaries

### Actors

| Term         | Definition                                                                                     | Aliases to avoid          |
| ------------ | ---------------------------------------------------------------------------------------------- | ------------------------- |
| **User**     | The person whose intent and authorization a **client** mediates during agent work.             | Client, account, ACP peer |
| **Client**   | An ACP client, such as Nori CLI or webchat, that mediates between a **user** and an **agent**. | Harness, peer, observer   |
| **Agent**    | An ACP server that performs work and emits requests, responses, and session updates.           | Model, provider, backend  |
| **Provider** | The external organization or service from which an **agent** obtains models or credentials.    | Agent, model, ACP server  |

### Nori runtime boundaries

| Term                | Definition                                                                                                                            | Aliases to avoid                 |
| ------------------- | ------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------- |
| **ACP host**        | Nori's low-level client-side boundary that owns the ACP SDK connection, agent process, wire lifecycle, and delegated-request routing. | Client, harness, agent           |
| **Session harness** | Nori's headless runtime that composes the **ACP host** with product lifecycle and one ordered event stream.                           | ACP host, client, session broker |
| **Session broker**  | The Nori service that shares a **session**, acting as agent downstream and client toward the upstream agent.                          | Provider, turn owner, transport  |

### Protocol boundaries

| Term                  | Definition                                                                                                                           | Aliases to avoid              |
| --------------------- | ------------------------------------------------------------------------------------------------------------------------------------ | ----------------------------- |
| **Client connection** | One **client's** protocol relationship to a **session** and the perspective from which ownership is classified.                      | Transport, socket, observer   |
| **Transport**         | The bidirectional channel that carries ACP JSON-RPC messages between a **client** and an **agent** without defining their semantics. | Protocol, connection, session |

### Relationships

- A **user** acts through a **client** and is not an ACP protocol participant.
- A **client** and an **agent** exchange ACP messages over a **transport**.
- On each **client connection**, one client communicates with one agent about a **session**.
- A **session harness** composes an **ACP host**, which owns the active connection and transport.
- A **session broker** is the agent on downstream connections and the client on its upstream connection.
- An **agent** may call a **provider**, but provider-internal work crosses no ACP boundary unless the agent exposes it through ACP.

### Example dialogue

> **Dev:** "The user selected Codex backed by OpenAI. Which one is the **agent**?"
>
> **Domain expert:** "Codex is the **agent** and OpenAI is its **provider**; the **user** acts through Nori as the **client**."
>
> **Dev:** "Where do the **ACP host**, **session harness**, and **session broker** fit?"
>
> **Domain expert:** "The harness composes the host inside the client. The broker is agent toward downstream clients and client toward the upstream agent."

### Flagged ambiguities

- "Client" names the ACP role, not the **user**, UI, **ACP host**, or **client connection**.
- "Agent" names the ACP server, not its model or **provider**.
- **ACP host** and **session harness** are internal parts of Nori's client implementation, not additional ACP actors.
- **Transport**, **client connection**, and **session** mean channel, relationship, and conversation respectively.
- A **session broker** has no single ACP role; its role depends on the connection boundary.

## Turns and Ownership

### Turn ownership

| Term                     | Definition                                                                                                                       | Aliases to avoid                                           |
| ------------------------ | -------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------- |
| **Prompt turn**          | The ACP v1 turn that begins with `session/prompt` and ends with its correlated response.                                         | Request, task, conversation                                |
| **Client-owned turn**    | A **prompt turn** viewed from the **client connection** that sent its prompt request.                                            | Local turn, owned turn, normal turn                        |
| **Agent-owned turn**     | A Nori extension turn established on a **client connection** by explicit agent-turn metadata rather than a local prompt request. | Proactive turn, observed turn, remote turn, agent-led turn |
| **Turn ownership**       | The connection-relative classification of a turn as client-owned or agent-owned.                                                 | Initiator, authority, control                              |
| **Unowned update**       | A non-metadata **session update** received without an **active local request**.                                                  | Orphan update, stray update, agent-owned turn              |
| **Unowned presentation** | TUI-only grouping used to render **unowned updates** without asserting that an ACP turn exists.                                  | Proactive presentation, synthetic turn                     |

### Protocol

| Term                     | Definition                                                                           | Aliases to avoid             |
| ------------------------ | ------------------------------------------------------------------------------------ | ---------------------------- |
| **Prompt request**       | The ACP `session/prompt` request that starts a **prompt turn**.                      | Prompt, user message         |
| **Active local request** | A prompt or load request sent by this connection that has not received its response. | Active turn, working state   |
| **Session update**       | An ACP `session/update` notification carrying agent output or session state.         | Turn, message, chunk         |
| **Prompt response**      | The response to `session/prompt` that ends its **prompt turn** with a stop reason.   | Completion event, idle event |

ACP v1 defines prompt turns but no agent-initiated turn primitive.

### Nori extension

| Term                       | Definition                                                                                                 | Aliases to avoid                     |
| -------------------------- | ---------------------------------------------------------------------------------------------------------- | ------------------------------------ |
| **Agent-turn metadata**    | Nori metadata that establishes an **agent-owned turn** on the receiving connection.                        | Broker projection, synthetic request |
| **Nori agent-turn status** | The `working` or `idle` value in `_meta.nori.status` carried by **agent-turn metadata**.                   | Session status, ACP lifecycle event  |
| **Status-only frame**      | Session metadata containing a recognized **Nori agent-turn status** but no displayable title or timestamp. | Prompt response, completion event    |

### Relationships

- **Turn ownership** is always relative to one **client connection**.
- A **prompt turn** is client-owned only on the connection that sent its **prompt request**.
- With agent-turn metadata, the same work may be client-owned on one connection and agent-owned on another.
- `working` establishes an **agent-owned turn**; `idle` ends it on that connection.
- Without **agent-turn metadata**, **unowned updates** do not constitute a turn in ACP language.
- **Unowned presentation** may render statusless updates without inventing a turn or request ID.
- Ownership names provenance, not authority, controls, cancellation rights, or future protocol behavior.
- Supporting a future ACP agent-initiated turn primitive will require an explicit terminology migration.

### Example dialogue

> **Dev:** "A webchat sent the prompt, but the CLI received the resulting updates. Who owns the turn?"
>
> **Domain expert:** "Ownership is connection-relative. It is **client-owned** on the webchat connection."
>
> **Dev:** "Is it automatically **agent-owned** on the CLI connection?"
>
> **Domain expert:** "Only if **agent-turn metadata** establishes that turn. Otherwise the CLI received **unowned updates**, not an ACP turn."

### Flagged ambiguities

- "Owned turn" omits both owner and connection; say **client-owned turn** or **agent-owned turn**.
- **Agent-owned turn** is Nori extension language, not an ACP v1 protocol primitive.
- **Unowned update** and **agent-owned turn** are not synonyms: metadata is required to establish the turn.
- "Ownership" must not imply authority, cancellation, permission, or queue semantics.
- "Prompt" conflates text, a user-message update, and an ACP request; use **prompt request** for `session/prompt`.
- "Completion" conflates a **prompt response** with `idle` agent-turn metadata; name the exact boundary.

## Sessions, History, and Continuity

### Identity

| Term                | Definition                                                                                 | Aliases to avoid           |
| ------------------- | ------------------------------------------------------------------------------------------ | -------------------------- |
| **Session**         | An ACP conversation context with its own history and state.                                | Process, connection        |
| **ACP session ID**  | The opaque agent-issued identifier used in ACP requests for one session.                   | Conversation ID            |
| **Conversation ID** | Nori's local UUID for a transcript-backed conversation, independent of its ACP session ID. | Session ID, ACP session ID |
| **Continuity**      | Nori's preservation of prior work across session and context changes.                      | Persistence, replay        |

### Session lifecycle

| Term            | Definition                                                                                                                 | Aliases to avoid          |
| --------------- | -------------------------------------------------------------------------------------------------------------------------- | ------------------------- |
| **New**         | ACP `session/new` creates an independent session and returns its ACP session ID.                                           | Resume, reconnect         |
| **List**        | ACP `session/list` discovers agent-known sessions without restoring or modifying them.                                     | History, load             |
| **Load**        | ACP v1 `session/load` restores a session and replays its conversation history before responding.                           | Resume, transcript replay |
| **Resume**      | ACP v1 `session/resume` restores a session without replaying prior messages.                                               | Load, Nori resume         |
| **Nori resume** | Nori's user action chooses **Load**, **Resume**, or **New** plus transcript fallback from identity and agent capabilities. | `session/resume`, reload  |
| **Close**       | ACP `session/close` cancels ongoing work and frees resources associated with an active session.                            | Quit, detach, delete      |

### History and continuity operations

| Term                     | Definition                                                                                               | Aliases to avoid                         |
| ------------------------ | -------------------------------------------------------------------------------------------------------- | ---------------------------------------- |
| **Conversation history** | The agent-maintained prior content and context of a session.                                             | Transcript, session list, prompt history |
| **Transcript**           | Nori's versioned local JSONL record of metadata, user input, and ordered events.                         | Conversation history, ACP log            |
| **Replay**               | Filtered historical ACP notifications that never re-execute requests or side effects.                    | Resume, rerun, transcript                |
| **Fork**                 | Optional, unstable ACP `session/fork` creates a child from existing session context.                     | Rewind, Git branch                       |
| **Branch at head**       | Nori forks the active ACP session and transcript, activates the child, and freezes the resumable parent. | Rewind, undo                             |
| **Rewind to message**    | Nori's older-message `/fork` path starts fresh from a display summary and prefills that message.         | ACP fork, undo                           |
| **Compaction**           | Nori reduces model context through agent-native compaction or summary-and-session-swap.                  | History deletion, replay                 |
| **Undo**                 | Nori restores a Git ghost snapshot without changing agent context or transcript.                         | Rewind, rollback conversation            |

### Relationships

- One **Conversation ID** may outlive multiple **ACP session IDs**, notably after fallback **compaction**.
- **Load** produces agent-sourced **replay**; Nori's fallback uses transcript-sourced replay after **New**.
- A **Transcript** records selected session traffic but is not the agent-maintained **conversation history**.
- **Close** ends an active session; later resumability is agent-specific, while quitting may only detach.
- **Undo** changes files, not conversation state; **Rewind to message** changes conversation direction, not files.

### Example dialogue

> **Dev:** "Does `nori resume` always send ACP **Resume**?"
>
> **Domain expert:** "No. **Nori resume** may use **Load**, live **Resume**, or **New** plus transcript-sourced **replay**."
>
> **Dev:** "After **Undo**, does the agent forget the reverted turn?"
>
> **Domain expert:** "No. **Undo** restores files only; use **Rewind to message** or **Branch at head** to change direction."

### Flagged ambiguities

- "Session ID" is unsafe alone; say **ACP session ID** or **Conversation ID**.
- "Resume" must distinguish the **Nori resume** action from ACP **Resume** and **Load**.
- "History" may mean **conversation history**, a **Transcript**, the session list, or composer prompt history; qualify it.
- `/fork` exposes both **Branch at head** and **Rewind to message**, but only the former uses ACP **Fork**.
- **Compaction** may replace the ACP session without creating a new Nori conversation, while **Undo** never rewinds agent context.

## Tools, Execution, and Permissions

### Actions and reports

| Term                 | Definition                                                                                | Aliases to avoid    |
| -------------------- | ----------------------------------------------------------------------------------------- | ------------------- |
| **Tool call**        | An identified action a model asks its agent to perform and the agent reports through ACP. | Command, permission |
| **Tool call update** | An ACP notification changing an existing **tool call** under the same ID.                 | New call, result    |
| **Shell command**    | A command line or argument vector executed as a process, from a tool call or a user.      | Tool call           |
| **Patch**            | Structured file additions, deletions, updates, or moves.                                  | Diff, edit          |

### Permission

| Term                       | Definition                                                                  | Aliases to avoid        |
| -------------------------- | --------------------------------------------------------------------------- | ----------------------- |
| **ACP permission request** | A correlated agent-to-client request presenting options for one tool call.  | Approval, authorization |
| **Permission option**      | An identified agent-supplied allow or reject choice, optionally remembered. | Outcome                 |
| **Permission outcome**     | An ACP response selecting an option or reporting prompt-turn cancellation.  | Approval, grant         |
| **User approval**          | A human's affirmative choice for one **ACP permission request**.            | Outcome, policy         |

### Controls and lifecycle

| Term                 | Definition                                                                                                                            | Aliases to avoid                 |
| -------------------- | ------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------- |
| **Tool call status** | The ACP lifecycle value `pending`, `in_progress`, `completed`, or `failed` reported for a tool call.                                  | Turn status                      |
| **Approval policy**  | Nori configuration deciding whether to surface a permission request or auto-select an allow option.                                   | Sandbox policy, execution policy |
| **Sandbox policy**   | Resolved filesystem and network restrictions for Nori's local sandbox execution.                                                      | Approval policy, permission      |
| **Execution policy** | Command rules classifying argument vectors as allow, prompt, or forbidden in execpolicy tooling, not Nori's live ACP permission path. | Approval policy                  |
| **Cancellation**     | A client signal to stop the active turn, cancel pending permission requests, and abort agent work.                                    | Rejection, failure               |

### Relationships

- A **shell command** or **patch** may be the payload of a **tool call**, but neither is itself an ACP tool call.
- A **tool call update** reports progress or results; it does not request execution.
- An **ACP permission request** may leave its tool call `pending`; its selected **permission outcome** may allow or reject.
- Only an affirmative human selection is **user approval**.
- **Approval policy** governs consultation, while **sandbox policy** constrains an execution environment.
- External ACP-agent tools bypass Nori's sandbox executor, and provider-internal tools may never cross an ACP permission boundary.
- **Cancellation** is not complete until the correlated prompt response reports the cancelled stop reason; final tool updates may arrive first.

### Example dialogue

> **Dev:** "The agent reported a **tool call** containing `cargo test`; did Nori run it?"
>
> **Domain expert:** "No. `cargo test` is the **shell command**; the external agent owns execution."
>
> **Dev:** "If Nori shows an **ACP permission request**, does selecting Reject mean the tool failed?"
>
> **Domain expert:** "No. That is a rejecting **permission outcome**; `failed` is a **tool call status**, while **cancellation** belongs to the prompt turn."

### Flagged ambiguities

- "Tool" may mean a capability, invocation, or UI row; reserve **tool call** for the identified ACP invocation.
- "Approval" conflates the request, a human decision, an ACP outcome, and policy; name the exact concept.
- ACP cancellation prose says clients should mark unfinished calls `cancelled`, but ACP's **tool call status** enumeration has no `cancelled` value; do not invent that wire status.
- "Patch" and "diff" differ: a diff can report file effects without proving Nori applied a patch.
- "Policy" alone is unusably broad; say **approval policy**, **sandbox policy**, or **execution policy**.
