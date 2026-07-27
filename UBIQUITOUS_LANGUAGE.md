# Ubiquitous Language

## Actors

| Term | Definition | Aliases to avoid |
| --- | --- | --- |
| **Client** | An ACP client, such as the Nori CLI or a webchat, that communicates with an **agent**. | Harness, peer, observer |
| **Agent** | An ACP server that performs work and emits requests, responses, and **session updates**. | Model, provider, backend |
| **Agent session** | One persistent ACP conversation between an **agent** and one or more clients. | Chat, thread, broker session |

## Turn ownership

| Term | Definition | Aliases to avoid |
| --- | --- | --- |
| **Turn** | A client-visible interval of agent work classified by which side controls its local lifecycle. | Request, task, conversation |
| **Client-owned turn** | The ACP v1 turn initiated by this client with a **prompt request** and bounded by its correlated response. | Local turn, owned turn, normal turn |
| **Agent-owned turn** | Agent work received without an **active client request**, modeled locally until ACP exposes agent-initiated turns directly. | Proactive turn, observed turn, remote turn, agent-led turn |
| **Active client request** | An outstanding prompt or load request issued by this client that owns related updates. | Active turn, local request |
| **Unowned update** | A non-metadata **session update** received without an **active client request**. | Orphan update, stray update, request-owned update |
| **Agent-owned presentation** | TUI-only state that groups and renders an **agent-owned turn** without changing ACP request state. | Proactive presentation, synthetic turn |

## Protocol and diagnostics

| Term | Definition | Aliases to avoid |
| --- | --- | --- |
| **Prompt request** | The ACP `session/prompt` request that starts a **client-owned turn**. | Prompt, user message |
| **Session update** | An ACP `session/update` notification carrying content or session metadata. | Event, message, chunk |
| **Unowned-update warning window** | The interval in which only the first **unowned update** emits the no-active-client-request warning. | Proactive turn, warning burst |

The warning text is `Received update with no active local request`. It is
diagnostic: the update is still accepted, preserved, and rendered.

## Nori Sessions metadata

| Term | Definition | Aliases to avoid |
| --- | --- | --- |
| **Session broker** | The Nori service that shares an **agent session** across clients and may supply Nori-specific metadata. | Turn owner, agent |
| **Nori agent-turn status** | An optional `working` or `idle` value in `_meta.nori.status` that sharpens **agent-owned presentation**. | Session status, lifecycle event |
| **Status-only frame** | Session metadata containing a recognized **Nori agent-turn status** but no displayable title or timestamp. | Turn event, completion event |

## Relationships

- Every **client-owned turn** has one local **prompt request** and one correlated response.
- An **agent-owned turn** has no local **prompt request**, synthetic request ID, or inferred client owner.
- An **unowned update** starts or extends **agent-owned presentation** and remains unowned.
- `working` starts or confirms **agent-owned presentation**; `idle` ends that presentation.
- Nori agent-turn statuses are optional presentation hints, not ACP lifecycle authority.
- Without a status hint, a later **active client request** separates agent-owned output.
- Agent-owned presentation never drives cancellation, queue draining, command gating, or ACP phase.
- Only a client-owned prompt response completes a **client-owned turn** and may drain its queue.
- A **status-only frame** is hidden; other metadata on the same frame remains visible.
- An **unowned-update warning window** is rearmed by a new local prompt or load, not by `idle`.
- A **session broker** may relay agent-owned activity, but agent-owned turns do not require one.

## Example dialogue

> **Dev:** "A stdio agent sent content even though this client never sent a prompt request. What owns the turn?"
>
> **Domain expert:** "It is an **agent-owned turn**. The **unowned update** emits one diagnostic warning, then renders normally."
>
> **Dev:** "Do `working` and `idle` make it a client request that I can cancel?"
>
> **Domain expert:** "No. They only bound **agent-owned presentation**. A **client-owned turn** starts with this client's **prompt request** and ends with its correlated response."

## Flagged ambiguities

- "Owned turn" omits the owner; always say **client-owned turn** or **agent-owned turn**.
- **Agent-owned** describes the local ACP control relationship, not the human or transport that ultimately initiated the work.
- An **unowned update** is unowned by the local client but may belong to an **agent-owned turn**.
- "Completion" conflates a client prompt response with an agent presentation boundary; name the specific boundary.
- "Prompt" conflates text, a user-message update, and a request; use **prompt request** only for `session/prompt`.
- "Session status" conflates durable metadata with `_meta.nori.status`; use **Nori agent-turn status** for the latter.
- Use **session broker** only for broker-specific behavior; it does not define turn ownership.
