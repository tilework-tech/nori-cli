# Headless execution

`nori exec` runs a configured ACP agent without starting the terminal UI. It
has two separate interfaces: a plaintext command for one-off shell use and a
standard ACP stdio facade (`--acp`) for programmatic callers.

Code: `nori-rs/cli/src/main.rs`, `nori-rs/cli/src/stdin_prompt.rs`,
`nori-rs/exec/src/lib.rs`, `nori-rs/exec/src/managed.rs`.

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

`nori -p` (`--print`) selects the same mode without the subcommand:

```sh
nori -p "Summarize this repository"
```

`-p` cannot be combined with a subcommand or with `-i`/`--image`; both are
rejected rather than silently ignored.

To use piped text as *context* for an instruction, ask for it with `--stdin`:

```sh
git diff | nori -p --stdin "Review this change"
```

The argument is the instruction and the piped text is the context. The two
are joined into one prompt, instruction first, separated by a blank line.

**A prompt argument on its own never reads stdin.** Only an omitted prompt, a
`-` prompt, or `--stdin` does. A process inherits its parent's stdin, so
without this rule `nori exec "..."` inside a `while read` loop, a git hook, or
a CI step would consume input meant for something else, or block until an
unrelated pipe closed. Stdin that is a terminal is never read. Supplying no
prompt from any source is an error.

Piped input of 10 MiB or more is rejected rather than buffered.

### Output

stdout carries only the assistant's text for the turn: every
`agent_message_chunk` concatenated, with a trailing newline added if missing.
Diagnostics and failures go to stderr, so stdout can be redirected or piped
without filtering.

### Approvals

`nori exec` and `nori -p` force `approval_policy = on-request`, overriding
config, unless `--dangerously-bypass-approvals-and-sandbox` is passed.

Plaintext mode never shows an approval prompt. When an ACP-visible permission
request arrives, Nori selects the first reject option the agent offers
(`reject_once` or `reject_always`), or cancels the request when there is none.
The agent may still return a final explanation; Nori prints it and then exits
with status 1.

Use `--dangerously-bypass-approvals-and-sandbox` only for trusted unattended
work. It sets full sandbox access and never asks for approval for operations
visible through ACP. It cannot govern tools a provider runs internally without
an ACP permission request.

### No steering or queue

Plaintext mode sends exactly one prompt. There is no way to steer a running
turn or queue a follow-up.

## Exit codes

| Code | Meaning |
| ---- | ------- |
| `0`  | The prompt completed, whatever its `stopReason` (`end_turn`, `max_tokens`, `refusal`, `cancelled`); or the `--acp` caller closed stdin. |
| `1`  | Runtime error: rejected permission request, missing prompt, oversized stdin, agent or prompt failure, `-p` with a subcommand or `--image`. |
| `2`  | Flag conflict or other usage error reported by clap, such as `--acp` with a prompt or `--stdin`. |

Plaintext callers that need to tell a refusal from a normal answer must read
the output; the exit code does not distinguish them.

## ACP stdio facade

```sh
nori exec --acp
```

Nori acts as an ACP agent over line-delimited JSON-RPC on stdin and stdout.
`--acp` conflicts with a prompt argument and with `--stdin`. stdout carries
only ACP JSON-RPC; diagnostics go to stderr or Nori's tracing log under
`$NORI_HOME/log`.

The facade handles:

- `initialize`
- `session/new`
- `session/set_config_option`, only while no facade `session/prompt` is
  running (in single-session mode, only before the one prompt). A turn started
  another way does not block it here; the call then fails with `-32000`.
- `session/prompt`
- `session/cancel`
- agent-to-client `session/request_permission`

`session/new` starts the downstream session in the request's `cwd`
(overriding `-C`) and returns the session ID and its effective configuration
options. Caller-provided `mcpServers` and `additionalDirectories` are rejected
with invalid params. Configure the agent, MCP servers, approval policy,
sandbox, and other runtime settings through Nori configuration and
command-line overrides instead.

