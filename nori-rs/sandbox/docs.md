# Noridoc: codex-sandbox

Path: @/nori-rs/sandbox

### Overview

- Pure sandboxed-execution engine, split out of `codex-core` during the crate-layering cleanup (`@/docs/specs/crate-layering.md`). It owns platform sandbox selection (`safety.rs`, re-exported at the crate root as `get_platform_sandbox` / `set_windows_sandbox_enabled`), process spawning (`spawn.rs`), and the exec engine (`exec.rs`) that runs a command under a sandbox policy and returns captured output.
- Also home to the exec-adjacent support code that has no business living near config or auth: environment construction (`exec_env.rs`), output truncation policy (`truncate.rs`), legacy-encoding output decoding (`text_encoding.rs`), and the shared error vocabulary (`error.rs` — `CodexErr`, `SandboxErr`).
- Deliberately has no config or auth dependencies. The policy types it consumes (`SandboxPolicy`, `ShellEnvironmentPolicy` and friends) come from `codex_protocol` (`@/nori-rs/protocol/src/config_types.rs` and `protocol/sandbox.rs`).

### How it fits into the larger codebase

```
codex-protocol (SandboxPolicy, ShellEnvironmentPolicy)
        |
        v
  codex-sandbox ----> codex-windows-sandbox (restricted tokens)
   ^   ^   ^   \----> codex-linux-sandbox binary (exec'd for Landlock+seccomp)
   |   |   |
 core tui cli/linux-sandbox/core_test_support
```

- `@/nori-rs/cli/` (`debug_sandbox.rs`, the `nori sandbox macos|linux|windows` subcommand) is the main in-workspace runtime consumer: it calls the seatbelt/landlock spawn helpers and `exec_env::create_env` directly to run arbitrary commands under a sandbox (its windows path calls `codex-windows-sandbox` directly).
- `@/nori-rs/linux-sandbox/` builds its Landlock/seccomp setup on this crate's error types, and its integration tests drive `process_exec_tool_call` end-to-end.
- `@/nori-rs/tui/` uses `get_platform_sandbox()` for sandbox-availability checks in approval flows and `set_windows_sandbox_enabled()` in tests.
- `@/nori-rs/core/` depends on this crate for the error types its auth code returns (`CodexErr`, `RefreshTokenFailedError`), `TruncationPolicy` (used by `model_family.rs`), the `CODEX_SANDBOX*` env-var constants, and platform-sandbox selection during config resolution. `codex-login` reaches these error types transitively through core's auth API. The dependency direction is `codex-core -> codex-sandbox`, never the reverse.
- Agent commands at runtime are executed by external ACP agent subprocesses, not by this crate; within the `nori` binary the engine is exercised by the debug subcommand and by tests.

### Core Implementation

- `exec::process_exec_tool_call()` is the engine entry point: it picks a `SandboxType` from the `SandboxPolicy` (`DangerFullAccess` means no sandbox; otherwise `get_platform_sandbox()`), transforms the portable command spec into a ready-to-spawn `ExecEnv` via `sandboxing/`, and executes it with output capture, streaming, truncation, and timeout/cancellation handling (`ExecExpiration`).
- `sandboxing/` owns sandbox placement: wrapping the command in `sandbox-exec` with the `.sbpl` Seatbelt policies (`seatbelt.rs`) on macOS, exec'ing the `codex-linux-sandbox` helper binary (`landlock.rs`, see `@/nori-rs/linux-sandbox/`) on Linux, or delegating to `codex-windows-sandbox` (`@/nori-rs/windows-sandbox-rs/`) on Windows. It also injects the `CODEX_SANDBOX` / `CODEX_SANDBOX_NETWORK_DISABLED` markers defined in `spawn.rs`.
- `exec_env::create_env()` builds the child-process environment from a `ShellEnvironmentPolicy` (inherit mode, exclude/include-only patterns, explicit sets); the policy types themselves live in `codex_protocol::config_types`.
- `error.rs` defines `SandboxErr` (denial, timeout, signal, Landlock/seccomp setup failures) and the broader `CodexErr`, including the auth-refresh error types consumed by `@/nori-rs/core/src/auth.rs`.
- The Windows sandbox is opt-in at runtime: `set_windows_sandbox_enabled()` flips a process-global atomic that `get_platform_sandbox()` consults; core's feature resolution (`@/nori-rs/core/src/config/mod.rs`) is the production caller.

### Things to Know

- The integration tests in `@/nori-rs/sandbox/tests/` are the real coverage for sandbox denial, timeout, and output-truncation semantics (`exec.rs`), Seatbelt writable-root and `.git` protection rules (`seatbelt.rs`), and legacy-encoding output decoding (`text_encoding_fix.rs`). They moved here from `core/tests/suite/` along with the code. Linux denial semantics are additionally covered by `@/nori-rs/linux-sandbox/tests/suite/landlock.rs`.
- Sandbox-in-sandbox does not work: tests early-exit when `CODEX_SANDBOX=seatbelt` or `CODEX_SANDBOX_NETWORK_DISABLED=1` is set (i.e. when the test itself runs inside a sandbox). Never modify code touching these env vars; `core_test_support` (`@/nori-rs/core/tests/common/`) re-exports the constants for its skip macros.
- A sandbox denial is inferred, not reported by the OS: a non-zero exit under an active sandbox surfaces as `SandboxErr::Denied` carrying the full output, and timeouts surface as `SandboxErr::Timeout` rather than a plain error string, so callers can distinguish retry-without-sandbox cases.
- `truncate.rs` is policy plus helpers only; the former `Config`-coupled constructor was deleted when the module moved here, so consumers pass an explicit `TruncationPolicy` (e.g. from `codex_core::model_family`).
- This crate must stay free of config/auth machinery — that boundary is the point of the split. If new exec behavior needs configuration, thread it in as a parameter or a `codex_protocol` type.

Created and maintained by Nori.
