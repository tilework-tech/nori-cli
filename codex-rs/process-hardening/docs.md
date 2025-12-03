# Noridoc: process-hardening

Path: @/codex-rs/process-hardening

### Overview

The `codex-process-hardening` crate applies security hardening measures at process startup. It's invoked via `#[ctor]` in release builds to apply protections before main() runs.

### How it fits into the larger codebase

Process hardening is used by CLI for security:

- **CLI** uses `#[ctor]` to call `pre_main_hardening()`
- **Only in release builds** (skipped in debug for easier development)
- **Applies** platform-specific protections

### Core Implementation

`pre_main_hardening()` applies:
- Stack protections
- Memory protections
- Platform-specific security features

### Things to Know

**Timing:**

Runs before main() via the `ctor` attribute, ensuring protections are active for entire process lifetime.

**Debug Builds:**

Disabled in debug builds to avoid interfering with development and debugging tools.

Created and maintained by Nori.
