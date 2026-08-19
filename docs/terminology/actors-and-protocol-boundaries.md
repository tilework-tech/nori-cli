# Actors and Protocol Boundaries

## Actors

| Term         | Definition                                                                                     | Aliases to avoid          |
| ------------ | ---------------------------------------------------------------------------------------------- | ------------------------- |
| **User**     | The person whose intent and authorization a **client** mediates during agent work.             | Client, account, ACP peer |
| **Client**   | An ACP client, such as Nori CLI or webchat, that mediates between a **user** and an **agent**. | Harness, peer, observer   |
| **Agent**    | An ACP server that performs work and emits requests, responses, and session updates.           | Model, provider, backend  |
| **Provider** | The external organization or service from which an **agent** obtains models or credentials.    | Agent, model, ACP server  |

## Nori runtime boundaries

| Term                | Definition                                                                                                                            | Aliases to avoid                 |
| ------------------- | ------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------- |
| **ACP host**        | Nori's low-level client-side boundary that owns the ACP SDK connection, agent process, wire lifecycle, and delegated-request routing. | Client, harness, agent           |
| **Session harness** | Nori's headless runtime that composes the **ACP host** with product lifecycle and one ordered event stream.                           | ACP host, client, session broker |
| **Session broker**  | The Nori service that shares a **session**, acting as agent downstream and client toward the upstream agent.                          | Provider, turn owner, transport  |

## Protocol boundaries

| Term                  | Definition                                                                                                                           | Aliases to avoid              |
| --------------------- | ------------------------------------------------------------------------------------------------------------------------------------ | ----------------------------- |
| **Client connection** | One **client's** protocol relationship to a **session** and the perspective from which ownership is classified.                      | Transport, socket, observer   |
| **Transport**         | The bidirectional channel that carries ACP JSON-RPC messages between a **client** and an **agent** without defining their semantics. | Protocol, connection, session |

## Relationships

- A **user** acts through a **client** and is not an ACP protocol participant.
- A **client** and an **agent** exchange ACP messages over a **transport**.
- On each **client connection**, one client communicates with one agent about a **session**.
- A **session harness** composes an **ACP host**, which owns the active connection and transport.
- A **session broker** is the agent on downstream connections and the client on its upstream connection.
- An **agent** may call a **provider**, but provider-internal work crosses no ACP boundary unless the agent exposes it through ACP.

## Example dialogue

> **Dev:** "The user selected Codex backed by OpenAI. Which one is the **agent**?"
>
> **Domain expert:** "Codex is the **agent** and OpenAI is its **provider**; the **user** acts through Nori as the **client**."
>
> **Dev:** "Where do the **ACP host**, **session harness**, and **session broker** fit?"
>
> **Domain expert:** "The harness composes the host inside the client. The broker is agent toward downstream clients and client toward the upstream agent."

## Flagged ambiguities

- "Client" names the ACP role, not the **user**, UI, **ACP host**, or **client connection**.
- "Agent" names the ACP server, not its model or **provider**.
- **ACP host** and **session harness** are internal parts of Nori's client implementation, not additional ACP actors.
- **Transport**, **client connection**, and **session** mean channel, relationship, and conversation respectively.
- A **session broker** has no single ACP role; its role depends on the connection boundary.
