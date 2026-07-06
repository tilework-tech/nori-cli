# Noridoc: codex-core

Path: @/nori-rs/core

### Overview

The core crate is shared infrastructure inherited from the Codex fork, slimmed down by the crate-layering cleanup (`@/docs/specs/crate-layering.md`) to what the `nori` binary actually uses: configuration loading and editing, authentication, MCP auth helpers, and model/provider metadata. It is no longer a business-logic hub -- session semantics live in `@/nori-rs/acp/` (which does not depend on this crate at all), and the sandboxed-execution engine now lives in `@/nori-rs/sandbox/`.

### How it fits into the larger codebase

```
nori-tui / nori-cli / codex-login
         |
         v
    codex-core
    /    |     \
   v     v      v
config  auth  codex-sandbox (errors, platform sandbox selection)
         |
         v
    codex-protocol (types)
```

The core crate is depended on by:
- `@/nori-rs/tui/` - for config loading, auth management, and git info
- `@/nori-rs/cli/` - for config and auth
- `@/nori-rs/login/` - for auth primitives
- `@/nori-rs/acp/` does **not** depend on core; the ACP-facing helpers it used to import (user notifications, custom prompts, shell/command parsing, compact constants, patch construction) now live in that crate

Key integrations:
- Uses `codex-protocol` for shared types (`@/nori-rs/protocol/`), including the MCP server config types and shell environment policy types defined in its `config_types` module. Core previously re-exported `codex_protocol`'s protocol modules; those re-exports were deleted, so every crate imports `codex_protocol` directly.
- Uses `codex-sandbox` (`@/nori-rs/sandbox/`) for the shared error types (`CodexErr`, `RefreshTokenFailedError` in `auth.rs`), `TruncationPolicy` (in `model_family.rs`), and platform-sandbox selection during config resolution (`get_platform_sandbox` / `set_windows_sandbox_enabled` in `config/mod.rs`). The dependency direction is core -> sandbox, never the reverse.
- Uses `codex-rmcp-client` for MCP OAuth flows (`@/nori-rs/rmcp-client/`)
- Uses `codex-keyring-store` for persistent auth token storage (`@/nori-rs/keyring-store/`)

### Core Implementation

**Configuration** (`config/`, `config_loader/`): Loads and merges configuration from:
1. Global config at `$CODEX_HOME/config.toml` (the `nori` binary points `CODEX_HOME` at `~/.nori/cli`, so core and the Nori config layer in `@/nori-rs/acp/src/config/` read the same file)
2. Project-local config at `<cwd>/.codex/config.toml`
3. Command-line overrides


**Configuration Editing** (`config/edit.rs`): Provides a builder API for programmatic config updates via `toml_edit`:

The `ConfigEditsBuilder` allows code to modify `config.toml` atomically without losing comments or formatting:

```rust
ConfigEditsBuilder::new(codex_home)
    .set_default_model("claude-code", "haiku")
    .apply()
    .await?;
```

Key methods:
- `set_default_model(agent, model)`: Persists a model preference to the `[default_models]` table for a specific agent
- `set_path(path, value)`: Sets arbitrary TOML paths for advanced config mutations
- `apply()`: Writes changes asynchronously; locks config file during write
- `apply_blocking()`: Synchronous variant for non-async contexts

The builder is used by the TUI layer (`@/nori-rs/tui/`) to persist user preferences like model selections when `/model` is invoked (see `@/nori-rs/tui/docs.md`).

**Authentication** (`auth.rs`, `auth/`): Supports multiple auth modes:
- API key via `ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, etc.
- ChatGPT login flow with OAuth
- Keyring storage for persistent tokens (`codex-keyring-store`)

**Command Execution**: No longer lives here. The exec engine, sandbox wrappers, spawn helpers, and error types moved to the `codex-sandbox` crate -- see `@/nori-rs/sandbox/docs.md`.

**MCP Auth Helpers** (`mcp/`): Provides OAuth/auth-status helpers for MCP servers defined in config (e.g. `mcp::auth::compute_auth_statuses()`, used by the TUI's MCP server picker). The `McpServerConfig` and `McpServerTransportConfig` types themselves are defined in `codex_protocol::config_types` (`@/nori-rs/protocol/src/config_types.rs`) so that `@/nori-rs/acp/` can consume them without depending on core; core re-exports them through `config/types.rs` for its own config code. The `McpServerTransportConfig::StreamableHttp` variant supports two OAuth credential modes: dynamic client registration (the default, handled by `rmcp`'s `OAuthState`) and pre-configured client credentials via optional `client_id` and `client_secret_env_var` fields for servers that do not support dynamic registration (e.g., Slack). The `client_secret_env_var` field follows the same env-var-name pattern as `bearer_token_env_var` -- the actual secret is resolved from the environment at runtime. These fields are rejected during deserialization for stdio transport.

**Data Flow (ACP path):**

```
User Input -> Op (UserTurn) -> AcpBackend (@/nori-rs/acp) -> Agent (JSON-RPC via subprocess stdio)
    |
    v
