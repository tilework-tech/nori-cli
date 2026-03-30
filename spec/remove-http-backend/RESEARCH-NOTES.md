# Research Notes

## HTTP Backend Architecture

The codebase has two distinct backends:
1. **HTTP backend** (legacy): `Codex` struct -> `Session` -> `ModelClient` -> `codex-api` -> `codex-client` -> `reqwest` -> OpenAI HTTP API
2. **ACP backend** (production): `AcpBackend` -> `SacpConnection` -> subprocess (ACP agent)

The nori binary exclusively uses the ACP path. The HTTP backend is unreachable from nori but still compiles into the binary.

## Key Finding: codex-api as the Critical Dependency

The `codex-api` crate is the HTTP API client layer that `codex-core` depends on. Making it optional behind a cargo feature is the cleanest way to gate the HTTP backend because:
- It's a single, well-defined dependency boundary
- All HTTP-backend modules in codex-core import from `codex-api`
- When the feature is off, the compiler eliminates all HTTP-backend code

## Modules that directly import from codex-api

1. `api_bridge.rs` - Error mapping between codex-api and codex-core
2. `client.rs` - `ModelClient` struct, the HTTP API client
3. `client_common.rs` - `ResponseEvent` re-export, `ResponseStream` type
4. `model_provider_info.rs` - `to_api_provider()` method only (struct itself is shared via Config)

## Modules that transitively depend on HTTP-backend types

1. `codex/` module (entire) - uses `ModelClient`, `Session` contains `ModelClient`
2. `codex_conversation.rs` - wraps `Codex` struct
3. `conversation_manager.rs` - wraps `CodexConversation`
4. `compact_remote.rs` - remote compaction via HTTP API
5. `compact.rs` - partially (functions use `Session`/`TurnContext`, but constants are shared with ACP)
6. `tasks/` - uses `TurnContext` from `codex/` module
7. `state/` - used only by tasks and codex

## Shared code (used by both ACP and HTTP paths)

- `config/` - Configuration loading and types (includes `ModelProviderInfo`)
- `auth/` - Authentication management
- `protocol` re-exports from `codex-protocol`
- `compact::SUMMARIZATION_PROMPT`, `compact::SUMMARY_PREFIX` - prompt constants
- `compact::content_items_to_text` - utility function
- `compact::collect_user_messages`, `build_compacted_history` etc.
- `default_client.rs` - reqwest HTTP client (used by TUI for update checks)
- `model_provider_info.rs` - struct and constants (used in Config)
- `tools/` - Tool handling infrastructure (used by both backends)
- `mcp/` - MCP server management
- Various utility modules

## Compact endpoint in codex-api (next removal target)

After removing `compact_remote.rs` from codex-core, the compact endpoint in `codex-api` is now dead code:

**Files to remove/modify:**
1. `codex-api/src/endpoint/compact.rs` - entire file (CompactClient, tests)
2. `codex-api/src/endpoint/mod.rs` - remove `pub mod compact;` line
3. `codex-api/src/lib.rs` - remove `pub use crate::endpoint::compact::CompactClient;` and `pub use crate::common::CompactionInput;`
4. `codex-api/src/common.rs` - remove `CompactionInput` struct and its doc comment
5. `codex-api/src/provider.rs` - remove `WireApi::Compact` variant
6. `codex-api/src/endpoint/compact.rs` references `WireApi::Compact` in `path()` method
7. `codex-api/src/endpoint/responses.rs:93` - has `WireApi::Compact | WireApi::Responses` match arm that needs updating

**Verification:**
- No external consumers of `CompactClient` or `CompactionInput` exist (only within codex-api)
- `WireApi::Compact` is only used within codex-api (codex-core has its own WireApi without Compact)
- All integration tests use the streaming endpoints (Chat/Responses), not Compact

## Approach: Feature-gate codex-api (future)

Add a `legacy-http-backend` feature to codex-core:
- Makes `codex-api` an optional dependency
- Gates HTTP-backend-only modules with `#[cfg(feature = "legacy-http-backend")]`
- Dev-dependencies enable the feature so all tests pass
- Downstream crates (acp, tui, cli) do NOT enable it
