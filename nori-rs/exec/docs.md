# Noridoc: nori-exec

Path: @/nori-rs/exec

### Overview

- `nori-exec` is the terminal-independent runner for one finite plaintext prompt (`nori exec "..."` and its `-p` / `--print` alias).
- It executes through the same ACP harness used by the interactive TUI, then projects the turn's assistant text into one final answer.
- It no longer serves ACP. `nori exec --acp` is dispatched by `nori-cli` to the headless background host (`nori host --stdio`) in [cli/src/host](../cli/src/host/docs.md#local-host-on-stdio); this crate is not involved in that path.

### How it fits into the larger codebase

- `nori-cli` resolves configuration, selects the mode, and owns process stdin, stdout, stderr, and exit status. It takes the `--acp` branch into the host before it reads any prompt from stdin, so the host keeps exclusive ownership of the JSON-RPC stream.
- Prompt ingestion is entirely a `nori-cli` concern: it composes the argument prompt and any piped stdin into one string and hands `run_plaintext` a fully resolved prompt. `nori-exec` never reads a prompt from the process itself.
- `nori-exec` prepares and activates its session through `nori-harness` (`prepare_agent`, then `launch_session`); it does not spawn agents or reduce ACP state independently.
- `nori-installed` supplies the optional authenticated activity reporter, attached to the downstream harness handle so capture uses the same first-prompt-on-ACP boundary as the interactive frontend.

### Core Implementation

- [`run_plaintext`](src/lib.rs) prepares the agent, activates a new session and submits one prompt. It collects text `agent_message_chunk` updates until the response correlated with that prompt's request ID, then shuts down the session.
- When supplied, the reporter emits `nori_agent_session_started` with `session_mode = exec` after the prompt receives a downstream wire request ID, then performs a bounded final flush without changing the plaintext outcome.
- If the agent fails during startup, `run_plaintext` briefly waits for the terminal `SessionEnded` event so stderr can name the cause, such as a missing key variable. On timeout it returns the prompt-submission error.
- Unattended permission requests are rejected immediately using a schema-provided reject option, with cancellation as the safe fallback; the outcome then reports `permission_denied` so the CLI exits unsuccessfully. Any other agent-to-client request is answered with `method_not_found`.
- `SessionEnded` or `RequestFailed` before the prompt response fails the run with the harness's message.

### Things to Know

- Analytics is optional at this crate boundary. The binary constructs the reporter; embedders that pass `None` retain identical session behavior without capture.
- Plaintext awaits `prepare_agent` before activating the connection with `launch_session`, retaining the same downstream subprocess across preparation and activation.
- The dangerous approval bypass can govern only permission requests that cross an ACP boundary; it cannot constrain or approve provider-internal tools.
- The earlier bounded single-session ACP facade and its `multi_session` manager variant (which advertised `_meta["nori/multiSession"]`) were removed in favour of the host, so there is one ACP server implementation for stdio and WebSocket clients.

Created and maintained by Nori.
