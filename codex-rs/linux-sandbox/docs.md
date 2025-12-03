# Noridoc: linux-sandbox

Path: @/codex-rs/linux-sandbox

### Overview

The `codex-linux-sandbox` crate provides Linux-specific process sandboxing using Landlock LSM and seccomp. It restricts filesystem access and system calls for commands executed by Codex, enforcing security policies during shell tool execution.

### How it fits into the larger codebase

Linux sandbox is invoked by core for sandboxed command execution:

- **Core** spawns `codex-linux-sandbox` with command arguments
- **CLI** provides `codex sandbox linux` for manual testing
- **Embedded** via arg0 dispatch for single-binary distribution

The binary can be standalone or embedded in the main `codex` executable.

### Core Implementation

**Entry Point:**

`linux_run_main.rs` is the main entry when invoked as sandbox:
1. Parses sandbox configuration from environment/args
2. Sets up Landlock rules for filesystem access
3. Applies seccomp filters
4. Executes the target command

**Landlock Implementation:**

`landlock.rs` configures filesystem access:
- Read-only paths for system directories
- Write access to workspace root
- Configurable writable paths via settings

### Things to Know

**Environment Variables:**

- `CODEX_SANDBOX=landlock`: Set on sandboxed child processes
- Configuration passed via serialized settings

**Kernel Requirements:**

Landlock requires Linux kernel 5.13+ with LSM enabled. Falls back gracefully on older kernels.

**Seccomp Filters:**

Beyond Landlock filesystem restrictions, seccomp filters block dangerous syscalls.

**Testing:**

Tests in `tests/suite/landlock.rs` verify sandbox behavior:
- File access restrictions
- Write blocking
- Network access control

Created and maintained by Nori.
