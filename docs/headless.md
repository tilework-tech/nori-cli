# Headless execution

`nori exec` runs a configured ACP agent without starting the terminal UI. It has two intentionally separate interfaces: a plaintext command for one-off shell use, and `--acp`, which runs a headless Nori host speaking standard ACP on stdio for programmatic callers.

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

The complete assistant response is the only content written to stdout. Diagnostics and failures are written to stderr, so stdout can be redirected or piped without filtering terminal rendering or progress messages. When the agent cannot start, for example because its package cannot be launched or its token-provider key variable is unset, the error names that cause.

Plaintext mode never displays an interactive approval prompt. If an ACP-visible permission request occurs, Nori immediately selects a reject option supplied by the agent, or cancels the request when none exists. The agent may still return a final explanation; Nori prints that text but exits unsuccessfully to distinguish the run from fully authorized completion.

Use `--dangerously-bypass-approvals-and-sandbox` only for explicitly trusted unattended work. It disables Nori's approval and sandbox policy for operations visible through ACP. It cannot govern tools that a provider executes internally without requesting ACP permission.

## ACP on stdio (headless host)

```sh
nori exec --acp
```

`nori exec --acp` is an alias for `nori host --stdio`: a headless local
background host whose one client speaks newline-delimited JSON-RPC ACP v1 on
stdin and stdout. It serves exactly the methods of an authenticated
`/sessions` connection to `nori host` — `session/new`, `list`, `load`,
`resume`, `prompt`, `cancel`, `close`, `delete`, `set_mode` and
`set_config_option`, plus the `_nori/*` extension (control, subscribe,
follow-up, delegation, host stop). See
[remote-control.md §12](remote-control.md#12-standard-acp-v1-on-the-background-host)
for the wire contract.

- `initialize` answers with `agentInfo.name = "nori-host"` and the host's
  standard `sessionCapabilities`.
- One stream can run any number of sessions and any number of prompts per
  session. Turns stream `session/update` notifications as the agent produces
  them, then answer with the agent's `stopReason`.
- `session/new` takes an absolute `cwd`, whose project configuration applies,
  and accepts stdio and HTTP `mcpServers`.
- The stdio client always holds control, so it receives every
  `session/request_permission`; `observe: true` on `initialize` is ignored.
  There is no bearer token.
- `--agent`, `-C`, `-c`, `--add-dir` and
  `--dangerously-bypass-approvals-and-sandbox` set the host's configuration
  exactly as they do for `nori host`.
- The process takes no host lock and publishes no discovery files, so it runs
  beside a detached `nori host` using the same `NORI_HOME`. Its sessions end
  with the process; their transcripts remain and `session/load` or
  `session/resume` restarts them later. Neither host sees the
  other's live sessions: each lists the other's as stored (`live: false`), and
  loading or resuming one there runs it twice on one transcript. Do not.

| Event | Outcome |
| --- | --- |
| stdin reaches EOF | Sessions stop; exit 0. |
| `_nori/host/stop` | Sessions stop; exit 0. |
| SIGINT or SIGTERM | Sessions stop; exit non-zero. |
| The client falls too far behind and is disconnected | Sessions stop; exit non-zero. |

Blank input lines are ignored. A line that is not JSON receives a `-32700`
error with a null `id`. Stdout carries only ACP JSON-RPC lines; diagnostics go
to stderr.

Clients of the earlier bounded facade see these differences: replies stream as
chunks rather than one final text chunk, the `nori/multiSession` capability is
no longer advertised, and caller `mcpServers` are accepted rather than
rejected. The exec analytics event with `session_mode = acp` is no longer sent.

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
