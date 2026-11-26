# Glossary

This glossary defines the terms used only inside `nori-cli`. Terms that cross
a repo boundary (agent, ACP adapter, attach, close, conversation, harness,
host, session, steer, turn, and others) are defined in the nori-monorepo
`GLOSSARY.md`. An entry here that names a cross-repo term only adds the CLI
detail; the definitions agree.

Every CLI document uses terms as this glossary defines them.

## Terms

**ACP adapter** — See nori-monorepo `GLOSSARY.md`. The adapter each built-in
agent runs, and its pinned version: `docs/agents.md`.

**Active session** — The one session the TUI shows. The composer, terminal
title, and scrollback belong to it. Other live sessions are parked.

**Agent** — See nori-monorepo `GLOSSARY.md`. The CLI agent IDs and their
aliases: `docs/agents.md`.

**Approval policy** — The `approval_policy` setting: `untrusted`,
`on-failure`, `on-request`, or `never`. Each value stands for one permission
mode. See `docs/permissions.md`.

**Child session** — A session that the session manager starts for a parent
session through delegation. It runs one task and returns its result to the
parent. Also called a delegated child.

**Compaction** — Reducing the agent's context on `/compact`. **Native
compaction** forwards `/compact` to an agent that advertises a `compact`
command, as an ordinary turn. **Summarize-and-swap** asks the agent for a
summary, starts a new ACP session for the same conversation, and puts the
summary in front of the next prompt. See `docs/turns.md`.

**Controller** — See **control role** in nori-monorepo `GLOSSARY.md`. In
`nori attach`, a watcher's hint row shows "watching · read-only", and Alt+T
takes control. The TUI is always the controller of the private host it owns.
A **local controller** runs on the host's machine; a **remote controller**
does not. See **local client**.

**Conversation** — See nori-monorepo `GLOSSARY.md`. In the CLI the
conversation ID is a UUID. On `nori host` the ACP `sessionId` is the
conversation ID. The TUI session list and `nori host ls` show sessions; each
row is keyed by its conversation ID.

**Delegation** — A request from a parent session to start a child session for
one task. The parent's agent asks with the `delegate` tool, or the user asks
with `/delegate <agent> <task>`. A delegation has a request ID, an origin, a
status, and a depth. See `docs/turns.md`.

**Delegation depth** — How far a child session is below the top session. A
child of a top session has depth 1. The limit is 2.

**Delegation origin** — Who asked for a delegation: `agent` (the parent's
agent called `delegate`) or `user` (`/delegate` or a client call).

**Delegation status** — `awaiting_approval`, `running`, `declined`,
`completed`, `failed`, or `cancelled`.

**Envelope** — A session event wrapped with its conversation ID, agent, and a
sequence number (`SessionEnvelope`). The sequence starts at 1 for each
conversation ID. A fork starts a new sequence for the new conversation. The
wire form is `_nori/session/event` (nori-monorepo
`docs/acp-extensions.md`).

**Family** — A top session and all its child sessions, at every depth.

**Family status** — The session status of the family member that most needs
the user, in this order: waiting on you, failed, working, starting, ready,
closed or done. The TUI session list shows it on the top session's row.

**Follow-up** — A message sent against a named running turn, with an intent
of steer or queue. Each follow-up has a message ID and gets a receipt. See
`docs/turns.md`.

**Follow-up sender** — Who sent a follow-up: `host-controller` (a client of
`nori host`) or `machine-client` (the broker on a managed host). The host sets
it.

**Fork** — ACP `session/fork` at the head of the conversation. It starts a
new conversation with the full history of the old one. The old session is
closed. Fork never means going back to an earlier message; that is rewind.

**Goal** — A long-running objective set with `/goal <objective>`. While a goal
is active, Nori or the agent keeps starting turns toward it until the goal is
complete, blocked, or stopped. Goal statuses: `active`, `paused`, `blocked`,
`usage_limited`, `budget_limited`, `complete`. See `docs/turns.md`.

**Goal continuation** — A hidden prompt that Nori sends after a turn to keep
an active goal going.

**Harness** — See nori-monorepo `GLOSSARY.md`: the upstream coding-agent
project. Never use "harness" for the session runtime.

**Live session** — A session that is registered with a running session
manager, with or without a running agent process. Opposite: stored session.

**Local client** — A client of a host that runs on the host's machine: plain
`nori`'s in-process pipe, or a WebSocket client that presents the host's
private `host/local-token`, as `nori attach` with local discovery does. Every
other client is a **remote client**, including every stdio client. Only a
local controller may run `!` commands. See `docs/tui.md`.

**Parked session** — A live session that the TUI holds off screen. It keeps
running and keeps its own transcript, composer, and pending requests.

**Pause** — The prompt queue stops sending after a turn that did not end
normally. Two reasons: **interrupted** (the user cancelled the turn) and
**turn failed** (the turn failed or stopped early). Pause is reserved for
the prompt queue.

**Permission mode** — Who answers a session's permission requests: `ask`,
`agent`, or `approve-all`. See `docs/permissions.md`.

**Prompt queue** — The ordered list of prompts that wait for a session's
running turn to end. It sends one prompt per turn. Clients see only the
user's prompts; Nori's own prompts (compaction, goal continuation) wait in it
hidden. Queue is reserved for the prompt queue. See `docs/turns.md`.

**Receipt** — The answer to a follow-up: its message ID, sender, and status
(`injected`, `queued`, `unsupported_queued`, `idle_queued`, `rejected`, or
`unknown`). A repeated message ID gets the first receipt again.

**Reclaim** — Take one queued prompt back out of the prompt queue before it
is sent, to edit it or to drop it. A prompt that has started cannot be
reclaimed.

**Rewind** — Go back to an earlier user message. Nori starts a new
conversation with a plain-text summary of the history before that message
and puts the message back in the composer. Rewind is not fork.

**Sandbox settings** — `sandbox_mode`, `[sandbox_workspace_write]`,
`--add-dir`, and the sandbox half of `--yolo`. They are parsed but not
enforced yet. See `docs/permissions.md`.

**Session** — See nori-monorepo `GLOSSARY.md`: the live ACP context. The TUI
and `nori host` list and manage sessions. UI strings that say "session" stay
as they are.

**Session cap** — The most live sessions with an agent process at once:
`[tui] max_sessions`, default 8. Child sessions count.

**Session manager** — The part of the session runtime that registers live
sessions by conversation ID, runs one agent process per session, and owns
delegation.

**Session runtime** — The layer above the ACP connection that runs sessions:
turns, the prompt queue, follow-ups, goals, compaction, delegation, and
transcripts. Every CLI surface uses it. Its crate is `nori-harness`
(`nori-rs/harness`); the crate name does not mean harness.

**Session status** — A live session's coarse state: starting, working,
waiting on you, ready, failed, closed, or done (a child session that finished
its task).

**Steer** — See nori-monorepo `GLOSSARY.md`. In the CLI a steer is a
follow-up with intent `steer`, sent to the agent with `_session/steering`.
See `docs/turns.md`.

**Stored session** — A conversation that has a transcript under `NORI_HOME`
but no live session. Resuming it starts a new live session for the same
conversation ID.

**Turn** — See nori-monorepo `GLOSSARY.md`. A turn ends normally when the
agent stops with `end_turn`. Any other stop reason, or an error, ends it
early.

**Undo** — `/undo`: restore the files from a Git snapshot taken before a
turn. The conversation does not change.

**Watcher** — See **controller**.
