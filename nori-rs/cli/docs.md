# nori-cli

Path: `nori-rs/cli`

## Purpose

`nori-cli` builds the `nori` binary. It parses the command line, resolves
configuration through `nori-config`, and sends each mode to its owner: the TUI
(`nori-tui`), the host commands (`nori host`, `nori attach`, `nori exec`),
`nori cloud` (a TUI pinned to `nori-handroll cloud-acp`), `nori auth`, and the
sandbox debug commands. It owns process-level concerns: arg0 dispatch, release
hardening, the rolling log, which standard streams each mode may touch, and
the exit message. Every command, flag, environment variable, and exit code is
owned by [`docs/commands.md`](../../docs/commands.md). Terms follow
[`GLOSSARY.md`](../../GLOSSARY.md).

## Modules

| Module | Main items | Role |
|---|---|---|
| `main.rs` | `MultitoolCli`, `Subcommand`, `cli_main`, `run_exec`, `handle_app_exit` | Parse, merge overrides, dispatch |
| `host/mod.rs` | `HostCommand`, `AttachCommand`, `run`, `attach` | `nori host [stop\|ls\|new]`, `nori attach`, discovery |
| `host/start.rs` | | Start the detached background host |
| `host/local_stdio.rs`, `host/join.rs` | `serve_stdio` | `host --stdio` and `exec --acp`: relay to the running host, or serve a private one |
| `host/stdio.rs` | | `host --stdio --launch`: one managed launch as plain ACP |
| `host/oneshot.rs` | `run_one_shot`, `OneShot` | `nori exec "prompt"` as an ACP v1 client of a host |
| `host/session_lookup.rs`, `host/table.rs` | | Resolve a session from an ID prefix; `host ls` output |
| `cloud.rs` | | Resolve `nori-handroll` (`NORI_HANDROLL_BIN`, then `PATH`); build the `nori-cloud` agent entry |
| `auth.rs`, `auth/` | `AuthCommand` | `nori auth`: status, `set-key`, `remove-key`, `prefer-api-key`, `prefer-login` |
| `stdin_prompt.rs` | `resolve_prompt`, `restore_stdin_from_terminal` | Compose a prompt from the argument and piped stdin |
| `debug_sandbox.rs`, `debug_sandbox/`, `lib.rs` | `SeatbeltCommand`, `LandlockCommand`, `WindowsCommand` | `nori sandbox macos\|linux\|windows` |
| `exit_status.rs`, `wsl_paths.rs` | | Exit with a child process status; WSL path normalization |

```text
nori argv
   |  arg0_dispatch_or_else (codex-arg0)  -> embedded Linux sandbox helper
   v
cli_main: MultitoolCli::parse, init $NORI_HOME/log/nori-acp.*
   |
   +-- (none) / resume / cloud -----> nori_tui::run_main --> OwnedHost (nori-host-runtime)
   +-- -p / exec "prompt" ----------> host::run_one_shot --> running host or in-process host
   +-- exec --acp / host --stdio ---> host::serve_stdio  --> relay or private host
   +-- host [stop|ls|new], attach --> host::run / host::attach --> nori_tui::run_joined_app
   +-- auth ------------------------> auth::run (blocking task)
   +-- sandbox, skillsets, completions, execpolicy, stdio-to-uds
```

## Dependency direction

```text
nori-cli --> nori-tui --> nori-host-runtime --> nori-harness --> nori-acp-host --> nori-protocol
    |   \--> nori-host-runtime, nori-harness, nori-config, nori-protocol
    \------> nori-installed, nori-auth-providers, nori-tui-components,
             codex-arg0, codex-common, codex-sandbox, codex-execpolicy,
             codex-process-hardening, codex-stdio-to-uds, codex-keyring-store
```

Nothing in the workspace depends on `nori-cli`. Keep logic that another
frontend could need in the crate below it; this crate stays a dispatcher.

## Invariants

- `nori exec --acp` and `nori host --stdio` own stdin as a JSON-RPC channel.
  `run_exec` returns into the host before it reads a prompt from stdin.
  Never read stdin upstream of that branch.
- Stdin is read for a prompt only when the prompt argument is absent, is `-`,
  or `--stdin` is set. A plain argument leaves stdin untouched.
- One-shot `nori host ls` uses the terminal’s default background and leaves
  queued input available to the shell.
- After the interactive path drains a pipe, it points fd 0 back at the
  terminal, so children such as `$EDITOR` get a usable stdin.
- `-p` selects a mode. It is rejected with a subcommand or `--image`.
- `nori exec` writes only the final agent text to stdout. Diagnostics go to
  stderr. A rejected permission request makes the run fail after the text is
  written.
- `nori cloud` sets the agent to `nori-cloud` after flag merging, so `--agent`
  cannot bypass Sessions. All broker logic lives in `nori-handroll`.
- An `AnalyticsReporter` is created only for routes that run an agent:
  interactive, cloud, and plaintext exec. Event names are owned by
  `nori-monorepo: docs/analytics.md`.
- Process hardening (`codex-process-hardening`) runs before `main` in release
  builds only.
- Root-level `-c` overrides are prepended, so overrides after a subcommand win.

## Tests

| What | Where |
|---|---|
| Argument parsing and override merging | `#[cfg(test)]` in `src/main.rs` |
| Host lifecycle, ACP v1 face, attach, modes, transcripts | `tests/host_lifecycle.rs`, `tests/host_lifecycle/` |
| Stdio host, managed launch and recovery | `tests/host_stdio.rs`, `tests/managed_recovery.rs`, `tests/recovery/`, `tests/server_launch/`, `tests/standard_acp/` |
| `nori exec` | `tests/headless_exec.rs`, `tests/exec_host_client.rs` |
| Host wire schema and TypeScript | `tests/host_wire_schema.rs` |
| Auth, analytics, token providers, execpolicy | `tests/auth.rs`, `tests/analytics.rs`, `tests/token_provider.rs`, `tests/execpolicy.rs` |

Run:

```sh
cd nori-rs
cargo build -p mock-acp-agent
MOCK_ACP_AGENT_BIN=$PWD/target/debug/mock_acp_agent cargo test -p nori-cli
```

`tests/headless_exec.rs` and `tests/analytics.rs` require
`MOCK_ACP_AGENT_BIN`. Other tests fall back to `mock_acp_agent` next to the
`nori` test binary. The PTY suite (`just test e2e` from the repository root)
runs the real binary.
