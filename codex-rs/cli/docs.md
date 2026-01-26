# Noridoc: cli

Path: @/codex-rs/cli

### Overview

The `nori-cli` crate is the main binary that provides the `nori` command. It serves as the entry point for the interactive TUI mode with optional login management and sandbox debugging tools. The crate handles CLI argument parsing, subcommand routing, and cross-cutting concerns.

### How it fits into the larger codebase

This crate is the primary entry point that ties together the core crates:

- **Always included:** `nori-tui`, `codex-acp`, `codex-core`
- **Optional via features:** `codex-login`
- **Uses** `codex-arg0` for arg0-based dispatch (Linux sandbox embedding)

### Core Implementation

**Main Entry:**

`main.rs` parses CLI using `clap` and routes based on subcommand:

```rust
match subcommand {
    None => nori_tui::run_main(...),           // Interactive TUI
    Some(Subcommand::Login(cli)) => run_login_*(...),
    Some(Subcommand::Sandbox(args)) => debug_sandbox::run_*(...),
    // ... other subcommands
}
```

**Subcommands:**

| Subcommand | Alias | Description | Required Feature |
|------------|-------|-------------|------------------|
| `login` | | Manage authentication | `login` |
| `logout` | | Remove stored credentials | `login` |
| `sandbox` | `debug` | Test sandbox enforcement | (always) |
| `execpolicy` | | Execpolicy tooling (hidden) | (always) |
| `stdio-to-uds` | | Internal stdio relay (hidden) | (always) |

**Always-Available Safety Override:**

The `--dangerously-bypass-approvals-and-sandbox` flag (alias: `--yolo`) is available in all builds. When set, it configures `approval_policy: AskForApproval::Never`, causing the ACP backend to auto-approve all tool operations without prompting the user.

### Things to Know

**Binary Name:**

The compiled binary is named `nori` (defined in `Cargo.toml`). Help output and error messages reference `nori` as the command name. The default config location is `~/.nori/cli/config.toml`.

**Cargo Feature Flags (Compile-time):**

The CLI uses Cargo features to enable optional functionality. By default (`default = []`), only core functionality is included (TUI + ACP).

| Feature | Dependencies | Enables |
|---------|--------------|---------|
| `login` | `codex-login`, `nori-tui/login` | `login`/`logout` subcommands + TUI login |

**Feature Propagation to TUI:**

The `login` feature propagates to the TUI crate for coordinated behavior:
- `login` -> `nori-tui/login`: Enables login screens and `/login` command in TUI

Build examples:
```bash
cargo build -p nori-cli                    # Minimal (TUI + ACP only)
cargo build -p nori-cli --features login   # With login support
```

Feature-gated code uses `#[cfg(feature = "...")]` on imports, enum variants, match arms, and struct definitions in `main.rs`.

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
3. Config file (lowest)

**Process Hardening:**

The `#[ctor]` attribute applies security hardening measures at process startup in release builds via `codex_process_hardening::pre_main_hardening()`.

**WSL Path Handling:**

On non-Windows, `wsl_paths.rs` normalizes paths for WSL environments to ensure commands work correctly when the CLI is invoked from Windows but executes in WSL.

**Exit Handling:**

`handle_app_exit()` prints token usage and session resume hints after TUI exits, then optionally runs update actions if the user requested an upgrade.

Created and maintained by Nori.
