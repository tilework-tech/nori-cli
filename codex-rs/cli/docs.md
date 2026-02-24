# Noridoc: nori-cli

Path: @/codex-rs/cli

### Overview

The `nori-cli` crate is the main binary that provides the `nori` command. It serves as the entry point for the interactive TUI mode with optional login management and sandbox debugging tools. The crate handles CLI argument parsing, subcommand routing, and cross-cutting concerns.

### How it fits into the larger codebase

This crate is the primary entry point that ties together the core crates:

- **Always included:** `nori-tui`, `codex-acp`, `codex-core`
- **Optional via features:** `codex-login`
- **Uses** `codex-arg0` for arg0-based dispatch (Linux sandbox embedding)

### Core Implementation

**SeatbeltCommand**: macOS sandbox testing with options:
- `--full-auto` - Network-disabled sandbox with cwd/TMPDIR write access
- `--log-denials` - Capture and print sandbox denials via `log stream`

**LandlockCommand**: Linux sandbox testing with:
- `--full-auto` - Network-disabled sandbox with cwd/TMPDIR write access

**WindowsCommand**: Windows sandbox testing with:
- `--full-auto` - Restricted token sandbox with cwd/TMPDIR write access

```rust
match subcommand {
    None => nori_tui::run_main(...),           // Interactive TUI
    Some(Subcommand::Resume(cmd)) => /* set TUI flags, run_main */ ,
    Some(Subcommand::Fork(cmd)) => /* set TUI flags, run_main */ ,
    Some(Subcommand::Login(cli)) => run_login_*(...),
    Some(Subcommand::Sandbox(args)) => debug_sandbox::run_*(...),
    Some(Subcommand::Skillsets(cmd)) => run_skillsets_command(...),
    Some(Subcommand::Completions(cmd)) => clap_complete::generate(...),
    // ... other subcommands
}
```

**ResumeCommand**: Resume a previous session via `nori resume`. Accepts:
- An optional `SESSION_ID` positional argument to resume a specific session by UUID
- `--last` to resume the most recent session
- `--all` to show all sessions (disables working directory filtering)

When no arguments are given, opens the interactive resume picker. The subcommand sets internal TUI flags (`resume_session_id`, `resume_last`, `resume_picker`, `resume_show_all`) on the `TuiCli` struct and delegates to `nori_tui::run_main()`.

**ForkCommand**: Fork (branch off from) a previous session via `nori fork <SESSION_ID>`. Accepts:
- A required `SESSION_ID` positional argument
- `--turn N` to specify the 0-based user message turn to fork at (drops that turn and everything after). Defaults to the last user message.

The subcommand sets `fork_session_id` and `fork_turn` on the `TuiCli` struct and delegates to `nori_tui::run_main()`. Fork reuses `ConversationManager::fork_conversation` -- the same codepath as TUI backtrack.

**Debug Sandbox** (`debug_sandbox.rs`): Implementation of the sandbox testing commands.

**Login** (`login.rs`, feature-gated by `login`): Authentication-related CLI functionality.

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

`handle_app_exit()` prints token usage and session hints after TUI exits, then optionally runs update actions if the user requested an upgrade. When a `conversation_id` is present, exit messages include both a resume hint (`nori resume <id>`) and a fork hint (`nori fork <id>`), each rendered in cyan when color is supported.

Created and maintained by Nori.
