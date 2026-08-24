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

- agent registry and distribution configuration, including per-agent
  `[default_models]` and the optional `[[agents]].model_override`
  (`ModelOverrideToml { env, arg }`) declaring how a custom agent accepts a
  forced model id at spawn time (see below);
- `AskForApproval`, `SandboxPolicy`, `SandboxMode`, and `TrustLevel`;
- `McpServerConfig` and MCP transport configuration;
- `ShellEnvironmentPolicy` and related environment patterns;
- resolved TUI, hooks, history, notifications, worktree, cloud, and proxy
  settings; and
- comment-preserving `NoriConfigEdits` for the user-owned TOML file.

Resolution applies typed CLI overrides, raw dotted overrides, then the user
file. Project trust resolves against the effective cwd and primary git root.

`[tui] resize_reflow` controls whether the inline TUI rebuilds terminal
scrollback after width changes. It defaults to `true`; the `/settings` toggle
persists to this same key, so startup configuration and runtime changes share
one source of truth.

`[default_models]` maps an agent slug to a model id. This crate only stores the
value; the id is not validated here (an agent may accept models its ACP picker
never advertised). `nori-acp-host` reads `model_override` to build a
`ModelInjection` strategy — `env` names an environment variable, `arg` a CLI
flag, and `env` wins if both are set — so an out-of-catalog default model can be
forced through the agent subprocess at spawn. Built-in agents (Claude, Codex,
Gemini) know their own channel and ignore `model_override`. The user-facing
`config.toml` schema, including these tables and the `[[agents]]` fields, is
described in [`config.md`](../config.md).

### Things to Know

- These policy types are configuration semantics, not ACP session events; they
  must not move into `nori-protocol` to avoid a dependency edge.
- MCP authentication status is computed and owned by `codex-rmcp-client`.
- There are no Codex profiles, managed preferences, or project-local config
  merge layers. Reusable agent behavior belongs to Nori Skillsets.
- The crate remains independent of `codex-core` and the deleted
  `codex-protocol` crate.

Created and maintained by Nori.
