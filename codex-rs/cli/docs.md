# Noridoc: nori-cli

Path: @/codex-rs/cli

### Overview

The cli crate provides debug utilities and CLI argument types for sandbox testing. It exposes commands for directly testing the platform-specific sandboxing mechanisms (Seatbelt on macOS, Landlock on Linux, restricted tokens on Windows).

### How it fits into the larger codebase

This crate provides development and debugging utilities. The sandbox commands are useful for:
- Testing sandbox policies in isolation
- Debugging sandbox denial issues
- Verifying that commands work correctly under restriction

### Core Implementation

**SeatbeltCommand**: macOS sandbox testing with options:
- `--full-auto` - Network-disabled sandbox with cwd/TMPDIR write access
- `--log-denials` - Capture and print sandbox denials via `log stream`

**LandlockCommand**: Linux sandbox testing with:
- `--full-auto` - Network-disabled sandbox with cwd/TMPDIR write access

**WindowsCommand**: Windows sandbox testing with:
- `--full-auto` - Restricted token sandbox with cwd/TMPDIR write access

**Debug Sandbox** (`debug_sandbox.rs`): Implementation of the sandbox testing commands.

**Login** (`login.rs`, feature-gated by `login`): Authentication-related CLI functionality.

### Things to Know

- All commands accept trailing arguments as the command to sandbox
- The `--full-auto` flag provides a sensible default for most use cases
- On macOS, `--log-denials` requires elevated permissions for log streaming
- Commands use `CliConfigOverrides` for consistency with main CLI

Created and maintained by Nori.
