# Noridoc: nori-cli

Path: @/nori-rs/cli

### Overview

The `nori-cli` crate is the main binary that provides the `nori` command. It serves as the entry point for the interactive TUI mode, sandbox debugging tools, and utility subcommands. The crate handles CLI argument parsing, subcommand routing, and process-level concerns -- including how a prompt reaches the session and which of the process's standard streams each mode is allowed to touch.

### How it fits into the larger codebase

This crate is the primary entry point that ties together the core crates:

- **Always included:** `nori-tui`, `nori-harness`, `nori-config`, `codex-sandbox`, `nori-providers`
- **Uses** `codex-arg0` for arg0-based dispatch (Linux sandbox embedding)
- **Uses** `codex-sandbox` (`@/nori-rs/sandbox/`) for the `nori sandbox` debug subcommand's seatbelt/landlock/windows spawn helpers
- Owns the background [host lifecycle](src/host/docs.md), composing the
  harness session manager with an authenticated managed transport. The
  [attached frontend](../tui/src/attach/docs.md) owns only terminal state.
- Maps each agent-bearing entrypoint to the stable product analytics modes in
  [`nori-installed`](../installed/docs.md): ordinary and resumed TUI sessions
  are `interactive`, `nori cloud` is `cloud`, and plaintext execution is `exec`.
  `nori exec --acp` runs the headless host, which reports no analytics yet.

### Core Implementation