Event (TurnStart/Delta/Complete) <- Response Processing <- Tool Execution
```

ACP (Agent Context Protocol) integration is handled in `@/nori-rs/acp`, not embedded in core. Core provides infrastructure (config, auth) to the frontends; the ACP backend itself does not import core -- it shares only the `codex-protocol` type vocabulary.

**Shared Types Module (`tool_types.rs`):** Types and constants needed across modules are collected in `tool_types.rs`. This includes `ApplyPatchToolType`, `ConfigShellToolType`, and `CODEX_APPLY_PATCH_ARG1`. The constant `CODEX_APPLY_PATCH_ARG1` is re-exported from `lib.rs` because `codex-arg0` (`@/nori-rs/arg0/`) imports it for argv dispatch and Windows batch scripts.

**Model Provider Info (`model_provider_info.rs`):** A pure configuration type defining `ModelProviderInfo` (provider name, optional env-key reference, and retry/timeout settings). The ACP backend communicates over subprocess stdio rather than HTTP, so HTTP-specific fields (base URL, headers, query params, bearer token) have been removed. Built-in providers (OpenAI, Ollama, LMStudio) are defined in `built_in_model_providers()`. User-defined providers in `config.toml` may still include removed fields; serde silently ignores them for backwards compatibility.

**TUI Display Settings:**

Core's `Config::tui_notifications` is a simple `bool` that controls whether the TUI sends OSC 9 terminal escape sequence notifications. It derives its value from the ACP config's `TerminalNotifications` enum during config loading. Core also carries TUI display booleans such as `animations` and `custom_working_messages`; the latter mirrors `[tui].custom_working_messages` from Nori config so the TUI can choose between rotating custom working headers and the plain `Working` label without re-reading config. Core additionally carries `Config::custom_working_message_list: Vec<String>`, mirroring `[tui].custom_working_message_list`; when non-empty and `custom_working_messages` is `true`, the TUI samples this user list instead of the builtin whimsical messages.

### Things to Know

**Module Structure Convention:**

Large modules use a directory layout (`foo/mod.rs` + submodules) instead of a single `foo.rs` file. This separates concerns and keeps individual files manageable. Modules using this pattern include `config/`, `auth/`, and `mcp/`. Test submodules use `tests/mod.rs` + `tests/part*.rs` for large test suites (e.g., `config/tests/`).

**What moved out during the crate-layering cleanup** (`@/docs/specs/crate-layering.md`):

- Dead Codex-engine subsystems were deleted outright: rollout recording (superseded by the transcript recorder in `@/nori-rs/acp/src/transcript/`), command-safety auto-approval, turn diff tracking, event mapping, and user-instruction plumbing.
- ACP-facing leaf helpers moved into `@/nori-rs/acp/src/`: user notifications, custom prompt discovery, shell/command parsing (`parse_command`, `shell`, `bash`, `powershell`), the compact summarization constants and templates, and `create_patch_with_context` (formerly in `util.rs`, which now only holds error-message parsing helpers).
- `McpServerConfig`/`McpServerTransportConfig` and the shell environment policy types (`ShellEnvironmentPolicy` and friends) moved down into `codex_protocol::config_types`; core re-exports them for its own config code.
- The sandboxed-execution engine moved into `codex-sandbox` (`@/nori-rs/sandbox/`): `exec`, `exec_env`, `spawn`, `safety`, `sandboxing/`, `seatbelt` (+ `.sbpl` policies), `landlock`, `text_encoding`, `truncate`, and `error` (`CodexErr`/`SandboxErr`). Its integration tests (exec, seatbelt, text encoding) moved out of `core/tests/suite/` at the same time. Frontends that need exec/sandbox functionality import `codex_sandbox` directly.

Other notes:

- Config uses TOML with optional environment variable expansion
- Auth tokens are stored in the system keyring with fallback to file storage
- Core has no `error` module of its own; it uses the `thiserror` types from `codex_sandbox::error`

**Test Suite:**

The integration test suite in `@/nori-rs/core/tests/suite` covers auth refresh and live CLI behavior; the exec/seatbelt/text-encoding suites now live in `@/nori-rs/sandbox/tests/`. The `core_test_support` helper crate (`@/nori-rs/core/tests/common/`) provides config helpers, macros, and filesystem wait utilities for tests; its exec helper builds shell invocations via `nori_acp::shell` since the shell helpers moved to `@/nori-rs/acp/`, and its sandbox-skip macros use the env-var constants from `codex_sandbox::spawn`.

Created and maintained by Nori.
