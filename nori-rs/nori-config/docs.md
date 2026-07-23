# Noridoc: nori-config

Path: @/nori-rs/nori-config

### Overview

`nori-config` is the sole configuration and runtime-policy boundary for the
Nori CLI, TUI, ACP host, harness, and local sandbox support.

### How it fits into the larger codebase

Frontends resolve one `NoriConfig` from `$NORI_HOME/config.toml` (default
`~/.nori/cli/config.toml`) plus explicit launch overrides, then inject it into
the harness. Session launch, resume, and probing do not reload ambient config.

### Core Implementation

The crate owns:

- agent registry and distribution configuration;
- `AskForApproval`, `SandboxPolicy`, `SandboxMode`, and `TrustLevel`;
- `McpServerConfig` and MCP transport configuration;
- `ShellEnvironmentPolicy` and related environment patterns;
- resolved TUI, hooks, history, notifications, worktree, cloud, and proxy
  settings; and
- comment-preserving `NoriConfigEdits` for the user-owned TOML file.

Resolution applies typed CLI overrides, raw dotted overrides, then the user
file. Project trust resolves against the effective cwd and primary git root.

### Things to Know

- These policy types are configuration semantics, not ACP session events; they
  must not move into `nori-protocol` to avoid a dependency edge.
- MCP authentication status is computed and owned by `codex-rmcp-client`.
- There are no Codex profiles, managed preferences, or project-local config
  merge layers. Reusable agent behavior belongs to Nori Skillsets.
- The crate remains independent of `codex-core` and the deleted
  `codex-protocol` crate.

Created and maintained by Nori.
