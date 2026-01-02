# ACP Subprocess Sandboxing Implementation Plan

This document describes the implementation plan for wrapping ACP agent subprocesses with OS-level sandboxing (Seatbelt on macOS, Landlock+seccomp on Linux, restricted tokens on Windows).

## Background

The ACP module spawns agent subprocesses (Claude Code CLI, Codex CLI, Gemini CLI) without OS-level sandboxing. Currently, there is only a temporary application-level path restriction in `write_text_file()` that checks paths are within the working directory or `/tmp`.

The codex-core module has fully implemented platform-specific sandboxing that should be reused:
- **macOS**: Seatbelt via `/usr/bin/sandbox-exec`
- **Linux**: Landlock LSM + seccomp filters via `codex-linux-sandbox` binary
- **Windows**: Restricted tokens + ACL manipulation

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                      SandboxPolicy                              │
│  (codex-protocol/src/protocol.rs)                              │
│  ┌──────────────────┬──────────────────┬─────────────────────┐ │
│  │ DangerFullAccess │    ReadOnly      │   WorkspaceWrite    │ │
│  │   (no sandbox)   │ (full read, no   │ (cwd + extras +     │ │
│  │                  │  write/network)  │  configurable net)  │ │
│  └──────────────────┴──────────────────┴─────────────────────┘ │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                     ACP Connection                              │
│  (acp/src/connection.rs)                                        │
│  - spawn_connection_internal() wraps subprocess in sandbox      │
│  - Platform-specific transformation at spawn time               │
└─────────────────────────────────────────────────────────────────┘
                              │
         ┌────────────────────┼────────────────────┐
         ▼                    ▼                    ▼
┌─────────────────┐ ┌─────────────────┐ ┌─────────────────────────┐
│ Linux Landlock  │ │ macOS Seatbelt  │ │ Windows Restricted      │
│ + seccomp       │ │                 │ │ Tokens                  │
│ (codex-linux-   │ │ (/usr/bin/      │ │ (codex-windows-sandbox) │
│  sandbox binary)│ │  sandbox-exec)  │ │                         │
└─────────────────┘ └─────────────────┘ └─────────────────────────┘
```

## Phase 1: Core Sandbox Wrapping

### 1.1 Add sandbox module to ACP

Create a new module `acp/src/sandbox.rs` that provides sandbox transformation utilities.

**Key components:**
- `SandboxType` enum (reuse from codex-core or redefine)
- `get_platform_sandbox()` function for platform detection
- `transform_command_for_sandbox()` function that wraps commands

### 1.2 Modify spawn_connection_internal()

Transform the subprocess spawn in `connection.rs` to wrap the command with platform-specific sandbox:

**Current code** (connection.rs:446-455):
```rust
let mut child = Command::new(&config.command)
    .args(&config.args)
    .current_dir(cwd)
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .spawn()?;
```

**New approach:**
```rust
let (exe, args, arg0) = transform_command_for_sandbox(
    &config.command,
    &config.args,
    &sandbox_policy,
    cwd,
    codex_linux_sandbox_exe.as_deref(),
)?;

let mut cmd = Command::new(&exe);
cmd.args(&args)
   .current_dir(cwd)
   .stdin(Stdio::piped())
   .stdout(Stdio::piped())
   .stderr(Stdio::piped());

// For Linux, set argv[0] to "codex-linux-sandbox" for arg0 dispatch
if let Some(ref a0) = arg0 {
    cmd.arg0(a0);
}

let child = cmd.spawn()?;
```

### 1.3 Platform-specific transformations

**macOS (Seatbelt):**
- Wrap command with `/usr/bin/sandbox-exec -p <policy> -D<params> -- <command>`
- Reuse `create_seatbelt_command_args()` from codex-core
- Set env var `CODEX_SANDBOX=seatbelt`

**Linux (Landlock+seccomp):**
- Wrap command with `codex-linux-sandbox --sandbox-policy <json> --sandbox-policy-cwd <path> -- <command>`
- Reuse `create_linux_sandbox_command_args()` from codex-core
- Set argv[0] to "codex-linux-sandbox" for arg0 dispatch

**Windows:**
- More complex - requires in-process token restriction (see Phase 3)

### 1.4 Remove temporary path restrictions

After sandbox is implemented, remove the application-level path checking in `write_text_file()` (connection.rs:576-623). The OS-level sandbox will enforce write restrictions more robustly.

## Phase 2: Configuration Threading

### 2.1 Add sandbox configuration to AcpAgentConfig

Modify `registry.rs` to include sandbox-related configuration:

```rust
pub struct AcpAgentConfig {
    pub agent: AgentKind,
    pub provider_slug: String,
    pub command: String,
    pub args: Vec<String>,
    pub provider_info: AcpProviderInfo,
    // NEW fields:
    pub sandbox_policy: SandboxPolicy,
    pub codex_linux_sandbox_exe: Option<PathBuf>,
}
```

### 2.2 Thread sandbox policy through spawn

Update `AcpConnection::spawn()` signature to accept sandbox configuration:

```rust
pub async fn spawn(
    config: &AcpAgentConfig,
    cwd: &Path,
    sandbox_policy: &SandboxPolicy,
    codex_linux_sandbox_exe: Option<&Path>,
) -> Result<Self>
```

### 2.3 Update callers in backend.rs

The `AcpBackend` must pass sandbox policy when spawning connections:

```rust
// In backend.rs, when creating new AcpConnection:
let connection = AcpConnection::spawn(
    &config,
    cwd,
    &self.sandbox_policy,  // From session config
    self.codex_linux_sandbox_exe.as_deref(),
).await?;
```

### 2.4 Resolve codex-linux-sandbox executable path

On Linux, the `codex-linux-sandbox` binary must be located. Options:
1. Embedded in main binary via arg0 dispatch (check if main exe supports it)
2. Sibling executable next to main binary
3. Environment variable `CODEX_LINUX_SANDBOX_EXE`
4. Well-known path

## Phase 3: Windows Support

Windows sandbox uses a fundamentally different approach than command wrapping.

### 3.1 Windows token-based sandboxing

Windows uses `CreateProcessAsUserW` with restricted tokens and ACL manipulation:
- `CreateRestrictedToken()` with `DISABLE_MAX_PRIVILEGE`, `LUA_TOKEN`, `WRITE_RESTRICTED`
- Temporary ACEs added for allowed paths
- ACEs revoked after execution

### 3.2 Integration options

**Option A: Wrapper binary approach**
- Create a Windows wrapper similar to `codex-linux-sandbox`
- The wrapper creates restricted token and spawns the real process
- Simpler integration but requires separate binary

**Option B: In-process token restriction**
- Use `codex-windows-sandbox` crate directly from ACP
- More complex as it requires different spawn path for Windows
- Matches how codex-core implements Windows sandbox

**Option C: Defer Windows sandboxing**
- Document as unsupported for initial implementation
- ACP on Windows runs without sandbox (matches current behavior)

### 3.3 Current implementation: Option C (Deferred)

**Status:** Windows sandboxing is deferred for the initial implementation.

The current implementation in `acp/src/sandbox.rs` handles Windows by:

1. **Platform detection:** `get_platform_sandbox()` returns `None` on Windows
2. **No-op transformation:** `transform_command_for_sandbox()` returns the original command unchanged on Windows when `sandbox_type` is `None`
3. **Warning on restrictive policies:** When a non-`DangerFullAccess` policy is requested on Windows, the function returns the command unsandboxed but logs a warning

**Code behavior on Windows:**
```rust
// In sandbox.rs - Windows returns None for sandbox type
#[cfg(target_os = "windows")]
{ None }

