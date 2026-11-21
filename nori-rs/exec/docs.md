# Noridoc: nori-exec

Path: @/nori-rs/exec

### Overview

- `nori-exec` is the terminal-independent frontend for finite and script-controlled Nori sessions.
- It offers a one-prompt plaintext runner and an ACP-over-stdio facade; the runtime `multi_session` flag adds manager envelopes and session-addressed commands to that facade.
- Both surfaces execute through the same ACP harness used by the interactive TUI.

### How it fits into the larger codebase

- `nori-cli` resolves configuration, selects the mode (`nori exec` or its `-p` / `--print` alias), and owns process stdin, stdout, stderr, and exit status.
- Prompt ingestion is entirely a `nori-cli` concern: it composes the argument prompt and any piped stdin into one string and hands `run_plaintext` a fully resolved prompt. `nori-exec` never reads a prompt from the process itself.
- `nori-exec` prepares and activates sessions through `nori-harness`; it does not spawn agents or reduce ACP state independently.
- `nori-harness` supplies the ordered `SessionEvent` stream and preserves exact ACP request IDs used for prompt and permission correlation.
- `nori-installed` supplies the optional authenticated activity reporter. Both
  execution surfaces attach it to the downstream harness handle, so capture uses
  the same first-prompt-on-ACP boundary as the interactive frontend.
- Plaintext mode projects text `agent_message_chunk` updates into one final answer.
- ACP mode uses the upstream ACP SDK to present an agent endpoint while acting as a client of the configured downstream ACP agent.

### Core Implementation

- `run_plaintext` passes `AgentPrepareSpec` and the already-chosen
  `SessionStart::New` to `prepare_and_launch_session`, submits one text prompt,
  collects text chunks until the correlated prompt response, and shuts down the
  session. When supplied, its reporter emits `nori_agent_session_started` with
  `session_mode = exec` after that prompt receives a downstream wire request ID,
  then performs a bounded final flush without changing the plaintext outcome.
- If the prompt cannot be submitted because the agent never started,
  `run_plaintext` drains the already-buffered events and returns the
  `SessionEnded` message (for example the `SpawnFailed` reason, such as a
  missing token-provider key) instead of the generic prompt-rejection error.
  The generic error remains only when no ended message was buffered.
- Unattended permission requests are rejected immediately using a schema-provided reject option, with cancellation as the safe fallback.
- With `multi_session` disabled, `run_acp` handles standard `initialize`, `session/new`,
  `session/set_config_option`, `session/prompt`, and `session/cancel` traffic
  over line-delimited JSON-RPC stdio. Its downstream `session/new` also uses
  `prepare_and_launch_session` with the explicit New choice. The facade assigns
  `session_mode = acp`; initialization and `session/new` alone are silent, and
  the attached reporter fires only after the facade's accepted `session/prompt`
  reaches the downstream ACP transport.
- The flag-off ACP facade exposes the downstream session ID and effective configuration options, then emits one complete `agent_message_chunk` before the correlated prompt response.
- Delegated permission requests are forwarded to the upstream ACP caller and their responses are returned to the downstream agent under the original downstream request ID.
- With `[features] multi_session`, [`managed.rs`](src/managed.rs) owns a
  [`SessionManager`](../harness/src/session_manager/mod.rs). Standard
  `session/new` creates independent sessions with stable Nori conversation IDs;
  prompts and configuration requests address those IDs. Each session can accept
  another prompt after its preceding turn completes. Each standard prompt
  subscribes to the harness immediately before admission, so output from a
  preceding extension-driven turn cannot leak into its text projection. The
  manager independently retains the original envelopes for extension replay.
  A separate session-filtered manager observer supplies `RequestResolved`
  signals while prompt content remains on the raw harness subscription. If a
  managed extension answers a forwarded permission, the collector stops
  awaiting the standard ACP answer and continues the original prompt; a later
  reply to that abandoned wait is not forwarded downstream.
- Managed extension requests are dispatched to the same
  [`ManagedRemoteHost`](../harness/src/managed_remote.rs) used by WebSocket
  clients. Subscription notifications carry the original `SessionEnvelope`,
  including agent, conversation ID, sequence, and ACP or Nori event. The
  standard ACP prompt projection remains a complete assistant-text update and
  correlated response. See the [wire contract](../../docs/remote-control.md)
  for method names, cursor recovery, and retention limits.
- A bounded managed subscription that ends drains its buffered envelopes, then
  emits `_nori/session/subscription_ended` with reason `replay_required`.
  The stdio connection remains open for a new subscription from the client's
  last processed cursors; the WebSocket adapter instead closes its connection.
  Encoding or writer failure stops forwarding without promising an end marker.
- The SDK owns stdout for both standard ACP responses and managed envelope
  notifications. EOF stops forwarding and shuts down every managed session;
  this remains a process-owned stdio frontend, not a persistent host daemon.

### Things to Know

- With `multi_session` disabled, the facade remains bounded to one downstream
  session and one prompt per process. Plaintext mode is unchanged by the flag.
- Analytics is optional at this crate boundary. The binary selects the mode and
  constructs the reporter; embedders that pass `None` retain identical session
  behavior without capture.
- Plaintext and flag-off ACP choose `SessionStart::New` before activation and use
  `prepare_and_launch_session`; managed ACP uses the manager's equivalent
  preparation and launch path. Preparation and activation retain one
  downstream subprocess even though the API keeps those lifecycle phases
  distinct.
- The combined harness path returns its handle immediately and keeps
  preparation inside the runtime's connect lifecycle: commands queue until the
  backend is ready, shutdown can win the race, the runtime emits a warning
  event after eight seconds, and an unresponsive connection is aborted after
  another thirty.
  `nori-exec` must not await `prepare_agent` outside that bounded lifecycle.
- Caller-provided MCP servers and additional directories are rejected in the first version because Nori cannot merge those inputs into an already resolved client configuration safely.
- Standard ACP methods are a facade projection. Managed subscribers explicitly
  opt into the complete manager stream rather than receiving only that
  projection.
- Only assistant text is projected into the final facade update. The prompt response retains the downstream ACP stop reason.
- ACP mode owns process stdin exclusively, in both directions of the contract: stdin EOF cancels active work and shuts down the downstream harness, and machine-readable stdout is owned exclusively by the ACP connection writer. The caller must therefore not consume stdin before dispatching into the facade -- any eager read (piped-prompt ingestion, in particular) belongs strictly after the ACP branch is ruled out, or the JSON-RPC stream is silently corrupted.
- The dangerous approval bypass can govern only permission requests that cross an ACP boundary; it cannot constrain or approve provider-internal tools.

Created and maintained by Nori.
