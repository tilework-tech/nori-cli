# Noridoc: windows-sandbox-rs

Path: @/codex-rs/windows-sandbox-rs

### Overview

The `codex-windows-sandbox-rs` crate provides Windows-specific process sandboxing using restricted tokens and ACL manipulation. It enables Codex to run commands with reduced privileges and controlled filesystem access on Windows platforms.

### How it fits into the larger codebase

Windows sandbox is the Windows counterpart to Linux Landlock:

- **Core** uses for sandboxed command execution on Windows
- **CLI** provides `codex sandbox windows` for testing
- **Stubs** out to error on non-Windows platforms

### Core Implementation

**Main Functions:**

```rust
pub fn run_windows_sandbox_capture(
    policy_json_or_preset: &str,
    sandbox_policy_cwd: &Path,
    codex_home: &Path,
    command: Vec<String>,
    cwd: &Path,
    env_map: HashMap<String, String>,
    timeout_ms: Option<u64>,
) -> Result<CaptureResult>

pub fn preflight_audit_everyone_writable(
    cwd: &Path,
    env_map: &HashMap<String, String>,
    logs_base_dir: Option<&Path>,
) -> Result<Vec<PathBuf>>
```

**CaptureResult:**

```rust
pub struct CaptureResult {
    pub exit_code: i32,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub timed_out: bool,
}
```

**Modules (Windows-only):**

| Module | Purpose |
|--------|---------|
| `token.rs` | Restricted token creation |
| `acl.rs` | ACL entry manipulation |
| `allow.rs` | Compute allowed paths |
| `audit.rs` | Security auditing |
| `policy.rs` | Sandbox policy parsing |
| `env.rs` | Environment normalization |

### Things to Know

**Sandbox Modes:**

- `ReadOnly` - No filesystem writes
- `WorkspaceWrite` - Writes to workspace only

**Token Approach:**

Creates restricted tokens with capability SIDs. Processes run with reduced privileges via `CreateProcessAsUserW`.

**ACL Manipulation:**

Adds temporary ACEs for allowed paths, revokes after execution (unless persistent).

**Non-Windows Stub:**

Returns error on non-Windows platforms. Compilation includes all code but runtime checks platform.

**Timeout Handling:**

Process terminated and exit code set to 128+64 on timeout.

**Logging:**

Logs sandbox operations to `codex_home` for debugging.

Created and maintained by Nori.
