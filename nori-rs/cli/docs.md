# Noridoc: nori-cli

Path: @/nori-rs/cli

### Overview

The `nori-cli` crate is the main binary that provides the `nori` command. It serves as the entry point for the interactive TUI mode, sandbox debugging tools, and utility subcommands. The crate handles CLI argument parsing, subcommand routing, and process-level concerns.

### How it fits into the larger codebase

This crate is the primary entry point that ties together the core crates:

- **Always included:** `nori-tui`, `nori-exec`, `nori-harness`, `nori-config`, `codex-sandbox`
- **Uses** `codex-arg0` for arg0-based dispatch (Linux sandbox embedding)
- **Uses** `codex-sandbox` (`@/nori-rs/sandbox/`) for the `nori sandbox` debug subcommand's seatbelt/landlock/windows spawn helpers

### Core Implementation

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

```rust
match subcommand {
    None => nori_tui::run_main(...),           // Interactive TUI
    Some(Subcommand::Resume(cmd)) => nori_tui::run_main(...),
    Some(Subcommand::Cloud(cmd)) => nori_tui::run_main(...),  // Pinned nori-handroll agent
    Some(Subcommand::Exec(cmd)) => run_exec(cmd),
    Some(Subcommand::Sandbox(args)) => debug_sandbox::run_*(...),
    Some(Subcommand::Skillsets(cmd)) => run_skillsets_command(...),
    Some(Subcommand::Completions(cmd)) => clap_complete::generate(...),
    // ... other subcommands
}
```

**ExecCommand**: Provides two terminal-independent execution surfaces:
- `nori exec [PROMPT]` runs one prompt through the selected ACP agent and writes only the complete assistant text to stdout. If the positional prompt is omitted, the command reads it from stdin. Diagnostics and failures remain on stderr so the answer can be piped or redirected directly.
- `nori exec --acp` serves Nori itself as a bounded ACP agent over stdio. The caller uses standard ACP JSON-RPC methods and notifications; Nori does not add a JSONL envelope or a second event schema.
- Both modes use the same resolved configuration and `nori-harness` runtime as the TUI. `--agent`, `--cwd`, and raw `-c` overrides remain available without initializing Ratatui.
- The explicit `--dangerously-bypass-approvals-and-sandbox` flag is the only unattended auto-approval path and applies only to permission boundaries visible through ACP.

**CloudCommand** (`cloud.rs`): Runs a TUI session backed by Nori Sessions by delegating everything cloud-related to the external `nori-handroll` binary (from the nori-sessions repo). The CLI no longer contains any broker client, OAuth flow, or WebSocket transport -- it does not know the word "broker" beyond translating one config value:
- `resolve_handroll_bin()` resolves the `nori-handroll` binary. A `NORI_HANDROLL_BIN` env override wins when set and must point at an existing file (a dangling override is an error, not a fallback); otherwise the first `nori-handroll` on `PATH` is used. A missing binary fails with an actionable "install Nori Sessions" error before the TUI starts
- `cloud_agent_config()` builds a synthetic registry entry (slug `nori-cloud`): a local distribution running `<handroll-bin> cloud-acp`, with the read-only `[cloud] broker_url` from `config.toml` (when present) translated to a `NORI_BROKER_URL` environment variable on the child, and an auth hint pointing at `nori-handroll login`. When the `--onboard` flag is set, `--onboard` is appended to the spawned `cloud-acp` argv so the handroll child acquires-or-resumes the org's broker-side onboarding session server-side
- The dispatch in `main.rs` forces `interactive.agent = "nori-cloud"` AFTER flag merging, so `--agent` cannot bypass Sessions, passes the entry via the clap-skipped `TuiCli.extra_agents` field, and sets the clap-skipped `TuiCli.cloud_mode` launch-origin flag (see `@/nori-rs/tui/src/cli.rs`). Cloud entry is picker-first: the TUI probes the agent's `session/list` and opens the session picker before anything can claim a VM, with "Start a new session" as an explicit pick (see `@/nori-rs/tui/docs.md`)
- `nori cloud --onboard` is the CLI half of customer onboarding (Part 2, after the hosted checkout instructions at norisessions.com/onboard-checkout.md have provisioned the org): it additionally sets the clap-skipped `TuiCli.cloud_onboard` flag, which makes the TUI skip the picker-first entry and spawn straight into the onboarding session, where the seeded onboarding-sessions skill leads the conversation. The broker serializes onboarding acquires and resumes the active onboarding session, so re-running the command reattaches rather than claiming a second VM
- From there the handroll child rides the ordinary local-agent path end to end: registry lookup, `AcpConnection::spawn()`, and unconditional local transcript recording (duplicating the broker's server-side recording is intentional)
- Auth, broker REST, session acquisition/release, and tunnel transport all live inside `nori-handroll cloud-acp`. Quitting the TUI is a detach: the graceful stdin-EOF shutdown contract in `@/nori-rs/acp-host/src/connection/acp_connection.rs` lets the child detach cleanly, and the broker session keeps running for later reattach -- only ACP `session/close` releases it
- ACP capabilities select supported operations and the resume strategy: handroll can advertise `sessionCapabilities.{list,resume,close}` and `loadSession`, serve recorded history through `session/load`, and release via `session/close`. They do not identify the deployment because handroll is a synthetic agent facade. The explicit `cloud_mode` launch fact instead governs cloud identity, command scope, picker behavior, and detach/reattach wording. The one-active-session contract (close before claiming another) remains agent-side. See `@/nori-rs/harness/docs.md` (resume strategy and failure handling) and `@/nori-rs/tui/docs.md` (cloud-mode presentation and lifecycle)
- TUI flags such as `--agent` and approval-bypass options can still be passed after `cloud` (only `--agent` is overridden)

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
