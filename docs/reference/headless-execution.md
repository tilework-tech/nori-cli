# Headless execution

`nori exec` runs a configured ACP agent without starting the terminal UI. It has two intentionally separate interfaces: a plaintext command for one-off shell use and a standard ACP stdio facade for programmatic callers.

## Plaintext mode

Pass the prompt as an argument:

```sh
nori exec "Summarize this repository"
```

Or pipe it on stdin:

```sh
printf '%s\n' "Explain the failing tests" | nori exec
```

The complete assistant response is the only content written to stdout. Diagnostics and failures are written to stderr, so stdout can be redirected or piped without filtering terminal rendering or progress messages.

Plaintext mode never displays an interactive approval prompt. If an ACP-visible permission request occurs, Nori immediately selects a reject option supplied by the agent, or cancels the request when none exists. The agent may still return a final explanation; Nori prints that text but exits unsuccessfully to distinguish the run from fully authorized completion.

Use `--dangerously-bypass-approvals-and-sandbox` only for explicitly trusted unattended work. It disables Nori's approval and sandbox policy for operations visible through ACP. It cannot govern tools that a provider executes internally without requesting ACP permission.

## ACP stdio facade

```sh
nori exec --acp
```

This mode makes Nori an ACP agent over line-delimited JSON-RPC on stdin and stdout. There is no Nori event envelope and no raw pass-through of the downstream agent's complete event stream.

The initial facade supports:

- `initialize`
- `session/new`
- `session/set_config_option` before the prompt starts
- `session/prompt`
- `session/cancel`
- agent-to-client `session/request_permission`

`session/new` returns the downstream ACP session ID and its effective configuration options. During `session/prompt`, Nori collects the downstream assistant text and sends one complete standard `agent_message_chunk` update, followed by the correlated prompt response with the downstream `stopReason`. Permission requests are forwarded as standard ACP requests; the caller's correlated response is relayed to the downstream agent.

Version one is deliberately bounded to one session and one prompt per process. Caller-provided `mcpServers` and `additionalDirectories` are rejected. Configure the selected agent, MCP servers, approval policy, sandbox, and other runtime settings through normal Nori configuration and command-line overrides instead.

Closing stdin cancels active work, resolves outstanding downstream activity safely, and shuts down the process. Stdout remains exclusively ACP JSON-RPC; operational diagnostics use stderr or Nori's configured tracing destination.

## Common options

```text
--agent <AGENT>   Select the configured ACP agent
-C, --cwd <DIR>  Set the execution working directory
-c <KEY=VALUE>   Apply a normal Nori configuration override
```

The interactive `nori` command is unchanged. Headless behavior is enabled only through the explicit `exec` subcommand.
