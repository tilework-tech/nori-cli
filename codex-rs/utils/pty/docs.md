# Noridoc: utils/pty

Path: @/codex-rs/utils/pty

### Overview

The `codex-utils-pty` crate provides pseudo-terminal handling for executing commands with PTY semantics. It enables Codex to spawn processes with full terminal emulation, useful for commands that expect interactive terminal features.

### How it fits into the larger codebase

PTY utils is used for command execution requiring terminal:

- **Core** may use for interactive command execution
- **Provides** bidirectional I/O via channels
- **Handles** process lifecycle management

### Core Implementation

**Main Function:**

```rust
pub async fn spawn_pty_process(
    program: &str,
    args: &[String],
    cwd: &Path,
    env: &HashMap<String, String>,
    arg0: &Option<String>,
) -> Result<SpawnedPty>
```

**SpawnedPty:**

```rust
pub struct SpawnedPty {
    pub session: ExecCommandSession,
    pub output_rx: broadcast::Receiver<Vec<u8>>,
    pub exit_rx: oneshot::Receiver<i32>,
}
```

**ExecCommandSession:**

Manages PTY lifecycle with:
- `writer_sender()` - Input channel
- `output_receiver()` - Output broadcast
- `has_exited()` - Status check
- `exit_code()` - Return code

### Things to Know

**PTY Size:**

Fixed at 24 rows x 80 columns.

**Channel Sizes:**

- Writer: 128-message channel
- Output: 256-message broadcast

**Cleanup:**

`Drop` implementation kills process and aborts all tasks.

**Portable PTY:**

Uses `portable_pty` crate for cross-platform PTY support.

Created and maintained by Nori.
