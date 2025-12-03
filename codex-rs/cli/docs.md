# Noridoc: cli

Path: @/codex-rs/cli

### Overview

The `codex-cli` crate is the main multitool binary that provides the `codex` command. It serves as the central dispatcher routing to different modes: interactive TUI, headless exec, MCP server, app server, login management, and sandbox debugging tools. The crate handles CLI argument parsing, subcommand routing, and cross-cutting concerns like feature toggles.

### How it fits into the larger codebase

This crate is the primary entry point that ties together all other crates:

- **Dispatches to** `codex-tui` for interactive mode (default, no subcommand)
- **Dispatches to** `codex-exec` for `codex exec` non-interactive execution
- **Dispatches to** `codex-mcp-server` for `codex mcp-server`
- **Dispatches to** `codex-app-server` for `codex app-server`
- **Dispatches to** `codex-cloud-tasks` for `codex cloud` browsing
- **Uses** `codex-login` for authentication flows
- **Uses** `codex-chatgpt` for the `codex apply` command
- **Uses** `codex-arg0` for arg0-based dispatch (Linux sandbox embedding)

### Core Implementation

**Main Entry:**

`main.rs` parses CLI using `clap` and routes based on subcommand:

```rust
match subcommand {
    None => codex_tui::run_main(...),           // Interactive
    Some(Subcommand::Exec(cli)) => codex_exec::run_main(...),
    Some(Subcommand::McpServer) => codex_mcp_server::run_main(...),
    Some(Subcommand::Login(cli)) => run_login_*(...),
    Some(Subcommand::Sandbox(args)) => debug_sandbox::run_*(...),
    // ... other subcommands
}
```

**Subcommands:**

| Subcommand | Alias | Description |
|------------|-------|-------------|
| `exec` | `e` | Run Codex non-interactively |
| `login` | | Manage authentication |
| `logout` | | Remove stored credentials |
| `mcp` | | Manage MCP server configurations |
| `mcp-server` | | Run as MCP server (stdio) |
| `app-server` | | Run app server (JSON-RPC stdio) |
| `resume` | | Resume previous session |
| `apply` | `a` | Apply latest Codex diff to working tree |
| `sandbox` | `debug` | Test sandbox enforcement |
| `cloud` | | Browse Codex Cloud tasks |
| `completion` | | Generate shell completions |
| `features` | | List feature flags |

**Feature Toggles:**

The `--enable` and `--disable` flags allow runtime feature flag control:
```bash
codex --enable web_search_request --disable unified_exec
```

These translate to `-c features.<name>=true/false` config overrides.

**Resume Logic:**

`codex resume` supports three modes:
- `codex resume <SESSION_ID>`: Resume specific session
- `codex resume --last`: Resume most recent session
- `codex resume`: Show session picker

### Things to Know

**Sandbox Debugging:**

The `debug_sandbox` module (in `debug_sandbox/`) provides:
- `codex sandbox macos` (Seatbelt)
- `codex sandbox linux` (Landlock)
- `codex sandbox windows` (Restricted token)

These allow testing sandbox behavior without running full Codex.

**Login Flow:**

`login.rs` implements multiple auth methods:
- `codex login`: OAuth browser-based (ChatGPT)
- `codex login --device-auth`: Device code flow
- `codex login --with-api-key`: Read API key from stdin

**Config Override Precedence:**

1. Subcommand-specific flags (highest)
2. Root-level `-c` overrides
3. `--enable`/`--disable` feature toggles
4. Config file (lowest)

**Process Hardening:**

The `#[ctor]` attribute applies security hardening measures at process startup in release builds via `codex_process_hardening::pre_main_hardening()`.

**WSL Path Handling:**

On non-Windows, `wsl_paths.rs` normalizes paths for WSL environments to ensure commands work correctly when Codex is invoked from Windows but executes in WSL.

**Exit Handling:**

`handle_app_exit()` prints token usage and session resume hints after TUI exits, then optionally runs update actions if the user requested an upgrade.

Created and maintained by Nori.
