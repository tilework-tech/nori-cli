# Command reference

This page lists every `nori` subcommand, flag, positional argument, alias,
environment variable, and exit code. Hidden commands and flags are marked
**hidden**. Terms follow `GLOSSARY.md`.

Related references:

- `config.toml` keys and `-c` overrides: [configuration.md](configuration.md)
- Files under `NORI_HOME`: [storage.md](storage.md)
- The `nori host` wire, discovery files, and managed launches:
  `nori-monorepo: docs/specs/nori-cli/host-contract.md`
- Agents, adapters, and `NORI_ACP_COMMAND_<SLUG>`: [agents.md](agents.md)
- `nori auth` behaviour, credential stores, token providers: [auth.md](auth.md)
- Permission modes and unattended runs: [permissions.md](permissions.md)

## Synopsis

```text
nori [OPTIONS] [PROMPT]
nori [OPTIONS] <COMMAND> [ARGS]
```

With no subcommand, `nori` opens the TUI. With `-p`, it runs one prompt and
prints the answer, as `nori exec` does.

| Command | Purpose |
|---|---|
| (none) | Open the TUI on a private host |
| `exec` | Run one prompt, or serve ACP on stdio |
| `resume` | Open the TUI on a stored session |
| `cloud` | Open the TUI on a session machine through Nori Sessions |
| `host` | Start, stop, list, or add sessions to the background host |
| `attach` | Open the TUI on sessions of a running host |
| `auth` | Show sign-in status and manage saved API keys |
| `skillsets` | Run `nori-skillsets` |
| `completions` | Print a shell completion script |
| `sandbox` (alias `debug`) | Run a command inside a platform sandbox |
| `execpolicy check` | **Hidden.** Check a command against execpolicy files |
| `stdio-to-uds` | **Hidden.** Relay stdio to a Unix socket |

## Global options

These options belong to the root command. Put them before the subcommand.
`-c` is global: it is also accepted after any subcommand.

