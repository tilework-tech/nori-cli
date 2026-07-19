# Noridoc: codex-core

Path: @/nori-rs/core

### Overview

- `codex-core` is inherited infrastructure from the Codex fork, retained primarily for authentication and compatibility code used behind `codex-login`.
- It is not the configuration boundary for the Nori binary. `nori-config` owns Nori's resolved settings and persistence.
- Session behavior lives in `@/nori-rs/harness/`, and sandboxed execution lives in `@/nori-rs/sandbox/`.

### How it fits into the larger codebase

- `@/nori-rs/login/` depends on core for OAuth tokens, credential storage, and auth lifecycle types, then exposes the smaller authentication surface used by the TUI.
- `@/nori-rs/tui/`, `@/nori-rs/cli/`, `@/nori-rs/harness/`, `@/nori-rs/common/`, `@/nori-rs/arg0/`, and the platform sandbox crates do not depend directly on core.
- Shared configuration vocabulary such as MCP servers, approval policy,
  sandbox policy, and shell environment policy lives in
  `@/nori-rs/nori-config/`, allowing Nori crates to use those types without
  importing core.
- Core depends on `codex-sandbox` for inherited error and sandbox types; the dependency never points from sandbox back to core.

### Core Implementation

- Authentication supports OAuth/API credentials and persistent storage through the system keyring or auth file. Nori reaches these capabilities through `codex-login`, including the in-TUI `/login` flow.
- The inherited `Config` implementation remains as an isolated compatibility surface within this crate. Its loader reads one user-owned `$CODEX_HOME/config.toml`; project-local layers, managed configuration, and macOS managed preferences have been removed.
- The inherited config editor now applies global edits only. Profile-scoped editing and profile resolution were deleted.
- Model/provider metadata remains as a compatibility type surface. ACP session and agent selection are owned by Nori's registry and harness instead.
- MCP OAuth helpers may remain for inherited consumers, but Nori's TUI computes MCP auth status through `codex-rmcp-client` and persists canonical protocol MCP settings through `nori-config`.

### Things to Know

- Core configuration is not mirrored into Nori configuration. The production CLI resolves `$NORI_HOME/config.toml` through `nori-config` and passes the resulting `NoriConfig` explicitly to the TUI and harness.
- Codex configuration profiles are gone. Nori uses `agent` for agent selection, `[default_models]` for per-agent model defaults, and Nori Skillsets for reusable agent behavior.
- The inherited onboarding flow, model-migration prompts and upgrade metadata, managed config loader, and OpenTelemetry integration were deleted rather than carried as dormant Nori code paths.
- Large modules may use directory-based Rust modules with adjacent test submodules, but new Nori responsibilities should not be added here merely because core previously acted as a catch-all.

Created and maintained by Nori.