// transform_command_for_sandbox handles this gracefully:
match get_platform_sandbox() {
    Some(sandbox_type) => { /* wrap command */ }
    None => {
        // Windows or unsupported platform - return command unchanged
        Ok(SandboxedCommand {
            program: program.to_string(),
            args: args.to_vec(),
            arg0: None,
            sandbox_type: SandboxType::None,
        })
    }
}
```

**Rationale for deferral:**
1. Windows sandbox requires significant additional complexity (token manipulation, ACL changes)
2. The command-wrapping pattern used by macOS and Linux doesn't apply to Windows
3. Users on Windows can still use ACP with `DangerFullAccess` policy
4. Application-level path restrictions in `write_text_file()` provide basic safety until Windows sandbox is implemented

**Future work for Windows:**
- Implement `codex-windows-sandbox` wrapper binary approach (Option A)
- Or integrate `windows-sandbox-rs` crate directly with special Windows spawn path (Option B)
- Remove the `SandboxType::None` fallback once Windows sandbox is implemented

## Reusable Components from codex-core

| Component | Location | Reusability |
|-----------|----------|-------------|
| `create_seatbelt_command_args()` | `core/src/seatbelt.rs:46` | Direct reuse |
| `create_linux_sandbox_command_args()` | `core/src/landlock.rs:43` | Direct reuse |
| `get_platform_sandbox()` | `core/src/safety.rs:99` | Direct reuse |
| `SandboxType` enum | `core/src/exec.rs:108` | Direct reuse |
| `SandboxPolicy` | `protocol/src/protocol.rs:258` | Already shared |
| `is_likely_sandbox_denied()` | `core/src/exec.rs:383` | Useful for error detection |
| Seatbelt policy files | `core/src/seatbelt_*.sbpl` | Included as strings |

## Dependencies

### New dependencies for ACP crate

```toml
[dependencies]
codex-core = { path = "../core", default-features = false, features = ["sandbox"] }
```

Or alternatively, extract sandbox utilities to a new `codex-sandbox` crate that both `codex-core` and `codex-acp` can depend on.

### Feature flags

Consider adding a feature flag for sandbox support:
```toml
[features]
default = ["sandbox"]
sandbox = []
```

## Testing

### Unit tests
- Test `transform_command_for_sandbox()` produces correct command structure
- Test platform detection returns expected values
- Test sandbox policy serialization

### Integration tests
- Spawn mock agent under sandbox, verify it can read files
- Spawn mock agent under sandbox, verify writes outside cwd fail
- Test sandbox denial detection from exit codes and stderr

### Manual testing
- Test with real Claude Code CLI on macOS
- Test with real Claude Code CLI on Linux
- Verify agents work correctly under sandbox

## Migration

1. Implement sandbox wrapping behind feature flag
2. Test extensively on macOS and Linux
3. Remove temporary path restrictions from `write_text_file()`
4. Enable by default
5. Document Windows limitations

## Files to modify

1. `acp/Cargo.toml` - Add dependencies
2. `acp/src/lib.rs` - Export new sandbox module
3. `acp/src/sandbox.rs` - New file with sandbox transformation
4. `acp/src/connection.rs` - Modify spawn to use sandbox
5. `acp/src/registry.rs` - Add sandbox config fields
6. `acp/src/backend.rs` - Thread sandbox policy to connection
7. `core/src/seatbelt.rs` - Make functions pub(crate) -> pub
8. `core/src/landlock.rs` - Make functions pub(crate) -> pub
9. `core/src/safety.rs` - Make functions pub(crate) -> pub
