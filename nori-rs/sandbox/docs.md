# Noridoc: codex-sandbox

Path: @/nori-rs/sandbox

### Overview

`codex-sandbox` is the local sandboxed-execution engine and platform selection
layer. It owns process spawning, environment construction, output capture, and
the shared sandbox error vocabulary. It is not part of the ACP session
protocol.

### How it fits into the larger codebase

```text
nori-config (SandboxPolicy, ShellEnvironmentPolicy)
                         |
                         v
                   codex-sandbox
                    /          \
          linux helper       windows sandbox
```

The CLI uses it for `nori sandbox ...`; platform crates and selected tests use
its lower-level helpers. External ACP agents execute their own tool commands.

### Core Implementation

`process_exec_tool_call` selects the platform mechanism from
`nori_config::SandboxPolicy`, builds the child environment from
`nori_config::ShellEnvironmentPolicy`, applies Seatbelt, Landlock/seccomp,
Windows restrictions, or no sandbox as configured, and returns structured
output or `SandboxErr`.

### Things to Know

- Approval, sandbox, and shell-environment policy moved to `nori-config` when
  the inherited Codex protocol crate was deleted.
- This dependency is semantic: config owns policy; sandbox owns execution.
- Agent tool execution is not routed through this crate in the normal ACP path.
- Sandbox-in-sandbox integration tests may skip when the relevant Nori sandbox
  marker environment variables show the test itself is already constrained.

Created and maintained by Nori.
