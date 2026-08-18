# Noridoc: nori-exec

Path: @/nori-rs/exec

### Overview

- `nori-exec` is the terminal-independent frontend for finite and script-controlled Nori sessions.
- It offers a one-prompt plaintext runner and a bounded ACP-over-stdio agent facade without introducing a Nori-specific machine event schema.
- Both surfaces execute through the same ACP harness used by the interactive TUI.

### How it fits into the larger codebase

- `nori-cli` resolves configuration, selects the mode (`nori exec` or its `-p` / `--print` alias), and owns process stdin, stdout, stderr, and exit status.
- Prompt ingestion is entirely a `nori-cli` concern: it composes the argument prompt and any piped stdin into one string and hands `run_plaintext` a fully resolved prompt. `nori-exec` never reads a prompt from the process itself.
- `nori-exec` launches and controls sessions through `nori-harness`; it does not spawn agents or reduce ACP state independently.
- `nori-harness` supplies the ordered `SessionEvent` stream and preserves exact ACP request IDs used for prompt and permission correlation.
- Plaintext mode projects text `agent_message_chunk` updates into one final answer.
- ACP mode uses the upstream ACP SDK to present an agent endpoint while acting as a client of the configured downstream ACP agent.

### Core Implementation

- `run_plaintext` creates one session, submits one text prompt, collects text chunks until the correlated prompt response, and shuts down the session.
- Unattended permission requests are rejected immediately using a schema-provided reject option, with cancellation as the safe fallback.
- `run_acp` handles standard `initialize`, `session/new`, `session/set_config_option`, `session/prompt`, and `session/cancel` traffic over line-delimited JSON-RPC stdio.
- The ACP facade exposes the downstream session ID and effective configuration options, then emits one complete `agent_message_chunk` before the correlated prompt response.
- Delegated permission requests are forwarded to the upstream ACP caller and their responses are returned to the downstream agent under the original downstream request ID.

### Things to Know

- The facade is intentionally bounded to one downstream session and one prompt per process.
- Caller-provided MCP servers and additional directories are rejected in the first version because Nori cannot merge those inputs into an already resolved client configuration safely.
- ACP mode is a facade, not a transport trace: internal notifications and Nori-owned lifecycle events are not passed through wholesale.
- Only assistant text is projected into the final facade update. The prompt response retains the downstream ACP stop reason.
- ACP mode owns process stdin exclusively, in both directions of the contract: stdin EOF cancels active work and shuts down the downstream harness, and machine-readable stdout is owned exclusively by the ACP connection writer. The caller must therefore not consume stdin before dispatching into the facade -- any eager read (piped-prompt ingestion, in particular) belongs strictly after the ACP branch is ruled out, or the JSON-RPC stream is silently corrupted.
- The dangerous approval bypass can govern only permission requests that cross an ACP boundary; it cannot constrain or approve provider-internal tools.

Created and maintained by Nori.
