# nori-protocol

Path: `nori-rs/nori-protocol`

## Purpose

`nori-protocol` holds the public types that cross the boundary of a Nori ACP
session. It re-exports the ACP schema as `nori_protocol::acp`. It adds only the
types ACP does not define: the session event stream, the session manager's
envelope and status, the `nori host` wire additions, and shared text for
delegation. The crate has no behavior beyond small pure helpers. Terms follow
[`GLOSSARY.md`](../../GLOSSARY.md).

## Modules

| Module | Main items | Role |
|---|---|---|
| `lib.rs` | `acp`, `PROMPT_ECHO_ID_META_KEY` | Re-exports `agent_client_protocol_schema` as `acp` |
| `session_event.rs` | `SessionEvent::{Acp, Nori}`, `AcpEvent`, `NoriEvent` and its payloads | The ordered event stream of one session |
| `session_envelope.rs` | `SessionEnvelope`, `SessionStatus` | One event tagged with its conversation ID, agent, and `seq` |
| `host.rs` | `HostDiscovery`, `HOST_CONTRACT`, `host_serves_contract`, `ControlRole`, `_nori/*` params and results, `host_wire_schema()` | Nori additions on `nori host` |
| `context.rs` | `ContextTimeline` and its turns, checkpoints, observations and segments; `ContextParams`, `ContextReport` | A session's context report, as `_nori/session/context` replies and the harness records it |
| `delegation.rs` | `DELEGATION_RESULT_HEADER`, `delegation_result_prompt`, name helpers | Text every surface uses for a delegated child |
| `pending_request.rs` | `PendingRequest` | What a waiting session asks for, in session lists |
| `reply_preview.rs` | `ReplyPreview` | Start of the latest agent reply, in session lists |

```text
SessionEnvelope { conversation_id, agent, seq, event }
                                              |
                                        SessionEvent
                                   /                    \
                     Acp(AcpEvent)                      Nori(NoriEvent)
           Notification | Request | Response      SessionStarted, QueueChanged,
           (raw ACP v1, unchanged)                GoalChanged, DelegationChanged, ...
```

`SessionEvent::Acp` carries agent-to-client ACP traffic as the agent sent it.
`SessionEvent::Nori` carries lifecycle and product facts that ACP has no
message for.

## Dependency direction

```text
nori-acp-host, nori-harness, nori-host-runtime, nori-tui, nori-cli
                         |
                         v
                   nori-protocol --> agent-client-protocol-schema, schemars 1.x, semver
```

`nori-protocol` depends on no other workspace crate. It is the only workspace
crate that depends on `agent-client-protocol-schema`. Other crates reach ACP
types through `nori_protocol::acp`.

## Invariants

- Keep behavior out. Reduction of ACP updates belongs to the session runtime
  (`nori-rs/harness`). Display projection belongs to `nori-tui`. Do not add a
  normalized ACP enum here.
- Do not restate ACP. Add a `NoriEvent` variant only for a fact ACP cannot
  carry.
- `SessionEnvelope.seq` starts at 1 per conversation and increases by one per
  event with no gaps. A fork starts a new conversation and a new sequence.
- `SessionEnvelope.conversation_id` is the conversation ID, never an ACP
  session ID.
- `AcpEvent::Response` decodes a payload that carries `stopReason` as a
  `PromptResponse`. ACP's `AgentResponse` is untagged, and the all-optional
  `AuthenticateResponse` would otherwise absorb any payload, so a client
  reading envelopes back from the wire never sees its turn end.
- `SessionStatus::is_closed()` is true for both `Closed` and `Done`. Code that
  checks only `Closed` misses finished delegated children.
- `HOST_CONTRACT` is the host wire version. The bump rule and the
  compatibility rule are owned by the host contract
  (`nori-monorepo: docs/specs/nori-cli/host-contract.md`, "Versioning").
  `host_serves_contract` implements the caret rule; keep its table test in
  step.
- Every `_nori/*` method and `nori/*` or `nori.dev/*` `_meta` key is listed in
  `nori-monorepo: docs/acp-extensions.md`. Update that registry before you add
  or change one here.
- The schema uses `schemars` 1.x, because the ACP schema types implement that
  trait. Older workspace crates use 0.8. Do not mix them in one type.
- A doc comment on a type in `host_wire_schema()` becomes a `description` in
  the checked-in schema and TypeScript. Regenerate both after you edit one.

## Generated files

| File | Source |
|---|---|
| `schema/host-wire.schema.json` | `host_wire_schema()`, JSON Schema draft 2020-12, keys sorted |
| `typescript/host-wire.ts` | Generated from the schema with Node |

Regenerate both from the repository root:

```sh
scripts/generate-host-wire-types.sh          # schema (cargo) and types (node)
scripts/generate-host-wire-types.sh --check  # types match the schema
```

## Tests

| What | Where | Run |
|---|---|---|
| Unit tests, including the contract table test | `#[cfg(test)]` modules in `src/` | `cd nori-rs && cargo test -p nori-protocol` |
| Checked-in schema and TypeScript match the types | `nori-rs/cli/tests/host_wire_schema.rs` | `cd nori-rs && cargo test -p nori-cli --test host_wire_schema` |

The TypeScript check needs `node` and `npm` on `PATH`. It skips without them,
except when `CI` is set.

## Where `main` differs

- `.github/workflows/rust-ci.yml` does not run `cargo test -p nori-protocol`.
  Only the schema tests in `nori-cli` run in CI.
