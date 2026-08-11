# Headless execution

`nori exec` runs a configured ACP agent without starting the terminal UI. It has two intentionally separate interfaces: a plaintext command for one-off shell use and a standard ACP stdio facade for programmatic callers.

## Plaintext mode

Pass the prompt as an argument:

```sh
nori exec "Summarize this repository"
```

Or pipe it on stdin, either by omitting the argument or by passing `-`:

```sh
printf '%s\n' "Explain the failing tests" | nori exec
printf '%s\n' "Explain the failing tests" | nori exec -
```

`nori -p` (`--print`) selects the same non-interactive mode without the
subcommand, matching the flag other agent CLIs use:

```sh
nori -p "Summarize this repository"
```

To use piped text as *context* for an instruction, ask for it with `--stdin`:

```sh
git diff | nori -p --stdin "Review this change"
```

The argument is the instruction and the piped text is the context it operates
on. The two are joined into one prompt, instruction first, separated by a blank
line.

**A prompt argument on its own never reads stdin.** Only an omitted prompt, a `-`
prompt, or `--stdin` does. This matters because a process inherits its parent's
stdin: without that rule, `nori exec "..."` inside a `while read` loop, a git
hook, or a CI step would consume input intended for something else, or block
until an unrelated pipe closed. Supplying no prompt from any source is an error.

A piped prompt is capped at 10 MiB; anything larger is rejected rather than
buffered.

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

Headless behavior is enabled only through the explicit `exec` subcommand or its `-p` / `--print` alias. Piping into bare `nori` does not make the run headless — see below.

## Piping into an interactive session

Bare `nori` is always interactive, whether or not stdin is a pipe. Piped text
seeds the first turn and the terminal UI then starts normally, so the
conversation continues from there:

```sh
echo "Explain this repository" | nori
git diff | nori --stdin "Review this change"
```

The same rules apply as for `exec`: a prompt argument on its own leaves stdin
alone, and piped text is read only when the prompt is omitted, is `-`, or
`--stdin` is passed. Unlike `exec`, supplying no prompt at all is fine — that is
just an ordinary interactive session with an empty composer.

Because the piped prompt is submitted as the first turn, be deliberate about
what you pipe in. Redirecting a file you did not intend to send (`nori < notes`)
sends it to the provider.

This requires a controlling terminal, because the UI reads keys from it once
stdin is consumed. Stdout must still be a terminal. In an environment with
neither — a CI job or a detached process — use `nori exec` or `nori -p`, which
never open a UI. After the pipe is drained Nori re-points its stdin at the
controlling terminal, so `$EDITOR` and the file browser still work normally.
