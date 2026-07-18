# Noridoc: nori-config

Path: @/nori-rs/nori-config

### Overview

- `nori-config` is the sole configuration boundary for the Nori CLI, TUI, and session harness.
- It resolves user configuration from `$NORI_HOME/config.toml`, defaulting to `~/.nori/cli/config.toml`.
- It owns both the resolved `NoriConfig` used at runtime and comment-preserving edits to the user-owned TOML file.

### How it fits into the larger codebase

- `@/nori-rs/cli/` and `@/nori-rs/tui/` use the crate directly for startup flags, sandbox diagnostics, onboarding trust, settings, agent selection, and MCP persistence.
- The frontend resolves one `NoriConfig` and injects a shared `Arc<NoriConfig>` into `@/nori-rs/harness/`; session launch, resume, and pre-session probing do not load ambient configuration again.
- Canonical MCP, sandbox, approval, trust, and shell-environment vocabulary comes from `@/nori-rs/protocol/`, avoiding a dependency on `codex-core`.
- Agent registry entries, hook paths, worktree behavior, history, notifications, TUI preferences, and cloud/proxy settings are projected from the resolved config into their owning runtime components.

### Core Implementation

- Resolution precedence is typed CLI overrides, then raw `-c key=value` overrides, then the user config file. Paths and additional writable roots are resolved against the effective working directory.
- Project trust first matches the effective cwd and then the primary git root, including linked worktrees whose trust belongs to the main repository.
- Sandbox mode and `[sandbox_workspace_write]` settings are resolved into the concrete runtime sandbox policy, and project trust supplies the default approval policy when no explicit policy is present.
- `NoriConfigEdits` applies focused or dotted-path mutations with a same-directory temporary-file replacement while preserving existing TOML comments, inline-table values, formatting, and file permissions. Newly created config files are private on Unix. MCP persistence replaces the complete table using the protocol-owned config type.

### Things to Know

- There are no managed, system, project-local, or macOS-preferences config layers. The user file plus explicit launch overrides are the entire stack.
- Codex-style `profile` and `[profiles]` keys are rejected with guidance to use Nori Skillsets. Skillsets own reusable agent behavior; they are not a hidden config merge layer.
- The legacy top-level `model` key is rejected. Use `agent` for the persisted agent and `[default_models]` for per-agent model defaults.
- The crate must stay independent of `codex-core`; adding a second loader or adapter would recreate the split-brain configuration boundary this crate exists to remove.

Created and maintained by Nori.
