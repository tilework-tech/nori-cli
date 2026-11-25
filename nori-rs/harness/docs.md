# Session runtime (`nori-harness`)

Path: `nori-rs/harness`

The crate is named `nori-harness` and will be renamed. This page calls it the
session runtime. In [`GLOSSARY.md`](../../GLOSSARY.md), "harness" alone means
the upstream coding-agent project, not this crate.

## Purpose

The session runtime runs Nori sessions on top of `nori-acp-host`. It prepares
an agent connection, starts or resumes an ACP session on it, and controls the
session through typed methods. It owns turns, the prompt queue, follow-ups and
steering, goals, compaction, rewind and fork, undo, hooks, user shell
commands, automatic worktrees, transcripts, message history, and delegation.
It publishes one ordered stream of `nori_protocol::SessionEvent` per session.
Its `SessionManager` keeps every live session of one process, keyed by
conversation ID. It has no terminal dependency. User-facing turn behavior is
owned by [`docs/turns.md`](../../docs/turns.md); transcript files by
[`docs/storage.md`](../../docs/storage.md).

## Main modules and types

| Module | Main items | Role |
|---|---|---|
| `runtime.rs` | `AgentPrepareSpec`, `prepare_agent`, `PreparedAgent`, `SessionStart`, `launch_session`, `HarnessHandle` | Public embedding API: prepare, activate, control |
| `backend/` (private) | `AcpBackend`, `session_reducer`, `prepared`, `nori_client_mcp`, `goal_ext`, `queue_reclaim`, `user_shell` | The per-session engine behind `HarnessHandle` |
| `normalized/` (private) | session phase | Private reduction of ACP updates |
| `session_event_fanout.rs` | | One unbounded primary consumer plus bounded subscribers |
| `session_manager/` | `SessionManager`, `SessionRecord`, `StartSession`, `ManagedSession`, `SessionSource` | Registry, capacity, envelopes, replay, recovery, delegation |
| `managed_remote.rs`, `managed_remote/`, `remote_agent.rs` | `ManagedRemoteHost` | Implements `nori_acp_host::remote::ManagedAgent` over the manager, including `_nori/session/queue/*` (`managed_remote/queue.rs`), `_nori/session/goal/*` (`managed_remote/goal.rs`), `_nori/session/context` (`managed_remote/context.rs`), `_nori/session/compact` (`managed_remote/compact.rs`) and `_nori/session/shell` (`managed_remote/shell.rs`); re-exports the transport types |
| `managed_launch.rs`, `managed_launch/` | `ManagedLaunches`, `LaunchRefusal`, `session_folder` | Launch admission for a managed host |
| `managed_acp.rs`, `managed_acp/` | | Relay for one admitted binding and generation; side sessions |
| `follow_up.rs`, `follow_up_rpc.rs` | `FollowUpReceipt`, `FollowUpStatus`, `supports_guarded_steering` | Follow-up delivery and steering gate |
| `permission_mode.rs` | | The three permission modes and their `approval_policy` |
| `transcript/`, `transcript_discovery.rs` | `TranscriptRecorder`, `TranscriptLoader`, `SCHEMA_VERSION` (3) | Nori transcripts; provider transcript lookup for token counts |
| `context_timeline/` | | Context usage readings at prompt boundaries; the timeline types are the wire types in `nori-protocol` |
| `instruction_files/` | | The instruction files an agent loads from a folder, with token counts; read on the host for `_nori/session/context` and by the TUI for local sessions |
| `hooks.rs`, `undo.rs`, `auto_worktree.rs`, `message_history.rs`, `custom_prompts.rs` | | Session hooks, git ghost snapshots, worktrees, `history.jsonl`, prompt files |

