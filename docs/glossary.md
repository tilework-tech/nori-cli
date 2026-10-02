# Glossary

Use these terms in code, docs, and reviews. The "Aliases to avoid" column lists
words that blur the boundary the term names.

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

### Connections

| Term                  | Definition                                                                                                      | Aliases to avoid              |
| --------------------- | --------------------------------------------------------------------------------------------------------------- | ----------------------------- |
| **Client connection** | One **client's** protocol relationship to a **session** and the perspective from which ownership is classified. | Transport, socket, observer   |
| **Transport**         | The channel that carries ACP JSON-RPC messages between a **client** and an **agent** without defining meaning.  | Protocol, connection, session |

- A **user** acts through a **client** and is not an ACP protocol participant.
- The **ACP host** and **session harness** are internal parts of Nori's client, not additional ACP actors.
- A **session broker** has no single ACP role: it is the agent on downstream connections and the client upstream.
- An **agent** may call a **provider**, but provider-internal work crosses no ACP boundary unless the agent exposes it through ACP.

## Turns and Ownership

### Turn ownership

| Term                     | Definition                                                                                                                       | Aliases to avoid                                           |
| ------------------------ | -------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------- |
| **Prompt turn**          | The ACP turn that begins with `session/prompt` and ends with its correlated response.                                            | Request, task, conversation                                |
| **Client-owned turn**    | A **prompt turn** viewed from the **client connection** that sent its prompt request.                                            | Local turn, owned turn, normal turn                        |
| **Agent-owned turn**     | A Nori extension turn established on a **client connection** by explicit agent-turn metadata rather than a local prompt request. | Proactive turn, observed turn, remote turn, agent-led turn |
| **Unowned update**       | A non-metadata **session update** received without an **active local request**.                                                  | Orphan update, stray update, agent-owned turn              |
| **Unowned presentation** | TUI-only grouping used to render **unowned updates** without asserting that an ACP turn exists.                                  | Proactive presentation, synthetic turn                     |

### Protocol

| Term                     | Definition                                                                           | Aliases to avoid             |
| ------------------------ | ------------------------------------------------------------------------------------ | ---------------------------- |
| **Prompt request**       | The ACP `session/prompt` request that starts a **prompt turn**.                      | Prompt, user message         |
| **Active local request** | A prompt or load request sent by this connection that has not received its response. | Active turn, working state   |
| **Session update**       | An ACP `session/update` notification carrying agent output or session state.         | Turn, message, chunk         |
| **Prompt response**      | The response to `session/prompt` that ends its **prompt turn** with a stop reason.   | Completion event, idle event |

### Nori extension

| Term                       | Definition                                                                                                                  | Aliases to avoid                     |
| -------------------------- | --------------------------------------------------------------------------------------------------------------------------- | ------------------------------------ |
| **Agent-turn metadata**    | A `session_info_update` whose `_meta.nori.status` establishes or ends an **agent-owned turn** on the receiving connection. | Broker projection, synthetic request |
| **Nori agent-turn status** | The `working` or `idle` value in `_meta.nori.status`.                                                                       | Session status, ACP lifecycle event  |
| **Status-only frame**      | Session metadata containing a recognized **Nori agent-turn status** but no displayable title or timestamp.                | Prompt response, completion event    |

- **Turn ownership** is relative to one **client connection**: the same work may be client-owned on the connection that sent the prompt and agent-owned on another.
- `working` establishes an **agent-owned turn**; `idle` ends it on that connection. Without that metadata, **unowned updates** are not a turn.
- ACP defines no agent-initiated turn; **agent-owned turn** is Nori extension language.
- Ownership names provenance only, not authority, cancellation rights, permission, or queue semantics.
- "Completion" conflates a **prompt response** with `idle` agent-turn metadata; name the exact boundary.

## Sessions, History, and Continuity

### Identity

| Term                | Definition                                                                                 | Aliases to avoid           |
| ------------------- | ------------------------------------------------------------------------------------------ | -------------------------- |
| **Session**         | An ACP conversation context with its own history and state.                                | Process, connection        |
| **ACP session ID**  | The opaque agent-issued identifier used in ACP requests for one session.                   | Conversation ID            |
| **Conversation ID** | Nori's local UUID for a transcript-backed conversation, independent of its ACP session ID. | Session ID, ACP session ID |

### Session lifecycle

