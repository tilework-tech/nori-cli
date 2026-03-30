# Current Progress

## Status: Sixth component removed

### Completed: Remove compact_remote module

Removed the remote compaction module (`compact_remote.rs`) from codex-core. This was an HTTP-backend-specific component that made direct HTTP calls to OpenAI's `/v1/responses/compact` endpoint for conversation history compaction.

**What was removed:**
- `compact_remote.rs` - the remote compaction implementation
- `Feature::RemoteCompaction` flag from the features system
- `should_use_remote_compact_task()` routing function
- 3 integration tests for remote compaction
- Test helpers: `mount_compact_json_once`, `mount_compact_json_once_match`, `compact_mock`

**What was simplified:**
- Auto-compaction in `turn_execution.rs` now always uses local compaction
- Manual compact task in `tasks/compact.rs` now always uses local compaction
- No more branching based on auth mode (ChatGPT vs API key)

**Impact:** -555 lines of HTTP-backend-specific code removed. All existing tests pass.

### Completed: Remove compact endpoint from codex-api

Removed the compact endpoint module from the `codex-api` crate. This was dead code after the previous commit removed `compact_remote.rs` from codex-core — no external consumers remained.

**What was removed:**
- `codex-api/src/endpoint/compact.rs` - entire `CompactClient` implementation and tests
- `CompactionInput` struct from `common.rs`
- `WireApi::Compact` enum variant from `provider.rs`
- Re-exports of `CompactClient` and `CompactionInput` from `lib.rs`

**What was simplified:**
- `endpoint/responses.rs` match arm: `WireApi::Responses | WireApi::Compact` → `WireApi::Responses`
- `WireApi` enum now only has `Responses` and `Chat` variants

**Impact:** ~170 lines of HTTP-backend-specific code removed. All existing tests pass (codex-api: 8 tests, codex-core: 439 pass, 23 pre-existing environment failures from nvm shell pollution).

### Completed: Remove Chat Completions wire protocol

Removed the entire Chat Completions (`WireApi::Chat`) wire protocol implementation. This was one of two HTTP wire APIs — the other being `WireApi::Responses` which the integration test suite uses for mocking. The nori binary uses ACP exclusively and never touches either wire protocol.

**What was removed:**
- `codex-api/src/endpoint/chat.rs` - `ChatClient` and `AggregatedStream` (AggregatedStream moved to new `aggregate.rs`)
- `codex-api/src/requests/chat.rs` - `ChatRequestBuilder` request body construction
- `codex-api/src/sse/chat.rs` - `spawn_chat_stream`, `process_chat_sse` SSE parser
- `WireApi::Chat` variant from codex-api's `WireApi` enum
- `stream_chat_completions()` method from codex-core's `ModelClient`
- `create_tools_json_for_chat_completions_api()` from tools
- `core/tests/chat_completions_sse.rs` - 8 SSE streaming tests
- `core/tests/chat_completions_payload.rs` - 7 request payload tests
- `stdio_image_completions_round_trip` test from `rmcp_client.rs` (was `#[ignore]`d)
- 4 Chat-specific URL routing tests from codex-api
- Re-exports: `ChatClient`, `ChatRequest`, `ChatRequestBuilder` from codex-api lib.rs

**What was simplified:**
- `ModelClient::stream()` now only handles `WireApi::Responses`; `WireApi::Chat` returns `UnsupportedOperation` error
- `ResponsesClient::path()` now always returns `"responses"` (no more Chat path fallback)
- Default `WireApi` changed from `Chat` to `Responses`
- Ollama built-in provider changed from `WireApi::Chat` to `WireApi::Responses`

**What was preserved for backwards compatibility:**
- `WireApi::Chat` variant still exists in codex-core's enum so config files with `wire_api = "chat"` still deserialize
- `AggregatedStream` and `AggregateStreamExt` moved to `codex-api/src/endpoint/aggregate.rs` (shared functionality, used by Responses stream aggregation)

**Impact:** ~1200 lines of Chat Completions code removed across codex-api and codex-core. All existing tests pass (codex-api: 16 tests, codex-core: 535 pass, 2 pre-existing nvm environment failures).

### Completed: Remove WireApi enum from codex-api

Removed the `WireApi` enum from `codex-api` entirely. After removing Chat Completions, this was a single-variant enum (`Responses` only) — a pointless abstraction. Also fixed the Ollama built-in provider which was incorrectly set to `WireApi::Chat` (would error at runtime).

**What was removed:**
- `WireApi` enum from `codex-api/src/provider.rs`
- `wire` field from `codex-api::Provider` struct
- `WireApi` re-export from `codex-api/src/lib.rs`
- `WireApi` import and match in `ResponsesClient::path()` (now returns `"responses"` directly)
- Dead `wire != Responses` check in `is_azure_responses_endpoint()`
- `WireApi as ApiWireApi` import from `codex-core/src/model_provider_info.rs`