```text
 frontend / host                              SessionManager
       |  StartSession                          | registry: ConversationId -> record
       v                                        | capacity (max_sessions, default 8)
 prepare_agent(AgentPrepareSpec) -> PreparedAgent
       |  + SessionStart (new | load | resume)  |
       v                                        |
 HarnessHandle --ops--> AcpBackend --> AcpConnection (nori-acp-host) --> agent
       ^                    |
       |                    v reduce (private)
       |            SessionEventFanout --> primary stream (unbounded)
       |                    |          \-> bounded subscribers
       +----- manager pump: SessionEvent -> SessionEnvelope {conversation_id, seq}
                                              -> journal, per-session stream, subscribers
```

## Dependency direction

```text
nori-host-runtime, nori-tui, nori-cli
            |
            v
      nori-harness --> nori-acp-host --> nori-protocol
            |      \-> nori-config, nori-protocol, nori-auth-providers,
            |          codex-git, codex-rmcp-client, codex-utils-string
```

The session runtime must not depend on `nori-host-runtime`, `nori-tui`,
`ratatui`, or `crossterm`. It reaches the remote transport only through the
`ManagedAgent` trait that `nori-acp-host` defines.

## Invariants

- Take a resolved `Arc<NoriConfig>`. Do not reload config while preparing,
  refreshing, launching, or resuming.
- `PreparedAgent` is the single owner of an initialized connection before a
  session exists. Activating it consumes it.
- Publish ACP unchanged in `SessionEvent::Acp`. Keep reduction private; do not
  move it into `nori-protocol` or expose a normalized enum.
- Correlate raw ACP requests and responses only by ACP `RequestId`.
- One agent process per session. The manager mints the conversation ID before
  launch and passes it as `AcpBackendConfig.conversation_id`; the transcript
  uses it as the transcript ID.
- Envelope `seq` starts at 1 per conversation, with no gaps. A resume of the
  same conversation continues it. A fork starts a new conversation.
- A bounded subscriber that falls behind is dropped and its receiver closes.
  It never slows a session or the primary consumer.
- A host subscription (`_nori/session/subscribe` in `managed_remote.rs`) may
  fall as far behind as the journal retains (4,096 envelopes by default). A
  reconnect could not replay a larger gap anyway. Restoring a failed session
  publishes thousands of envelopes within milliseconds, so a shorter queue
  dropped clients that only paused briefly.
- Remote and local mutations go through the same typed handle methods, so
  hooks, transcripts, goals, and permission policy always apply.
- Goal completion is proven only by `update_goal` on the `nori-client` MCP
  server (`backend/nori_client_mcp.rs`), never by a similarly named agent tool.
- Steering is gated by an adapter capability (`_meta.steering.supported`) plus
  a minimum adapter version per adapter. Otherwise a follow-up queues.
- "Fork" means ACP `session/fork` only. Going back to an earlier message is
  rewind.
- Changing a `_nori/*` method or `nori.dev/*` key starts in
  `nori-monorepo: docs/acp-extensions.md`. Managed launch, side sessions, and
  checkpoints follow `nori-monorepo: docs/specs/nori-cli/host-contract.md`.

## Tests

| What | Where |
|---|---|
| Runtime, queue, follow-ups, delegation, fork, transcripts, managed launch and remote | `tests/*.rs`, `tests/session_manager/`, `tests/delegation/`, `tests/context_timeline/` |
| Shared env helpers and fixtures | `tests/support/`, `tests/fixtures/` |
| Resume through the real runtime | `src/runtime_resume_tests.rs` |
| Registry rules without a process | `src/session_manager/registry_tests.rs` |
| Unit tests | `#[cfg(test)]` modules in `src/` |

Run:

```sh
cd nori-rs
cargo build -p mock-acp-agent      # or set MOCK_ACP_AGENT_BIN
cargo test -p nori-harness
```

Many integration tests drive `mock_acp_agent` through scenario environment
variables. The PTY suite (`nori-rs/tui-pty-e2e`) covers the same paths
through the real `nori` binary.

## Where `main` differs

- c01: the crate is still named `nori-harness`.
- c05: `supports_guarded_steering` (`src/follow_up.rs`) allows exact adapter
  versions (`claude-agent-acp` 0.102.0, 0.108.1, 0.109.0; `codex-acp` 2.7.0,
  2.8.0), not a minimum version per adapter.
