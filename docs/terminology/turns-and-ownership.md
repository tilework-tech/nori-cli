# Turns and Ownership

## Turn ownership

| Term                     | Definition                                                                                                                       | Aliases to avoid                                           |
| ------------------------ | -------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------- |
| **Prompt turn**          | The ACP v1 turn that begins with `session/prompt` and ends with its correlated response.                                         | Request, task, conversation                                |
| **Client-owned turn**    | A **prompt turn** viewed from the **client connection** that sent its prompt request.                                            | Local turn, owned turn, normal turn                        |
| **Agent-owned turn**     | A Nori extension turn established on a **client connection** by explicit agent-turn metadata rather than a local prompt request. | Proactive turn, observed turn, remote turn, agent-led turn |
| **Turn ownership**       | The connection-relative classification of a turn as client-owned or agent-owned.                                                 | Initiator, authority, control                              |
| **Unowned update**       | A non-metadata **session update** received outside a locally active prompt turn.                                                 | Orphan update, stray update, agent-owned turn              |
| **Unowned presentation** | TUI-only grouping used to render **unowned updates** without asserting that an ACP turn exists.                                  | Proactive presentation, synthetic turn                     |

## Protocol

| Term                | Definition                                                                         | Aliases to avoid             |
| ------------------- | ---------------------------------------------------------------------------------- | ---------------------------- |
| **Prompt request**  | The ACP `session/prompt` request that starts a **prompt turn**.                    | Prompt, user message         |
| **Session update**  | An ACP `session/update` notification carrying agent output or session state.       | Turn, message, chunk         |
| **Prompt response** | The response to `session/prompt` that ends its **prompt turn** with a stop reason. | Completion event, idle event |

ACP v1 defines prompt turns but no agent-initiated turn primitive.

## Nori extension

| Term                       | Definition                                                                                                 | Aliases to avoid                     |
| -------------------------- | ---------------------------------------------------------------------------------------------------------- | ------------------------------------ |
| **Agent-turn metadata**    | Nori metadata that establishes an **agent-owned turn** on the receiving connection.                        | Broker projection, synthetic request |
| **Nori agent-turn status** | The `working` or `idle` value in `_meta.nori.status` carried by **agent-turn metadata**.                   | Session status, ACP lifecycle event  |
| **Status-only frame**      | Session metadata containing a recognized **Nori agent-turn status** but no displayable title or timestamp. | Prompt response, completion event    |

## Relationships

- **Turn ownership** is always relative to one **client connection**.
- A **prompt turn** is client-owned only on the connection that sent its **prompt request**.
- With agent-turn metadata, the same work may be client-owned on one connection and agent-owned on another.
- `working` establishes an **agent-owned turn**; `idle` ends it on that connection.
- Without **agent-turn metadata**, **unowned updates** do not constitute a turn in ACP language.
- **Unowned presentation** may render statusless updates without inventing a turn or request ID.
- Ownership names provenance, not authority, controls, cancellation rights, or future protocol behavior.
- Supporting a future ACP agent-initiated turn primitive will require an explicit terminology migration.

## Example dialogue

> **Dev:** "A webchat sent the prompt, but the CLI received the resulting updates. Who owns the turn?"
>
> **Domain expert:** "Ownership is connection-relative. It is **client-owned** on the webchat connection."
>
> **Dev:** "Is it automatically **agent-owned** on the CLI connection?"
>
> **Domain expert:** "Only if **agent-turn metadata** establishes that turn. Otherwise the CLI received **unowned updates**, not an ACP turn."

## Flagged ambiguities

- "Owned turn" omits both owner and connection; say **client-owned turn** or **agent-owned turn**.
- **Agent-owned turn** is Nori extension language, not an ACP v1 protocol primitive.
- **Unowned update** and **agent-owned turn** are not synonyms: metadata is required to establish the turn.
- "Ownership" must not imply authority, cancellation, permission, or queue semantics.
- "Prompt" conflates text, a user-message update, and an ACP request; use **prompt request** for `session/prompt`.
- "Completion" conflates a **prompt response** with `idle` agent-turn metadata; name the exact boundary.
