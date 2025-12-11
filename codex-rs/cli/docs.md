# Noridoc: cli

Path: @/codex-rs/cli

### Overview

The `codex-cli` crate is the main multitool binary that provides the `codex` command. It serves as the central dispatcher routing to different modes: interactive TUI, headless exec, MCP server, app server, login management, and sandbox debugging tools. The crate handles CLI argument parsing, subcommand routing, and cross-cutting concerns like feature toggles.

### How it fits into the larger codebase

This crate is the primary entry point that ties together all other crates:

- **Always included:** `codex-tui`, `codex-exec`, `codex-acp`, `codex-core` (minimal build)
- **Optional via features:** `codex-mcp-server`, `codex-app-server`, `codex-cloud-tasks`, `codex-login`, `codex-chatgpt`, `codex-responses-api-proxy`
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

| Subcommand | Alias | Description | Required Feature |
|------------|-------|-------------|------------------|
| `exec` | `e` | Run Codex non-interactively | (always) |
| `login` | | Manage authentication | `login` |
| `logout` | | Remove stored credentials | `login` |
| `mcp` | | Manage MCP server configurations | `mcp-server` |
| `mcp-server` | | Run as MCP server (stdio) | `mcp-server` |
| `app-server` | | Run app server (JSON-RPC stdio) | `app-server` |
| `resume` | | Resume previous session | (always) |
| `apply` | `a` | Apply latest Codex diff to working tree | `chatgpt` |
| `sandbox` | `debug` | Test sandbox enforcement | (always) |
| `cloud` | | Browse Codex Cloud tasks | `cloud-tasks` |
| `completion` | | Generate shell completions | (always) |
| `features` | | List feature flags | (always) |

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

**Cargo Feature Flags (Compile-time):**

The CLI uses Cargo features to enable optional functionality. By default (`default = []`), only core functionality is included (TUI, exec, ACP). Optional features can be enabled individually or via the `full` meta-feature:

| Feature | Dependencies | Enables |
|---------|--------------|---------|
| `full` | All features | Complete legacy binary |
| `app-server` | `codex-app-server` | `app-server` subcommand |
| `cloud-tasks` | `codex-cloud-tasks` | `cloud` subcommand |
| `login` | `codex-login`, `codex-tui/login` | `login`/`logout` subcommands + TUI login |
| `feedback` | `codex-tui/feedback` | Sentry feedback in TUI |
| `backend-client` | `codex-tui/backend-client` | Cloud tasks backend client |
| `upstream-updates` | `codex-tui/upstream-updates` | OpenAI update mechanism (vs Nori's) |
| `mcp-server` | `codex-mcp-server`, `codex-rmcp-client` | `mcp`, `mcp-server` subcommands |
| `chatgpt` | `codex-chatgpt` | `apply` subcommand |
| `responses-api-proxy` | `codex-responses-api-proxy` | `responses-api-proxy` subcommand |
| `oss-providers` | `codex-tui/oss-providers`, `codex-common/oss-providers` | Ollama/LM Studio local model support |

**Feature Propagation to TUI:**

Several CLI features propagate to the TUI crate for coordinated behavior:
- `login` -> `codex-tui/login`: Enables login screens and `/login` command in TUI
- `feedback` -> `codex-tui/feedback`: Enables Sentry feedback and `/feedback` command
- `backend-client` -> `codex-tui/backend-client`: Enables cloud tasks backend
- `upstream-updates` -> `codex-tui/upstream-updates`: Uses OpenAI update system instead of Nori's
- `oss-providers` -> `codex-tui/oss-providers` -> `codex-common/oss-providers`: Enables Ollama/LM Studio local model support

Without these features, the TUI uses Nori-specific alternatives (e.g., GitHub Discussions for feedback, GitHub releases for updates). For OSS providers, the `codex-common` crate provides stub implementations that return `None` or errors when the feature is disabled.

Build examples:
```bash
cargo build -p codex-cli                    # Minimal (TUI + exec + ACP only, Nori updates)
cargo build -p codex-cli --features full    # All functionality (OpenAI-compatible)
cargo build -p codex-cli --features login,mcp-server  # Selective
```

Feature-gated code uses `#[cfg(feature = "...")]` on imports, enum variants, match arms, and struct definitions in `main.rs`. Integration tests that require specific features use `required-features` in `Cargo.toml` (e.g., MCP tests require `mcp-server`).

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