During `session/prompt`, Nori collects the downstream assistant text and sends
it as one complete `agent_message_chunk` update (none when the text is empty),
then the prompt response with the downstream `stopReason`. Permission requests
are forwarded as standard ACP requests and the caller's answer is relayed
downstream. A caller error on a permission request cancels it downstream and
fails the prompt. Other agent-to-client requests get method not found.

Neither facade mode supports `session/load` or `session/resume`; `initialize`
does not advertise `loadSession`, and both calls return method not found.

Closing stdin cancels active work, shuts down the downstream agent, and exits
with status 0.

### Single-session mode (default)

With `[features] multi_session = false` (the default):

- One session and one prompt per process. A second `session/new` or
  `session/prompt`, or a `session/set_config_option` after the prompt starts,
  fails with invalid params. The session shuts down after its prompt.
- `session/new` returns the downstream agent's ACP session ID.
- There is no Nori event envelope and no raw pass-through of the downstream
  event stream.
- The `delegate` tool is not available.

### Multi-session mode (`multi_session`)

With `[features] multi_session = true`, `--acp` runs sessions under a session
manager:

- `initialize` advertises `agentCapabilities._meta["nori/multiSession"]` with
  `{"version": 1}`.
- `session/new` may be called several times. It returns a Nori conversation
  ID, and updates and permission requests use that ID as `sessionId`.
- Each session accepts repeated prompts, one at a time. A prompt is never
  queued. It fails with `-32015` ("session already has an active turn")
  while an earlier facade `session/prompt` is still running. If the active
  turn was started another way (for example `_nori/session/prompt`), it fails
  with `-32000` and the same message.
- Other methods go to the managed session wire: `_nori/session/list`,
  `_nori/session/subscribe`, `_nori/session/prompt`, `_nori/session/delegate`,
  `_nori/session/delegation/resolve`, and `_nori/session/respond`, with
  `_nori/session/event` and `_nori/session/subscription_ended`
  notifications. See [remote control](remote-control.md) for that wire.
- Agents in these sessions get the `nori-client` `delegate` tool; see
  [nori-client MCP](nori-client-mcp.md).
- `_nori/session/follow_up` is not available (method not found), so a
  headless caller cannot steer or queue a follow-up. Wait for the turn to end
  and send the next prompt.

## Common options

`nori exec` accepts:

```text
--acp                                       Serve the ACP stdio facade
--stdin                                     Append piped stdin to PROMPT
--agent <AGENT>                             Select the configured ACP agent
-C, --cwd <DIR>                             Set the working directory
-c, --config <key=value>                    Apply a Nori config override
--dangerously-bypass-approvals-and-sandbox  Disable approvals and sandboxing
```

With `nori -p`, the top-level equivalents apply instead: `-a, --agent`,
`-C, --cd`, `--add-dir`, `-c`, `--stdin`, and
`--dangerously-bypass-approvals-and-sandbox` (alias `--yolo`). Top-level
`--add-dir` given before `exec` also applies to `nori exec`.

## Piping into an interactive session

Bare `nori` is always interactive, whether or not stdin is a pipe. Headless
mode needs `nori exec` or `nori -p`. Piped text seeds the first turn and the
terminal UI then starts normally:

```sh
echo "Explain this repository" | nori
git diff | nori --stdin "Review this change"
```

The same stdin rules apply as for `exec`: a prompt argument on its own leaves
stdin alone, and piped text is read only when the prompt is omitted, is `-`,
or `--stdin` is passed. Unlike `exec`, no prompt at all is fine; the session
starts with an empty composer.

The piped prompt is submitted as the first turn, so redirecting a file you did
not mean to send (`nori < notes`) sends it to the provider.

This needs a controlling terminal, because the UI reads keys from it once
stdin is consumed, and stdout must be a terminal. In a CI job or a detached
process, use `nori exec` or `nori -p`. After the pipe is drained Nori points
its stdin back at the controlling terminal, so `$EDITOR` and the file browser
work normally.
