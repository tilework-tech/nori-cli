# Noridoc: codex-windows-sandbox

Path: @/nori-rs/windows-sandbox-rs

### Overview

The windows-sandbox crate implements process sandboxing on Windows using restricted tokens and ACL manipulation. It creates constrained execution environments with controlled filesystem and network access.

### How it fits into the larger codebase

Used by `@/nori-rs/core/` (`exec.rs`) as the sandbox executor on Windows. Provides `run_windows_sandbox_capture()` which is called to execute commands in a restricted environment.

### Core Implementation

**Policy Types** (`policy.rs`): `SandboxPolicy` enum:
- `ReadOnly` - No filesystem writes allowed
- `WorkspaceWrite` - Write access to specified roots only
- `DangerFullAccess` - Not supported for sandboxing (errors)

**Token Creation** (`token.rs`): Creates restricted Windows tokens with capability SIDs for different access levels.

**ACL Manipulation** (`acl.rs`):
- `add_allow_ace()` - Grant access to specific paths
- `add_deny_write_ace()` - Deny write access to paths
- `revoke_ace()` - Clean up added ACEs after execution

**Path Computation** (`allow.rs`): `compute_allow_paths()` calculates which paths need allow/deny ACEs based on policy.

**Process Execution** (`run_windows_sandbox_capture()`):
1. Parse and validate sandbox policy
2. Create restricted token with appropriate capability SID
3. Set up ACLs for allowed/denied paths
4. Create process with restricted token via `CreateProcessAsUserW`
5. Capture stdout/stderr
6. Clean up ACEs (unless persistent)

**Network Blocking** (`env.rs`): Modifies environment to disable network access when required.

### Things to Know

- On non-Windows platforms, stub implementations return errors
- Capability SIDs are persisted to `~/.codex/cap-sids.json`
- `WorkspaceWrite` mode persists ACEs; `ReadOnly` revokes them after execution
- Command line arguments are properly quoted for Windows
- Timeout support with process termination
- Logging to `~/.codex/` for debugging sandbox issues

Created and maintained by Nori.