**What was simplified:**
- `codex-api::Provider` no longer carries a wire format selector — it always uses Responses API
- `to_api_provider()` in codex-core: `WireApi::Chat` error check moved to top of function, no longer constructs `wire` field
- Ollama built-in provider: `WireApi::Chat` → `WireApi::Responses` (fixes runtime error)

**What was preserved:**
- `WireApi::Chat` variant in codex-core's enum still exists for config deserialization backwards compatibility

**Tests added:**
- `chat_wire_api_config_deserializes_but_fails_to_create_provider` — verifies that `wire_api = "chat"` in config deserializes but errors at provider creation
- `ollama_builtin_provider_creates_successfully` — verifies Ollama built-in creates without error

**Impact:** Net -20 lines across codex-api and codex-core. All existing tests pass (codex-api: 16 tests, codex-core: 541 unit + 440 integration pass, 22 pre-existing nvm environment failures).

### Completed: Remove WireApi enum from codex-core

Removed the `WireApi` enum from codex-core entirely. After previous commits removed `WireApi` from codex-api and made the `Chat` variant a dead error path, the enum was a two-variant type (`Responses` / `Chat`) where one variant was always used and the other always errored. The `wire_api` field on `ModelProviderInfo` was always `Responses` in practice.

**What was removed:**
- `WireApi` enum from `core/src/model_provider_info.rs`
- `wire_api` field from `ModelProviderInfo` struct
- `WireApi` re-export from `core/src/lib.rs`
- `WireApi` parameter from `create_oss_provider()` and `create_oss_provider_with_base_url()`
- `WireApi::Chat` error check from `to_api_provider()`
- `WireApi::Chat` match arm from `ModelClient::stream()`
- `chat_wire_api_config_deserializes_but_fails_to_create_provider` test (tested removed behavior)
- `WireApi` imports and `wire_api` field references from 8 test files

**What was simplified:**
- `ModelClient::stream()` now directly calls `stream_responses_api()` — no more dispatch logic
- `ModelProviderInfo` no longer carries a wire protocol selector
- `create_oss_provider_with_base_url()` no longer takes a `WireApi` parameter
- Config files with `wire_api = "chat"` or `wire_api = "responses"` silently ignore the unknown field (serde default behavior) — better than the previous runtime error for Chat

**Impact:** Net ~-70 lines across codex-core source and tests. All existing tests pass (codex-core: 536 unit, 440 integration pass, 22 pre-existing nvm environment failures; codex-api: 2 tests; E2E: 6 tests).

### Completed: Introduce `legacy-http-backend` feature flag and gate HTTP-backend public API

Introduced a `legacy-http-backend` cargo feature in codex-core to begin gating HTTP-backend-only code. When the feature is OFF (which it is for all downstream crates: nori-tui, nori-cli, codex-acp), the HTTP-backend types are invisible in codex-core's public API.

**What was gated behind `#[cfg(feature = "legacy-http-backend")]`:**
- `codex_conversation` module and `CodexConversation` re-export
- `conversation_manager` module and `ConversationManager` / `NewConversation` re-exports
- Re-exports of `ModelClient`, `Prompt`, `ResponseEvent`, `ResponseStream`

**What was simplified:**
- `pub mod api_bridge;` → `pub(crate) mod api_bridge;` (no external consumers)
- `pub mod codex;` → `pub(crate) mod codex;` (no external consumers)
- `compact.rs`: `use crate::Prompt` → `use crate::client_common::Prompt` (direct path instead of gated re-export)
- Clippy: removed unused `use serde_json;` from codex/mod.rs

**What was preserved:**
- Dev-dependencies enable `legacy-http-backend`, so all existing tests compile and pass
- All internal module implementations unchanged — only lib.rs declarations/re-exports and Cargo.toml modified
- No behavioral changes

**Impact:** 6 HTTP-backend types removed from codex-core's default public API. All existing tests pass (codex-core: 537 unit, 441 integration pass, 21 pre-existing nvm environment failures; E2E: 6 tests).

### Suggested next steps for future commits
1. Gate `client.rs`, `api_bridge.rs`, and `sandboxing/assessment.rs` behind `legacy-http-backend` (requires also gating compact.rs HTTP-only functions)
2. Make `codex-api` an optional dependency (`dep:codex-api`) enabled by `legacy-http-backend`
3. Gate the `codex/` module and its cascade (Session/TurnContext permeate tools/, tasks/, state/, etc. — requires separating shared infrastructure from HTTP-specific orchestration)
4. Eventually: remove the `codex-api` and `codex-client` crates entirely
