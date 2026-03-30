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

## Chat Completions wire protocol (next removal target)

The HTTP backend supports two wire APIs: `WireApi::Responses` (Responses API) and `WireApi::Chat` (Chat Completions API). The Chat Completions path is a distinct, self-contained component.

**Key findings:**
- Nori uses ACP exclusively — neither wire protocol matters for production
- The integration test suite uses `WireApi::Responses` for mocking, NOT `WireApi::Chat`
- Only one `#[ignore]`d test (`rmcp_client::stdio_image_completions_round_trip`) uses `WireApi::Chat`
- `WireApi::Chat` is the `#[default]` variant in codex-core (for Ollama/OSS providers)
- Built-in Ollama provider explicitly sets `WireApi::Chat`

**codex-api files to remove:**
1. `codex-api/src/endpoint/chat.rs` - ChatClient, AggregatedStream (~266 lines)
2. `codex-api/src/requests/chat.rs` - ChatRequestBuilder (~388 lines)
3. `codex-api/src/sse/chat.rs` - spawn_chat_stream, process_chat_sse (~504 lines)

**codex-api files to modify:**
1. `codex-api/src/endpoint/mod.rs` - remove `pub mod chat;`
2. `codex-api/src/requests/mod.rs` - remove `pub mod chat;`
3. `codex-api/src/sse/mod.rs` - remove `pub mod chat;`
4. `codex-api/src/lib.rs` - remove ChatClient re-export
5. `codex-api/src/provider.rs` - remove `WireApi::Chat` variant
6. `codex-api/tests/clients.rs` - remove Chat URL routing tests

**codex-core files to modify:**
1. `core/src/client.rs` - remove `stream_chat_completions()`, simplify `stream()` dispatch
2. `core/src/tools/spec/mod.rs` - remove `create_tools_json_for_chat_completions_api()`
3. `core/src/model_provider_info.rs` - keep `WireApi::Chat` variant for config compat, remove `to_api_provider()` Chat mapping

**Test files to remove:**
1. `core/tests/chat_completions_sse.rs` - 8 SSE parsing tests
2. `core/tests/chat_completions_payload.rs` - 7 request payload tests

**Test files to modify:**
1. `core/tests/suite/rmcp_client.rs` - update or remove the `#[ignore]`d Chat test
2. `core/src/config/tests/mod.rs` - update test fixtures
3. `core/src/model_provider_info.rs` - update deserialization tests

**Strategy:** Keep the `WireApi::Chat` variant in codex-core's enum for config compatibility. When the Chat path is selected in `ModelClient::stream()`, return an error. This prevents config deserialization breakage while removing all implementation code.

## WireApi enum removal from codex-api (next removal target)

After removing Chat Completions, `codex-api`'s `WireApi` enum is now a single-variant enum (`Responses` only). It's a pointless abstraction that adds noise to the codebase.

**codex-api files to modify:**
1. `codex-api/src/provider.rs` - Remove `WireApi` enum, remove `wire` field from `Provider`, simplify `is_azure_responses_endpoint()`
2. `codex-api/src/lib.rs` - Remove `WireApi` re-export
3. `codex-api/src/endpoint/responses.rs` - Remove `WireApi` import, simplify `path()` to always return `"responses"`
4. `codex-api/src/requests/responses.rs` - Update test helper to not set `wire` field
5. `codex-api/tests/clients.rs` - Remove `WireApi` import, update `provider()` helper

**codex-core files to modify:**
1. `core/src/model_provider_info.rs` - Remove `WireApi as ApiWireApi` import, remove `wire` mapping from `to_api_provider()`, move `WireApi::Chat` error check earlier in the function, fix Ollama provider from `WireApi::Chat` to `WireApi::Responses`

**Key observations:**
- Ollama built-in provider currently sets `WireApi::Chat`, which will error at runtime via `to_api_provider()`. Fixing to `WireApi::Responses`.
- The `WireApi::Chat` variant in codex-core is kept for config deserialization compat (existing user config files).
- `codex-core`'s `WireApi` enum and its Chat variant are NOT part of this removal — only the codex-api side.

## Approach: Feature-gate codex-api (future)

Add a `legacy-http-backend` feature to codex-core:
- Makes `codex-api` an optional dependency
- Gates HTTP-backend-only modules with `#[cfg(feature = "legacy-http-backend")]`
- Dev-dependencies enable the feature so all tests pass
- Downstream crates (acp, tui, cli) do NOT enable it
