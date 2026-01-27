# Noridoc: codex-linux-sandbox

Path: @/codex-rs/linux-sandbox

### Overview

The linux-sandbox crate provides a Landlock-based sandboxing binary for Linux. It wraps command execution with filesystem and network restrictions using the Linux Security Module (LSM) framework.

### How it fits into the larger codebase

Used by `@/codex-rs/core/` (`exec.rs`) as the sandbox executor on Linux. The crate produces a `codex-linux-sandbox` binary that is exec'd to run commands in a restricted environment.

### Core Implementation

**Landlock Setup** (`landlock.rs`): Configures Landlock rules for:
- Read-only paths (system directories, project files)
- Read-write paths (working directory, temp files)
- Network access restrictions (optional)

**Main Entry** (`linux_run_main.rs`): The `run_main()` function:
1. Parses sandbox configuration from environment or arguments
2. Sets up Landlock ruleset
3. Exec's the target command with restrictions applied

**Platform Check**: On non-Linux platforms, `run_main()` panics with a clear message.

### Things to Know

- Requires Linux kernel 5.13+ with Landlock support
- Falls back gracefully on older kernels with reduced security
- Environment variables pass sandbox configuration to avoid arg parsing complexity
- The binary is typically invoked by the core crate, not directly by users

Created and maintained by Nori.
