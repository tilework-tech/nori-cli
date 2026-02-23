# Noridoc: codex-process-hardening

Path: @/codex-rs/process-hardening

### Overview

The process-hardening crate performs security hardening steps early in process startup. It prevents debugging/tracing, disables core dumps, and removes potentially dangerous environment variables.

### How it fits into the larger codebase

Called via `#[ctor::ctor]` attribute to run before `main()` in `@/codex-rs/tui/`. Ensures the CLI process starts in a hardened state to protect API keys and sensitive data.

### Core Implementation

**pre_main_hardening()**: Platform-specific hardening dispatcher.

**Linux** (`pre_main_hardening_linux()`):
- `prctl(PR_SET_DUMPABLE, 0)` - Prevent ptrace attach
- `setrlimit(RLIMIT_CORE, 0)` - Disable core dumps
- Remove `LD_*` environment variables

**macOS** (`pre_main_hardening_macos()`):
- `ptrace(PT_DENY_ATTACH)` - Prevent debugger attachment
- `setrlimit(RLIMIT_CORE, 0)` - Disable core dumps
- Remove `DYLD_*` environment variables

**BSD** (`pre_main_hardening_bsd()`):
- `setrlimit(RLIMIT_CORE, 0)` - Disable core dumps
- Remove `LD_*` environment variables

**Windows** (`pre_main_hardening_windows()`): Currently a no-op placeholder.

### Things to Know

- Hardening failures cause immediate process exit with specific exit codes
- Exit codes: 5 (prctl), 6 (ptrace), 7 (rlimit)
- Removes `LD_PRELOAD` and similar library injection vectors
- MUSL-linked binaries ignore `LD_*` anyway, but clearing is defense-in-depth
- Must run before any threads are created (uses `#[ctor::ctor]`)

Created and maintained by Nori.
