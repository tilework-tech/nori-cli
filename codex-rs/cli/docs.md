# Noridoc: cli

Path: @/codex-rs/cli

### Overview

The `codex-cli` crate is the main multitool binary that provides the `nori` command. It serves as the central dispatcher routing to different modes: interactive TUI, headless exec, MCP server, app server, login management, and sandbox debugging tools. The crate handles CLI argument parsing, subcommand routing, and cross-cutting concerns like feature toggles.

### How it fits into the larger codebase

This crate is the primary entry point that ties together all other crates:

- **Always included:** `codex-tui`, `codex-acp`, `codex-core` (minimal build)
- **Optional via features:** `codex-exec`, `codex-mcp-server`, `codex-app-server`, `codex-cloud-tasks`, `codex-login`, `codex-chatgpt`
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
| `exec` | `e` | Run non-interactively | `codex-features` |
| `login` | | Manage authentication | `login` |
| `logout` | | Remove stored credentials | `login` |
| `mcp` | | Manage MCP server configurations | `mcp-server` |
| `mcp-server` | | Run as MCP server (stdio) | `mcp-server` |
| `app-server` | | Run app server (JSON-RPC stdio) | `app-server` |
| `resume` | | Resume previous session | `codex-features` |
| `apply` | `a` | Apply latest diff to working tree | `chatgpt` |
| `sandbox` | `debug` | Test sandbox enforcement | (always) |
| `cloud` | | Browse Cloud tasks | `cloud-tasks` |
| `completion` | | Generate shell completions | (always) |
| `features` | | List feature flags | `codex-features` |

**Feature Toggles (requires `codex-features`):**

The `--enable` and `--disable` flags allow runtime feature flag control:
```bash
nori --enable web_search_request --disable unified_exec
```

These translate to `-c features.<name>=true/false` config overrides. These flags are only available when the `codex-features` Cargo feature is enabled.

**Power-User CLI Flags (requires `codex-features`):**

The following CLI flags are gated behind the `codex-features` Cargo feature and are not available in minimal/white-label builds:

| Flag | Short | Description |
|------|-------|-------------|
| `--oss` | | Select local OSS model provider |
| `--local-provider` | | Specify which local provider (lmstudio/ollama) |
| `--sandbox` | `-s` | Select sandbox policy |
| `--ask-for-approval` | `-a` | Configure approval policy |
| `--full-auto` | | Sandboxed automatic execution mode |
| `--dangerously-bypass-approvals-and-sandbox` | | Skip all safety checks (dangerous) |
| `--search` | | Enable web search tool |
| `--enable` | | Enable a feature toggle |
| `--disable` | | Disable a feature toggle |

Without `codex-features`, passing these flags results in a clap parse error.

**Resume Logic (requires `codex-features`):**

`nori resume` supports three modes:
- `nori resume <SESSION_ID>`: Resume specific session
- `nori resume --last`: Resume most recent session
- `nori resume`: Show session picker

### Things to Know

**Binary Name:**

The compiled binary is named `nori` (defined in `Cargo.toml`). Help output and error messages reference `nori` as the command name. The default config location is `~/.nori/cli/config.toml`.

**Cargo Feature Flags (Compile-time):**

The CLI uses Cargo features to enable optional functionality. By default (`default = []`), only core functionality is included (TUI, ACP). Optional features can be enabled individually or via the `full` meta-feature:

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
| `oss-providers` | `codex-tui/oss-providers`, `codex-common/oss-providers` | Ollama/LM Studio local model support |
| `codex-features` | `codex-tui/codex-features` | `exec`, `resume`, `features` subcommands + power-user CLI flags + TUI `/undo`, `/compact`, `/review` |

**Feature Propagation to TUI:**

Several CLI features propagate to the TUI crate for coordinated behavior:
- `login` -> `codex-tui/login`: Enables login screens and `/login` command in TUI
- `feedback` -> `codex-tui/feedback`: Enables Sentry feedback and `/feedback` command
- `backend-client` -> `codex-tui/backend-client`: Enables cloud tasks backend
- `upstream-updates` -> `codex-tui/upstream-updates`: Uses OpenAI update system instead of Nori's
- `oss-providers` -> `codex-tui/oss-providers` -> `codex-common/oss-providers`: Enables Ollama/LM Studio local model support
- `codex-features` -> `codex-tui/codex-features`: Enables `/undo`, `/compact`, `/review` commands in TUI + power-user CLI flags

Without these features, the TUI uses Nori-specific alternatives (e.g., GitHub Discussions for feedback, GitHub releases for updates). For OSS providers, the `codex-common` crate provides stub implementations that return `None` or errors when the feature is disabled.

Build examples:
```bash
cargo build -p codex-cli                    # Minimal (TUI + ACP only, no power-user flags)
cargo build -p codex-cli --features full    # All functionality (OpenAI-compatible)
cargo build -p codex-cli --features login,mcp-server  # Selective
```

Feature-gated code uses `#[cfg(feature = "...")]` on imports, enum variants, match arms, and struct definitions in `main.rs`. Integration tests that require specific features use `required-features` in `Cargo.toml` (e.g., MCP tests require `mcp-server`).

**Sandbox Debugging:**

The `debug_sandbox` module (in `debug_sandbox/`) provides:
- `nori sandbox macos` (Seatbelt)
- `nori sandbox linux` (Landlock)
- `nori sandbox windows` (Restricted token)

These allow testing sandbox behavior without running the full TUI.

**Login Flow:**

`login.rs` implements multiple auth methods:
- `nori login`: OAuth browser-based (ChatGPT)
- `nori login --device-auth`: Device code flow
- `nori login --with-api-key`: Read API key from stdin

**Config Override Precedence:**

1. Subcommand-specific flags (highest)
2. Root-level `-c` overrides
3. `--enable`/`--disable` feature toggles (requires `codex-features`)
4. Config file (lowest)

**Process Hardening:**

The `#[ctor]` attribute applies security hardening measures at process startup in release builds via `codex_process_hardening::pre_main_hardening()`.

**WSL Path Handling:**

On non-Windows, `wsl_paths.rs` normalizes paths for WSL environments to ensure commands work correctly when the CLI is invoked from Windows but executes in WSL.

**Exit Handling:**

`handle_app_exit()` prints token usage and session resume hints after TUI exits, then optionally runs update actions if the user requested an upgrade.

Created and maintained by Nori.
