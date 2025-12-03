# Noridoc: arg0

Path: @/codex-rs/arg0

### Overview

The `codex-arg0` crate provides argv[0]-based dispatch for embedding multiple binaries in a single executable. This enables the Linux sandbox binary to be included within the main Codex binary, invoked by renaming or symlink.

### How it fits into the larger codebase

Arg0 is used by CLI for single-binary distribution:

- **CLI** `main.rs` calls `arg0_dispatch_or_else()`
- **Enables** `codex-linux-sandbox` to be embedded
- **Simplifies** distribution (one binary instead of two)

### Core Implementation

`arg0_dispatch_or_else()` checks argv[0]:
- If matches a known embedded binary name, dispatch to it
- Otherwise, run the main CLI logic
- Returns the path to sandbox executable for core to use

### Things to Know

**How It Works:**

When installed, a symlink like `codex-linux-sandbox -> codex` can be created. When invoked as `codex-linux-sandbox`, the argv[0] check triggers sandbox mode.

**Dispatch Logic:**

```rust
arg0_dispatch_or_else(|sandbox_exe| async move {
    // Main CLI logic
    // sandbox_exe is the path to use for spawning sandbox
})
```

Created and maintained by Nori.
