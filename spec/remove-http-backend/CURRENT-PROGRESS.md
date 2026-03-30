# Current Progress

## Status: Third component removed

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

### Suggested next steps for future commits
1. Remove the `codex_conversation.rs` and `conversation_manager.rs` wrappers (only used by HTTP backend tests — would cascade to ~30 integration test modules, so consider rewriting tests to use ACP path first)
2. Feature-gate `codex-api` dependency in codex-core behind a `legacy-http-backend` cargo feature
3. Eventually: remove the `codex-api` and `codex-client` crates entirely