| Term            | Definition                                                                                                                 | Aliases to avoid          |
| --------------- | -------------------------------------------------------------------------------------------------------------------------- | ------------------------- |
| **Load**        | ACP `session/load` restores a session and replays its conversation history before responding.                             | Resume, transcript replay |
| **Resume**      | ACP `session/resume` restores a session without replaying prior messages.                                                  | Load, Nori resume         |
| **Nori resume** | Nori's user action that chooses **Load**, **Resume**, or `session/new` plus transcript replay from identity and agent capabilities. | `session/resume`, reload  |
| **Close**       | ACP `session/close` cancels ongoing work and frees resources associated with an active session.                            | Quit, detach, delete      |

### History operations

| Term                     | Definition                                                                                               | Aliases to avoid                         |
| ------------------------ | -------------------------------------------------------------------------------------------------------- | ---------------------------------------- |
| **Conversation history** | The agent-maintained prior content and context of a session.                                             | Transcript, session list, prompt history |
| **Transcript**           | Nori's versioned local JSONL record of metadata, user input, and ordered events.                         | Conversation history, ACP log            |
| **Replay**               | Historical ACP notifications re-emitted without re-executing requests or side effects.                   | Resume, rerun, transcript                |
| **Branch at head**       | Nori forks the active ACP session and transcript, activates the child, and freezes the resumable parent. | Rewind, undo                             |
| **Rewind to message**    | Nori's older-message `/fork` path starts fresh from a display summary and prefills that message.         | ACP fork, undo                           |
| **Compaction**           | Nori reduces model context through agent-native compaction or summary-and-session-swap.                  | History deletion, replay                 |
| **Undo**                 | Nori restores a Git ghost snapshot without changing agent context or transcript.                         | Rewind, rollback conversation            |

- One **conversation ID** may outlive multiple **ACP session IDs**, notably after fallback **compaction**.
- **Load** produces agent-sourced **replay**; Nori's fallback produces transcript-sourced replay after `session/new`.
- `/fork` exposes both **Branch at head** (which uses ACP `session/fork`) and **Rewind to message** (which does not).
- **Undo** changes files, not conversation state; **Rewind to message** changes conversation direction, not files.
- "History" alone may mean **conversation history**, a **transcript**, the session list, or composer prompt history; qualify it.

## Tools, Execution, and Permissions

### Actions and reports

| Term                 | Definition                                                                                | Aliases to avoid    |
| -------------------- | ----------------------------------------------------------------------------------------- | ------------------- |
| **Tool call**        | An identified action a model asks its agent to perform and the agent reports through ACP. | Command, permission |
| **Tool call update** | An ACP notification changing an existing **tool call** under the same ID.                 | New call, result    |
| **Shell command**    | A command line or argument vector executed as a process, from a tool call or a user.      | Tool call           |
| **Tool call status** | The ACP value `pending`, `in_progress`, `completed`, or `failed` reported for a tool call. | Turn status         |

### Permission and policy

| Term                       | Definition                                                                                                     | Aliases to avoid                 |
| -------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------- |
| **ACP permission request** | A correlated agent-to-client request presenting options for one tool call.                                     | Approval, authorization          |
| **Permission outcome**     | An ACP response selecting an agent-supplied option or reporting prompt-turn cancellation.                      | Approval, grant                  |
| **User approval**          | A human's affirmative choice for one **ACP permission request**.                                               | Outcome, policy                  |
| **Approval policy**        | Nori configuration deciding whether to surface a permission request or auto-select an allow option.            | Sandbox policy, execution policy |
| **Sandbox policy**         | Resolved filesystem and network restrictions for Nori's local sandbox execution.                               | Approval policy, permission      |
| **Execution policy**       | Execpolicy rules classifying argument vectors as allow, prompt, or forbidden; not part of the live ACP permission path. | Approval policy                  |
| **Cancellation**           | A client signal to stop the active turn, cancel pending permission requests, and abort agent work.             | Rejection, failure               |

- A **shell command** may be the payload of a **tool call** but is not itself one. External ACP agents execute their own tools; Nori's sandbox executor does not run them.
- A rejecting **permission outcome** is not a `failed` **tool call status**, and only an affirmative human selection is **user approval**.
- **Cancellation** completes when the correlated prompt response reports the cancelled stop reason; final tool call updates may arrive first. ACP's **tool call status** has no `cancelled` value; do not invent one.
- A diff can report file effects without proving Nori applied a patch.
- "Policy" alone is too broad; say **approval policy**, **sandbox policy**, or **execution policy**.
