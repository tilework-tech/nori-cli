# Headless execution

`nori exec` runs a configured ACP agent without starting the terminal UI. It
has two separate interfaces: a plaintext command for one-off shell use, and
`--acp`, which speaks standard ACP on stdio for programmatic callers. Both are
clients of [`nori host`](remote-control.md#11-background-host-and-attached-terminals):
each joins this `NORI_HOME`'s running host when it can and otherwise runs a
private one.

Code: `nori-rs/cli/src/main.rs`, `nori-rs/cli/src/stdin_prompt.rs`,
`nori-rs/cli/src/host/oneshot.rs`, `nori-rs/cli/src/host/local_stdio.rs`.

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
without filtering. When the agent cannot start, for example because its
package cannot be launched or its token-provider key variable is unset, the
error names that cause.

### Where the prompt runs

Plaintext mode is a [client of `nori host`](remote-control.md#one-shot-exec-as-a-client).
The prompt runs on this `NORI_HOME`'s background host when all of these hold:

- the host is running;
- its host contract serves this `nori`'s
  ([choosing a host](remote-control.md#choosing-a-host-for-clients));
- no other client controls it;
- neither `-c` nor `--add-dir` is given (a running host applies its own
  configuration).

The session then runs in that host's process and environment, and the
finished session stays listed by the host as stored. Otherwise Nori runs a
private in-process host for the one prompt. Output and exit status are the
same either way.

On the running host the agent sees the host's environment, not the calling
shell's: a variable such as an API key set only for the `nori exec` command
does not reach it. Interrupting `nori exec` (SIGINT or SIGTERM) cancels and
closes its session on either host.

### Approvals

Plaintext mode runs the session in the `agent` permission mode, which puts
Claude Code in `acceptEdits` and Codex in `workspace-write`
([session configuration](remote-control.md#session-configuration)).

Plaintext mode never shows an approval prompt. When an ACP-visible permission
request arrives, Nori selects the first reject option the agent offers
(`reject_once` or `reject_always`), or cancels the request when there is none.
The agent may still return a final explanation; Nori prints it and then exits
with status 1.

Use `--dangerously-bypass-approvals-and-sandbox` only for trusted unattended
work. It runs the session in the `approve-all` mode, so Nori never asks for
approval for operations visible through ACP; on a private host it also sets
full sandbox access. It cannot govern tools a provider runs internally
without an ACP permission request.

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

## ACP on stdio (headless host)

```sh
nori exec --acp
```

`nori exec --acp` is an alias for `nori host --stdio`: one client that speaks
newline-delimited JSON-RPC ACP v1 on stdin and stdout to a `nori host`. It
gets exactly the methods of an authenticated `/acp` connection to `nori host`:
`session/new`, `list`, `load`, `resume`, `prompt`, `cancel`, `close`,
`delete`, `set_mode`, and `set_config_option`, plus the `_nori/*` extension
(control, subscribe, follow-up, delegation, host stop). See
[standard ACP v1 on the background host](remote-control.md#12-standard-acp-v1-on-the-background-host)
for the wire contract and [stdio transport](remote-control.md#stdio-transport)
for this mode.

`--acp` conflicts with a prompt argument and with `--stdin`. stdout carries
only ACP JSON-RPC lines; diagnostics go to stderr. Blank input lines are
ignored. A line that is not JSON receives a `-32700` error with a null `id`.

- `initialize` answers with `agentInfo.name = "nori-host"`,
  `agentInfo.version` set to `<nori version>+contract.<host contract>`, and
  the host's standard `sessionCapabilities`.
- One stream can run any number of sessions and any number of prompts per
  session. Turns stream `session/update` notifications as the agent produces
  them, then answer with the agent's `stopReason`.
- `session/new` takes an absolute `cwd`, whose project configuration applies,
  and accepts stdio and HTTP `mcpServers`.
- `session/load` and `session/resume` restart a stored session under its
  **[conversation ID](glossary.md#identity)**.

### Joining the running host

The process relays the client to this `NORI_HOME`'s background host over
`/acp` when all of these hold:

- the host is running and serves this `nori`'s host contract;
- no `--agent`, `-c`, `--add-dir`, or bypass flag is given;
- the host answers the client's first `initialize` with
  `control.role: "controller"`.

Sessions are then shared with every other client of that host and outlive
this process. stdin EOF only disconnects (exit 0); SIGINT or SIGTERM
disconnects with a non-zero exit. A stdio client never takes control from
another client; when it cannot join, it runs a private host.

### Private host

- The stdio client always holds control, so it receives every
  `session/request_permission`; `observe: true` on `initialize` is ignored.
  There is no bearer token.
- `--agent`, `-C`, `-c`, `--add-dir`, and
  `--dangerously-bypass-approvals-and-sandbox` set the private host's
  configuration exactly as they do for `nori host`.
- A private host takes no host lock and publishes no discovery files, so it
  runs beside a detached `nori host` using the same `NORI_HOME`. Its sessions
  end with the process; their transcripts remain, and `session/load` or
  `session/resume` restarts them later.
- Neither host sees the other's live sessions: each lists the other's as
  stored (`live: false`). Do not load or resume one host's live session on
  the other; that runs it twice on one transcript.

| Event | Outcome |
| --- | --- |
| stdin reaches EOF | Sessions stop; exit 0. |
| `_nori/host/stop` | Sessions stop; exit 0. |
| SIGINT or SIGTERM | Sessions stop; exit non-zero. |
| The client falls too far behind and is disconnected | Sessions stop; exit non-zero. |

## Common options

`nori exec` accepts:

```text
--acp                                       Serve ACP on stdio (alias of nori host --stdio)
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
