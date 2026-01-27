# Noridoc: codex-arg0

Path: @/codex-rs/arg0

### Overview

The arg0 crate implements the "argv[0] trick" for multi-binary dispatch. It allows a single executable to behave as different tools depending on how it was invoked, enabling deployment of multiple CLIs as one binary.

### How it fits into the larger codebase

Used as the entry point wrapper for `@/codex-rs/tui/` (the `nori` binary). Before the TUI starts, this crate checks if the binary was invoked as `codex-linux-sandbox` or `apply_patch` and dispatches accordingly.

### Core Implementation

**arg0_dispatch()**: Checks argv[0] and dispatches to:
- `codex-linux-sandbox` -> Calls `codex_linux_sandbox::run_main()` (never returns)
- `apply_patch` / `applypatch` -> Calls `codex_apply_patch::main()` (never returns)
- Otherwise -> Loads `.env`, updates PATH, returns to caller

**arg0_dispatch_or_else()**: Wraps the dispatch with Tokio runtime setup and executes the provided async main function if not dispatched.

**PATH Setup** (`prepend_path_entry_for_codex_aliases()`): Creates a temp directory with symlinks (Unix) or batch scripts (Windows) for `apply_patch` and adds it to PATH, making the tool available to child processes.

**Dotenv Loading** (`load_dotenv()`): Loads environment variables from `~/.codex/.env`, filtering out any `CODEX_` prefixed variables for security.

### Things to Know

- PATH modification happens before Tokio runtime creation (single-threaded requirement)
- The temp directory with aliases persists for the process lifetime
- `CODEX_` prefixed env vars in `.env` are silently ignored for security
- On Windows, batch scripts invoke the exe with a special `--codex-run-as-apply-patch` flag
- Symlinks on Unix point back to the current executable

Created and maintained by Nori.