[`main.rs`](src/main.rs) constructs an `AnalyticsReporter` only for agent-bearing
TUI, cloud, and plaintext exec routes, then passes it into the owning
frontend (for plaintext exec, the host's one-shot client). Help, version, configuration-only, sandbox, and other sessionless
commands do not create a reportable session. The reporter receives the resolved
Nori home for durable opt-out state. Authenticated Firebase credentials come
from the shared user-level `~/.nori-config.json`; the CLI does not derive
PostHog identity or organization membership itself.

**SeatbeltCommand**: macOS sandbox testing with options:
- `--full-auto` - Network-disabled sandbox with cwd/TMPDIR write access
- `--log-denials` - Capture and print sandbox denials via `log stream`

**LandlockCommand**: Linux sandbox testing with:
- `--full-auto` - Network-disabled sandbox with cwd/TMPDIR write access

**WindowsCommand**: Windows sandbox testing with:
- `--full-auto` - Restricted token sandbox with cwd/TMPDIR write access

**ResumeCommand**: Starts the TUI from a saved transcript session:
- `nori resume` - Opens the startup session picker for the current working directory
- `nori resume <session-id>` - Resumes a saved session by transcript session ID
- `nori resume --last` - Resumes the newest saved session for the current working directory
- `nori resume --all` - Lets picker/last selection search all transcript projects instead of only the current working directory
- TUI flags such as `--agent`, approval-bypass options, prompts, images, `-c`, and working-directory overrides can be passed after `resume`; they are merged into the normal interactive CLI configuration before `nori_tui::run_main()`

**Host and attach:** `nori host` starts a detached Unix process owning an empty
session manager; `host ls` prints its sessions tree-ordered (the shared two-row session-list layout on a terminal, a plain one-row-per-session table when piped; see [host docs](src/host/docs.md)) `host new [--cwd DIR] [--agent AGENT] [--prompt TEXT] [--json]` starts a session in a folder (default: the caller's directory) and prints its ID, and `host stop` shuts it down. Each host session loads configuration for its own folder, so that folder's project trust applies.
The host outlives both attached terminals and its final session. `nori attach` discovers the local host and token, then opens the
session list or a supplied conversation ID. Explicit `--host` and
`--token-file` must be supplied together. Startup and attach resolve ordinary
Nori configuration without entering the local process-owning TUI `App`.
Administrative list, new, and stop use host discovery directly, so a later malformed
configuration file cannot prevent shutdown.
Without `--launch`, `host --stdio` runs a headless local host for one client on stdin and stdout; see [local host on stdio](src/host/docs.md#local-host-on-stdio).
On a dedicated managed machine, `host --stdio --launch <FILE> [--inherit-env]`
runs the host in the foreground and serves one pinned adapter as plain ACP on
stdin and stdout until stdin closes; see [managed launch on stdio](src/host/docs.md#managed-launch-on-stdio).
See [the command and wire contract](../../docs/remote-control.md#11-background-host-and-attached-terminals).

**ExecCommand**: Provides two terminal-independent execution surfaces:
- `nori exec [PROMPT]` runs one prompt through the selected ACP agent and writes only the complete assistant text to stdout. The prompt may come from the positional argument, from piped stdin, or from both (see **Prompt Sources**). Diagnostics and failures remain on stderr so the answer can be piped or redirected directly. It is an ACP v1 client of `nori host`: `run_exec` resolves the prompt and configuration, then calls the host's [one-shot client](src/host/docs.md#one-shot-exec), which uses the home's running host when it can and an in-process host otherwise. A run with `-c` overrides or `--add-dir` always gets its own host, because a running host applies its own configuration.
- `nori exec --acp` is an alias for `nori host --stdio`: a headless local host serving the host's standard ACP face (any number of sessions, plus `_nori/*`) to one client on stdin and stdout, with no lock or discovery files. Dispatch loads configuration with the exec overrides and calls the host's `serve_stdio`; see [Local host on stdio](src/host/docs.md#local-host-on-stdio).
- `nori -p` / `--print` is a top-level boolean flag rather than a subcommand, matching the flag other agent CLIs use to select non-interactive output. It synthesizes an `ExecCommand` from the already-parsed interactive flags and routes into the same plaintext path, so it is an alias in behavior and not a second implementation. It cannot serve ACP on stdio; that stays behind the explicit `exec --acp`.
- Both modes use the same resolved configuration and `nori-harness` session manager (through the host) as the TUI. `--agent`, `--cwd`, and raw `-c` overrides remain available without initializing Ratatui.
- The explicit `--dangerously-bypass-approvals-and-sandbox` flag is the only unattended auto-approval path and applies only to permission boundaries visible through ACP.

**Prompt Sources** (`stdin_prompt.rs`): A prompt can arrive as a positional argument, on piped stdin, or as both. Resolution is shared by the interactive and non-interactive entry points so the two never drift:
- **Stdin is consumed only when the caller asked for it**: the prompt argument is absent, the argument is the `-` sentinel, or `--stdin` was passed. A plain prompt argument leaves stdin untouched. This is a correctness requirement rather than ergonomics -- a process inherits its parent's stdin, so a prompt argument that also drained stdin would let `nori exec "..."` swallow a `while read` loop's input or block until an unrelated pipe closed.
- When both sources are present they compose into a single prompt: the argument is the instruction and the piped text is the context it operates on, joined instruction-first with a blank line between, so `git diff | nori --stdin "review this"` reads the way it looks. Line endings are normalized and a source that carried only whitespace is treated as absent, so composition never emits stray separators. The read is capped so an unbounded producer is rejected rather than buffered until the process dies.
- The interactive path re-points file descriptor 0 at the controlling terminal once it has drained a pipe. Children the TUI spawns with inherited stdin (`$EDITOR`, the file browser) would otherwise receive an EOF'd pipe and exit immediately; repairing the descriptor once covers all of them.
- `-p` selects a mode, so it is rejected alongside a subcommand or `--image` rather than silently ignoring either.
- The two modes differ only in what "no prompt from any source" means. `exec` (and `-p`) requires one and fails otherwise; interactive treats it as an ordinary empty session.
- **Stdin is read only after `exec --acp` has taken its early return into the host.** The stdio host speaks JSON-RPC over stdin and must keep exclusive ownership of it, so the CLI must never ingest stdin during dispatch or anywhere upstream of the mode decision. This ordering is the load-bearing constraint of the piped-prompt path; violating it silently corrupts the ACP protocol rather than failing loudly.
- Piping into bare `nori` does not select headless behavior. The piped text seeds the first turn and the TUI starts normally, which requires only that a controlling terminal is reachable for key input (see `@/nori-rs/tui/docs.md`). Environments with no terminal at all must use `exec` or `-p`.

**CloudCommand** (`cloud.rs`): Runs a TUI session backed by Nori Sessions by delegating everything cloud-related to the external `nori-handroll` binary. The cloud route contains no broker client, OAuth flow, or cloud WebSocket transport; it only translates the broker configuration passed to Handroll:
- `resolve_handroll_bin()` resolves the `nori-handroll` binary. A `NORI_HANDROLL_BIN` env override wins when set and must point at an existing file (a dangling override is an error, not a fallback); otherwise the first `nori-handroll` on `PATH` is used. A missing binary fails with an actionable "install Nori Sessions" error before the TUI starts
- `cloud_agent_config()` builds a synthetic registry entry (slug `nori-cloud`): a local distribution running `<handroll-bin> cloud-acp`, with the read-only `[cloud] broker_url` from `config.toml` (when present) translated to a `NORI_BROKER_URL` environment variable on the child, and an auth hint pointing at `nori-handroll login`. When the `--onboard` flag is set, `--onboard` is appended to the spawned `cloud-acp` argv so the handroll child acquires-or-resumes the org's broker-side onboarding session server-side
- The dispatch in `main.rs` forces `interactive.agent = "nori-cloud"` AFTER flag merging, so `--agent` cannot bypass Sessions, passes the entry via the clap-skipped `TuiCli.extra_agents` field, and sets the clap-skipped `TuiCli.cloud_mode` launch-origin flag (see `@/nori-rs/tui/src/cli.rs`). Cloud entry is picker-first: the TUI prepares one live agent connection, lists sessions when advertised, and opens the session picker before anything can claim a VM, with "Start a new session" as an explicit pick. The chosen new or existing session is activated on that prepared connection rather than a second child (see `@/nori-rs/tui/docs.md`)
- `nori cloud --onboard` is the CLI half of customer onboarding (Part 2, after the hosted checkout instructions at norisessions.com/onboard-checkout.md have provisioned the org): it additionally sets the clap-skipped `TuiCli.cloud_onboard` flag. The TUI skips the picker but still prepares one connection and lists sessions when advertised; when the broker-tagged onboarding session is present, it dispatches the ordinary ACP resume action so the harness selects `session/load` and replays recorded history. If the purpose tag is absent or listing is unsupported, it explicitly starts a fresh onboarding session on the same connection, preserving the broker's serialized acquire-or-resume behavior and compatibility with older handroll/broker versions. Preparation or advertised-list failure stays distinct and is shown to the user. `/new` repeats this prepare-first choice. The onboarding argv remains baked into the process-wide registry entry, so every fallback acquisition is onboarding-only; `/close` remains picker-first
- From there the handroll child rides the ordinary local-agent path end to end: registry lookup, `AcpConnection::spawn()`, and unconditional local transcript recording (duplicating the broker's server-side recording is intentional)
- Auth, broker REST, session acquisition/release, and tunnel transport all live inside `nori-handroll cloud-acp`. Quitting the TUI is a detach: the graceful stdin-EOF shutdown contract in `@/nori-rs/acp-host/src/connection/acp_connection.rs` lets the child detach cleanly, and the broker session keeps running for later reattach -- only ACP `session/close` releases it
- ACP capabilities select supported operations and the resume strategy: handroll can advertise `sessionCapabilities.{list,resume,close}` and `loadSession`, serve recorded history through `session/load`, and release via `session/close`. They do not identify the deployment because handroll is a synthetic agent facade. The explicit `cloud_mode` launch fact instead governs cloud identity, command scope, picker behavior, and detach/reattach wording. The one-active-session contract (close before claiming another) remains agent-side. See `@/nori-rs/harness/docs.md` (resume strategy and failure handling) and `@/nori-rs/tui/docs.md` (cloud-mode presentation and lifecycle)
- TUI flags such as `--agent` and approval-bypass options can still be passed after `cloud` (only `--agent` is overridden)

**AuthCommand** ([`auth.rs`](src/auth.rs), with [`table.rs`](src/auth/table.rs), [`key_input.rs`](src/auth/key_input.rs), and [`prefer.rs`](src/auth/prefer.rs)): Reports per-agent sign-in status, manages API keys saved per model provider through Nori, and records each agent's choice between its own login and a saved key, backed by [`nori-providers`](../providers/docs.md) and the `[auth.<agent>]` table in [`nori-config`](../nori-config/docs.md). It runs on a blocking task because detection, keyring access, and terminal input are synchronous. It is the stable interface the Nori Native app uses, by running the bundled `nori` binary as a subprocess. The subcommand is `auth`; the backing crate keeps the `nori-providers` name.
- `nori auth` prints an `AGENT  STATUS  CREDENTIAL  SOURCE` table showing the credential each built-in agent will use (or its sign-in hint), how to disconnect read-only credentials, every notice as a `note:` line, and which providers have saved keys; `--json` prints the full `AuthReport` (`store`, `providers`, `agents`, with every credential in precedence order and each agent's `prefer_api_key`). Saved keys appear as an agent's credential only when that agent prefers API keys; otherwise the agent gets a `saved_key_not_used` notice. When the store cannot be read (legacy `saved-keys.json` present, corrupt `credentials.json`, keyring unavailable) the report still runs as if nothing were saved and carries a top-level `store_error` string in JSON, or a stderr warning in the table. Detection reads only the ambient environment, so the report does not reflect an agent's config `env`, `launch.env_remove`, or token-provider binding the way launch does.
- `nori auth set-key <provider> [--json]` (`anthropic`, `openai`, `google`, `xai`, `cursor`) never takes the key on argv. At a terminal it enters crossterm raw mode with bracketed paste *before* printing the prompt, so the terminal never echoes the key; it redraws only the key's known public prefix plus `*` for each other character (Enter or Ctrl-D accepts, Esc or Ctrl-C cancels, Ctrl-U clears). A guard restores the terminal on any exit. Piped stdin is read silently. The key is saved in `$NORI_HOME/credentials.json`, or the OS keyring when `NORI_CREDENTIALS_STORE=keyring`. The confirmation shows only `prefix****` and the agents that would receive the key once they prefer API keys; `--json` prints `{provider, type, store, agents}`. A key whose prefix names another issuer (for example an `sk-ant-` key given to `set-key openai`) prints a stderr warning but is still saved. An agent slug or alias is refused with the provider to use.
- `nori auth remove-key <provider>` deletes the saved credential. `set-key` and `remove-key` fail outright when the store is unreadable, and may fail on NFS-mounted `$NORI_HOME` (see the locking note in [`nori-providers`](../providers/docs.md)). Unknown provider input is never echoed back, since it may be a pasted key.
- `nori auth prefer-api-key <agent> [--yes] [--json]` writes `[auth.<agent>] prefer_api_key = true` through `NoriConfigEdits`. A provider id is refused with the agents that use it, and an agent with no saved-key routes (Hermes) is refused. It first runs `detect_local_agent`; when `api_key_billing_warning()` reports a subscription and `api_billing_confirmed` is not yet set, confirmation is required:

  | Situation | Behavior |
  | --- | --- |
  | `--yes` | Accepted without asking |
  | TTY, no `--json` | Prints a red `WARNING: THIS CHANGES WHO YOU ARE BILLED` banner to stderr and requires typing `yes`; anything else keeps the agent on its own login and exits 0 without writing |
  | `--json` or no TTY | Fails with non-zero exit; with `--json`, stdout is `{"error":"confirmation_required","code":"api_key_overrides_subscription","message":...}` |

  Once confirmed, `api_billing_confirmed = true` is written alongside, so later opt-ins for that agent do not ask again. Success with `--json` prints `{agent, prefer_api_key, api_billing_confirmed}`; text output notes when no key is saved yet. The expected Native flow is: run with `--json`, show the message on `confirmation_required`, then rerun with `--yes`.
- `nori auth prefer-login <agent> [--json]` writes `prefer_api_key = false` and leaves `api_billing_confirmed` untouched. Both preference changes apply from the agent's next launch.
- The command prints only variable names, file paths, keychain service names, and masked keys, never full key values.

**Debug Sandbox** (`debug_sandbox.rs`): Implementation of the sandbox testing commands.

### Things to Know

**Binary Name:**

The compiled binary is named `nori` (defined in `Cargo.toml`). Help output and error messages reference `nori` as the command name. The canonical config location is `$NORI_HOME/config.toml`, defaulting to `~/.nori/cli/config.toml`.

**Skillsets Alias:**

The `skillsets` subcommand is an alias that delegates to the `nori-skillsets` package:
- First checks if `nori-skillsets` is available in PATH (via `which::which`)
- If found in PATH, runs it directly
- If not in PATH, falls back to `npx nori-skillsets` or `bunx nori-skillsets` based on `detect_preferred_package_manager()`
- Passes through all arguments, stdout, stderr, and exit code

**Shell Completions:**

The `completions` subcommand generates shell-specific tab-completion scripts via `clap_complete::generate()`. It takes a required shell argument (bash, zsh, fish, powershell, elvish) and writes the completion script to stdout. Users redirect the output to their shell's completions directory (e.g., `nori completions bash > ~/.bash_completion.d/nori`). This subcommand is visible in `nori --help`.

**Sandbox Debugging:**

The `debug_sandbox` module (in `debug_sandbox/`) provides:
- `nori sandbox macos` (Seatbelt)
- `nori sandbox linux` (Landlock)
- `nori sandbox windows` (Restricted token)

These allow testing sandbox behavior without running the full TUI. All commands accept trailing arguments as the command to sandbox, and `--full-auto` provides sensible defaults. On macOS, `--log-denials` requires elevated permissions for log streaming.

**Config Override Precedence:**

1. Typed CLI flags for agent, sandbox, approval, cwd, and writable roots (highest)
2. Raw `-c key=value` overrides
3. `$NORI_HOME/config.toml` (lowest)

The CLI resolves this stack through `nori-config` and passes the resulting `NoriConfig` into the TUI and harness. It does not load or translate a second `codex-core` configuration. Codex-style `--profile`, `profile`, `[profiles]`, and the legacy `model` key are intentionally unsupported; use `agent` for the agent selection and Nori Skillsets for reusable agent behavior.

Authentication remains available inside the TUI through `/login`. The standalone top-level `nori login` and `nori logout` commands were removed so the CLI has one interactive authentication surface.

**Headless Approval Behavior:**

Plaintext execution cannot wait for an interactive answer. By default, Nori selects the first reject option offered by the ACP agent, or cancels the request if no reject option exists. The agent may then explain or recover, and that final text is still written to stdout, but the process exits unsuccessfully so automation cannot mistake partial work for an approved completion. In ACP facade mode, permission requests are forwarded to the caller as standard `session/request_permission` requests and the correlated response is relayed to the downstream agent. Caller EOF cancels outstanding work and shuts down the downstream session.

For `nori resume`, subcommand-scoped interactive flags are copied into the same `TuiCli` structure used by a fresh interactive launch. If both root-level and resume-scoped flags are present, the resume-scoped flag wins for that field while preserving unrelated root-level settings.

**Startup Resume:**

`nori resume` is the top-level counterpart to the in-TUI `/resume` command. It resolves saved sessions through Nori's transcript metadata instead of external provider rollout files:
- `nori resume <session-id>` searches all transcript projects by session ID.
- `nori resume --last` selects the newest saved session, scoped to the current working directory unless `--all` is present.
- `nori resume` without an ID opens a picker, scoped to the current working directory unless `--all` is present.
- If the saved session metadata records an agent, that recorded agent is used automatically. Passing a different `--agent` is a startup error so the command never resumes a session with the wrong agent.

**Process Hardening:**

The `#[ctor]` attribute applies security hardening measures at process startup in release builds via `codex_process_hardening::pre_main_hardening()`.

**WSL Path Handling:**

On non-Windows, `wsl_paths.rs` normalizes paths for WSL environments to ensure commands work correctly when the CLI is invoked from Windows but executes in WSL.

**Exit Handling:**

`handle_app_exit()` prints token usage when available and prints a copyable two-line resume hint for sessions that recorded activity. The lead line ends with `run:` and the next line contains only `nori resume <session-id>` so the command can be copied without surrounding output. It then optionally runs update actions if the user requested an upgrade.

Created and maintained by Nori.