| Option | Short | Value | Default | Effect |
|---|---|---|---|---|
| `--config` | `-c` | `key=value` | none | Override one `config.toml` key. Repeatable. See [configuration.md](configuration.md#command-line-overrides). |
| `--print` | `-p` | flag | off | Run `PROMPT` without the TUI, print the answer, and exit. Same as `nori exec`. |
| `--version` | `-V` | flag | | Print `nori-ai-cli <version>` and exit. |
| `--help` | `-h` | flag | | Print help and exit. |

### TUI options

The root command, `nori resume`, and `nori cloud` accept these options. `nori
host`, `nori attach`, and `nori exec` read `--agent`, `--cd`, `--add-dir`, and
`--yolo` from the root command, as the subcommand sections state.

| Option | Short | Value | Default | Effect |
|---|---|---|---|---|
| `PROMPT` | | text | none | First prompt of the session. `-` reads the prompt from piped stdin. |
| `--stdin` | | flag | off | Read piped stdin and append it to `PROMPT` as context. |
| `--image` | `-i` | `FILE[,FILE...]` | none | Attach images to the first prompt. Repeatable and comma-separated. |
| `--agent` | `-a` | slug | `agent` key | Agent for this run: `claude-code`, `codex`, `gemini` (Antigravity), `grok-build`, `opencode`, `cursor`, `hermes`, `pi`, or a configured agent. Does not change the saved `agent` key. |
| `--dangerously-bypass-approvals-and-sandbox` | | flag | off | Alias `--yolo`. Sets `approval_policy = "never"` and `sandbox_mode = "danger-full-access"` for this run. See [permissions.md](permissions.md). |
| `--cd` | `-C` | `DIR` | current directory | Working folder of the session. |
| `--add-dir` | | `DIR` | none | Extra writable root. Repeatable. The TUI exits with an error when the effective `sandbox_mode` is `read-only`. |
| `--skip-welcome` | | flag | off | Skip the first-launch welcome screen. |
| `--skip-trust-directory` | | flag | off | Skip the folder trust prompt. Nothing is saved, so the folder keeps the default `approval_policy`. |

### Prompt sources

`nori`, `nori -p`, and `nori exec` resolve the first prompt the same way:

| `PROMPT` | `--stdin` | stdin is a pipe | Prompt |
|---|---|---|---|
| text | off | any | `PROMPT`; stdin is not read |
| text | on | yes | `PROMPT`, a blank line, then stdin |
| text | on | no | `PROMPT` |
| `-` or omitted | any | yes | stdin |
| `-` or omitted | any | no | none |

- Line endings are normalized to `\n`. Leading and trailing whitespace is
  trimmed. An empty result counts as no prompt.
- Piped input is limited to 10 MiB. A larger pipe is an error.
- The TUI reopens `/dev/tty` as stdin after it reads a pipe (Unix only).
- `nori exec` and `nori -p` fail with "a prompt argument or piped stdin is
  required" when no prompt results.

## `nori` (TUI)

```text
nori [OPTIONS] [PROMPT]
```

Opens the TUI on a private host that lives as long as the process. On exit
it leaves the exit card in scrollback: the session ID, title, and agent;
message, tool, skill, and subagent counts; and the context, token totals, and
cost the agent reported. When the session had activity, the last line of
output is the command to resume it, on its own so a triple-click selects it:

```text
nori resume <conversation-id>
```

When the TUI exits with a pending update, `nori` runs the update command, then
prints "Update ran successfully! Please restart Nori."

### `-p`, `--print`

```text
nori -p [OPTIONS] [PROMPT]
```

Runs `nori exec` with the root `PROMPT`, `--stdin`, `--agent`, `--cd`,
`--add-dir`, `--yolo`, and `-c` values.

- `-p` with a subcommand fails: "--print cannot be combined with a subcommand;
  use `nori exec` instead".
- `-p` with `--image` fails: "--print does not support --image".

## `nori exec`

```text
nori exec [OPTIONS] [PROMPT]
nori exec --acp [OPTIONS]
```

| Option | Short | Value | Default | Effect |
|---|---|---|---|---|
| `PROMPT` | | text | none | Prompt to send. See [Prompt sources](#prompt-sources). Conflicts with `--acp`. |
| `--stdin` | | flag | off | Append piped stdin to `PROMPT`. Conflicts with `--acp`. |
| `--acp` | | flag | off | Serve ACP on stdin and stdout instead of running one prompt. |
| `--agent` | | slug | root `--agent`, then `agent` key | Agent for the session. |
| `--cwd` | `-C` | `DIR` | root `--cd`, then current directory | Working folder. Canonicalized when it exists. |
| `--dangerously-bypass-approvals-and-sandbox` | | flag | off | Approve every permission request. The root `--yolo` has the same effect. |
| `-c` | | `key=value` | none | Config override, applied after the root `-c` values. |

The root `--add-dir` also applies.

**One prompt** (`nori exec PROMPT`):

1. Nori uses the home's running background host when its `host.json`
   satisfies this build's host contract and the host makes this client the
   controller. A run with any `-c` override or `--add-dir` always uses a
   private host, because a running host applies its own configuration.
2. Otherwise it starts a private host inside the process.
3. It starts one session, sends the prompt, prints the agent's text to stdout
   with a trailing newline, and closes the session.
4. Without `--yolo`, the approval policy is `on-request` and every permission
   request is rejected. A rejected request makes the command fail with
   "permission request was rejected during unattended execution" after the
   output is printed.

**ACP on stdio** (`nori exec --acp`) is `nori host --stdio`: see
[`nori host --stdio`](#nori-host). The configured `approval_policy` applies
unless `--yolo` is set.

## `nori resume`

```text
nori resume [SESSION_ID] [OPTIONS] [PROMPT]
```

| Option | Value | Default | Effect |
|---|---|---|---|
| `SESSION_ID` | UUID | none | Stored session to open. |
| `--last` | flag | off | Open the most recent session without the picker. |
| `--all` | flag | off | List sessions from every folder, with a folder column. Default: only the current folder. |

All [TUI options](#tui-options) are accepted. With no `SESSION_ID` and no
`--last`, the TUI opens the session picker.

## `nori cloud`

```text
nori cloud [OPTIONS] [PROMPT]
```

| Option | Value | Default | Effect |
|---|---|---|---|
| `--onboard` | flag | off | Open the organization's onboarding session instead of the session picker. |

All [TUI options](#tui-options) are accepted, but the agent is always
`nori-cloud`: `--agent` is ignored.

- Nori runs `nori-handroll cloud-acp` (with `--onboard` when set) as the
  session's ACP agent. Handroll owns broker, sign-in, and transport. Sign in
  with `nori-handroll login`.
- The binary is `$NORI_HANDROLL_BIN` when set, else the first executable
  `nori-handroll` on `PATH`. A missing binary fails before the TUI starts.
- `[cloud] broker_url` is passed to the child as `NORI_BROKER_URL`.
- The TUI opens the session list first, and does not create a worktree.

## `nori host`

```text
nori host [OPTIONS]
nori host --stdio [--launch FILE [--inherit-env]]
nori host stop
nori host ls [--json]
nori host new [OPTIONS]
```

| Option | Short | Value | Default | Effect |
|---|---|---|---|---|
| `--agent` | `-a` | slug | root `--agent` | Agent for the host, or for `host new`. |
| `--port` | | integer | `7420` | Loopback port. `0` picks a free port. Must be 0 to 65535. |
| `--foreground` | | flag | off | **Hidden.** Run the host in this process. Conflicts with `--stdio`. |
| `--stdio` | | flag | off | Serve one client as ACP on stdin and stdout until stdin closes. |
| `--launch` | | `FILE` | none | JSON launch specification for one managed launch. Requires `--stdio`. |
| `--inherit-env` | | flag | off | Add this process's environment to the `--launch` specification; variables in the specification win. Requires `--launch`. |

The root `--agent`, `--cd`, `--add-dir`, `--yolo`, and `-c` values configure
the host. Paths are made absolute. `stop`, `ls`, and `new` do not load
configuration.

| Form | Behaviour |
|---|---|
| `nori host` | When a host already runs for this `NORI_HOME`, print its PID and endpoint and exit 0. Fail when its `host.json` does not satisfy this build's host contract. Otherwise start `nori host ... --foreground` detached in a new session, with output appended to `$NORI_HOME/host/host.log`, wait up to 15 seconds for it to answer, and print its PID, endpoint, and token path. |
| `nori host --foreground` | Run the background host in this process until `_nori/host/stop`, Ctrl-C, or SIGTERM. |
| `nori host --stdio` | Join the running background host when the client brings no configuration (no `--agent`, `--add-dir`, `--yolo`, or `-c`) and the host makes the client the controller. A joined client still copies the replies to requests it already forwarded after stdin closes. Replies are correlated by `id` and can arrive out of request order; `initialize` is always answered first. Otherwise serve a private host on stdio. Fails when `[host] managed = true`. A private stdio host exits with an error on SIGINT or SIGTERM. |
| `nori host --stdio --launch FILE` | Serve one managed launch as plain ACP on stdio. Needs `[host] managed = true` and `[host.dedicated_roots]`. |

Only one background host runs per `NORI_HOME`. Background hosts need Unix.
Discovery, endpoints, auth, and the managed wire:
`nori-monorepo: docs/specs/nori-cli/host-contract.md`.

### `nori host stop`

Stops the host and all of its sessions. Prints "No host running." when none
runs. Stop uses the connection the host reserves for lifecycle calls, so it
works when the host is at its client limit. It fails when the host it reaches
is not the instance recorded in the host files. Waits up to 30 seconds, then
prints "Host stopped."

### `nori host ls`

| Option | Value | Default | Effect |
|---|---|---|---|
| `--json` | flag | off | Print `{"sessions":[{"session_id","credential_refs"}]}`. |

Without `--json`, prints a table sized to the terminal, and plain text when
stdout is not a terminal.

### `nori host new`

Starts a session on a running host and prints its ID.

| Option | Value | Default | Effect |
|---|---|---|---|
| `--cwd` | `DIR` | current directory (local host); the host's own folder (`--host`) | Session folder. Relative paths start from the current directory. |
| `--agent` | slug | `host --agent`, then root `--agent`, then the host's agent | Agent for the session. |
| `--prompt` | text | none | First prompt, sent once the session starts. |
| `--json` | flag | off | Print `{"session_id","cwd"}` instead of the bare ID. |
| `--host` | URL | local host | Remote managed WebSocket URL. Requires `--token-file`. |
| `--token-file` | `FILE` | local token | Private token file of the remote host. Requires `--host`. |

Fails with "this host is too old for `nori host new`" when the host does not
advertise folder selection.

## `nori attach`

```text
nori attach [SESSION_ID] [--host URL --token-file FILE]
```

| Option | Value | Default | Effect |
|---|---|---|---|
| `SESSION_ID` | ID or ID prefix | none | Session to open. Omit it to open the session list. |
| `--host` | URL | local host | Remote managed WebSocket URL. Requires `--token-file`. |
| `--token-file` | `FILE` | `$NORI_HOME/host/token` | Private token file. Requires `--host`. |

- A full ID in any case, with or without hyphens, or a unique prefix, selects
  a session. An unknown or ambiguous prefix is an error.
- With the local host, Nori rereads the token file when authentication fails.
- The root `--agent`, `--cd`, `--add-dir`, `--yolo`, and `-c` values apply.

## `nori auth`

```text
nori auth [--json]
nori auth set-key <PROVIDER> [--json]
nori auth remove-key <PROVIDER>
nori auth prefer-api-key <AGENT> [--yes] [--json]
nori auth prefer-login <AGENT> [--json]
```

| Command | Arguments and options | Effect |
|---|---|---|
| `nori auth` | `--json`: print the full report as JSON | Show which agents are signed in and which keys are saved. |
| `set-key` | `PROVIDER`: `anthropic`, `openai`, `google`, `xai`, or `cursor`. `--json`: print `{"provider","type","store","agents"}`. | Save an API key. Reads it hidden at a terminal, else from stdin. Warns when the key prefix belongs to another provider. |
| `remove-key` | `PROVIDER` | Forget the saved key. |
| `prefer-api-key` | `AGENT`: agent slug. `--yes`: accept the billing change. `--json`: print `{"agent","prefer_api_key","api_billing_confirmed"}` or `{"error":"confirmation_required","code","message"}`. | Pass saved keys to the agent at launch. Asks once when the agent uses a subscription. Without a terminal and without `--yes`, it fails. |
| `prefer-login` | `AGENT`. `--json` as above. | Let the agent use its own login (the default). |

Credential stores and the billing rules: [auth.md](auth.md).

## `nori skillsets`

```text
nori skillsets [ARGS...]
```

Runs `nori-skillsets ARGS...`. Every argument, including `-h`, passes
through unchanged. Nori uses `nori-skillsets` from `PATH`, else
`bunx nori-skillsets` or `npx nori-skillsets` (Bun when
`NORI_MANAGED_BY_BUN` is set or `bunx` is on `PATH`). The exit code is the
child's.

## `nori completions`

```text
nori completions <SHELL>
```

`SHELL` is `bash`, `zsh`, `fish`, `powershell`, or `elvish`. Prints the
script to stdout.

## `nori sandbox`

```text
nori sandbox macos   [--full-auto] [--log-denials] [-c key=value] COMMAND...
nori sandbox linux   [--full-auto] [-c key=value] COMMAND...
nori sandbox windows [--full-auto] [-c key=value] COMMAND...
```

Visible alias: `nori debug`. Runs `COMMAND` inside a platform sandbox. This
is a debugging tool: agent sessions do not run inside it (see
[permissions.md](permissions.md)).

| Subcommand | Alias | Platform | Sandbox |
|---|---|---|---|
| `macos` | `seatbelt` | macOS | Seatbelt |
| `linux` | `landlock` | Linux | Landlock and seccomp |
| `windows` | | Windows | Restricted token |

| Option | Value | Default | Effect |
|---|---|---|---|
| `--full-auto` | flag | off | Use `workspace-write` (writes to the folder and `TMPDIR`, no network). Default: `read-only`. |
| `--log-denials` | flag | off | macOS only. Capture sandbox denials with `log stream` and print them after exit. |
| `COMMAND...` | args | | The command and its arguments. |

The command gets the environment that `shell_environment_policy` builds. The
`[sandbox_workspace_write]` table sets writable roots and network access.

## Hidden commands

| Command | Arguments | Effect |
|---|---|---|
| `nori execpolicy check` | `-p, --policy PATH` (required, repeatable), `--pretty`, `COMMAND...` (required) | Evaluate the command against the policy files and print the result as JSON. |
| `nori stdio-to-uds` | `SOCKET_PATH` | Relay stdin and stdout to a Unix domain socket. |

## Helper entry points

The `nori` binary also answers to these names and arguments. Nori puts links
with these names in a temporary directory at the front of `PATH` for its
children.

| Invocation | Effect |
|---|---|
| `argv[0]` is `apply_patch` or `applypatch` | Apply a patch and exit. |
| `argv[0]` is `codex-linux-sandbox` | Run the Linux sandbox helper. |
| `nori --codex-run-as-apply-patch PATCH` | Apply `PATCH` and exit 0, or exit 1 on failure. |

## `nori-tui`

The `nori-tui` binary is a development target in `nori-rs/tui`. Releases do
not ship it. It accepts the [global](#global-options) `-c` option and the
[TUI options](#tui-options), opens the TUI, and prints the token usage on
exit. It has no subcommands, sends no analytics, and prints no resume hint.

## Environment variables

Variables the `nori` binary reads. Variables Nori sets on agent processes are
documented in [agents.md](agents.md).

| Variable | Read by | Effect |
|---|---|---|
| `NORI_HOME` | all | Nori home folder. Default `~/.nori/cli`. |
| `NORI_HOST_STARTED_BY` | `nori host` | `startedBy` in `host.json`. Default `nori-cli`. |
| `NORI_HANDROLL_BIN` | `nori cloud` | Path of `nori-handroll`. Must name an existing file. |
| `NORI_CREDENTIALS_STORE` | `nori auth`, agent launch | `file` (default) or `keyring`. Any other value is an error. See [auth.md](auth.md). |
| `NORI_ACP_COMMAND_<SLUG>` | agent launch | Adapter command for one agent. Beats `[acp_adapters]`. See [agents.md](agents.md). |
| `NORI_SKILLSETS_COMMAND` | TUI skillset picker (skillsets v2) | Command line that launches the skillsets v2 CLI, split with shell rules. Default: `nori-skillsets` on `PATH`. Not used by `nori skillsets`. |
| `NORI_MANAGED_BY_BUN`, `NORI_MANAGED_BY_NPM` | TUI, agent launch | Set by the npm wrapper. Select the update command, the recorded install source, and `bunx` or `npx` for adapters. |
| `NORI_NO_ANALYTICS` | TUI, `exec` | `1` turns off product analytics. See `nori-monorepo: docs/analytics.md`. |
| `NORI_ANALYTICS_URL` | TUI, `exec` | Analytics endpoint. Also re-enables analytics when `CI` is set. |
| `CI` | TUI, `exec` | Any value other than empty or `0` turns off analytics unless `NORI_ANALYTICS_URL` is set. |
| `RUST_LOG` | all | Log filter for `$NORI_HOME/log/`. Default `warn,nori_tui=info,acp=info` (release builds) or `debug` (debug builds). |
| `VISUAL`, `EDITOR` | TUI | External editor for composing a prompt. `VISUAL` wins. |
| `CODEX_TUI_RECORD_SESSION` | TUI | `1`, `true`, `TRUE`, `yes`, or `YES` records a TUI event log. See [storage.md](storage.md). |
| `CODEX_TUI_SESSION_LOG_PATH` | TUI | Path of that event log. |
| `CODEX_HOME` | MCP OAuth | Folder of the MCP OAuth fallback file. Default `~/.codex`. See [Where `main` differs](#where-main-differs). |
| `CODEX_CONFIG` | Codex agent launch | Merged into the `CODEX_CONFIG` Nori passes to the Codex adapter. |
| `npm_config_cache`, `NPM_CONFIG_CACHE`, `TMPDIR` | agent launch | Locate adapters that `npx` or `bunx` installed. |
| Provider credential variables, such as `ANTHROPIC_API_KEY` and `OPENAI_API_KEY` | `nori auth` | Sign-in detection. Full list: [auth.md](auth.md). |
| `PATH` | all | Finds adapters, `nori-handroll`, `nori-skillsets`, `bunx`, and `npx`. |

Debug builds also read `NORI_SYNC_SYSTEM_INFO` and
`NORI_MOCK_INSTRUCTION_FILES`, which make TUI snapshots deterministic for
tests.

### `$NORI_HOME/.env`

At startup, before any thread starts, `nori` loads `$NORI_HOME/.env` into its
environment. Names that start with `CODEX_` (any case) are skipped. A missing
or unreadable file is ignored.

## Exit codes

| Code | When |
|---|---|
| `0` | Success, including `--help`, `--version`, and "Host already running". |
| `1` | Any error. Nori prints the error to stderr. This includes a configuration that fails to load, a bad `-c` value, `--add-dir` under `read-only`, and a rejected permission request in `nori exec`. |
| `2` | Invalid command line (unknown flag, missing argument, conflicting flags). |
| child's code | `nori skillsets`, and `nori sandbox` (`128 + signal` when a signal ends the child on Unix). A `nori skillsets` child ended by a signal gives `1`. |

## Where `main` differs

- The MCP OAuth fallback file is `$CODEX_HOME/.credentials.json` (default
  `~/.codex/.credentials.json`), not a file under `NORI_HOME`.
