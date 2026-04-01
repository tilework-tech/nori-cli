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

## WireApi enum removal from codex-core (next removal target)

After removing `WireApi` from codex-api, the codex-core `WireApi` is a two-variant enum where `Chat` is dead (errors at runtime) and `Responses` is always used. The `wire_api` field on `ModelProviderInfo` is always `Responses` in practice.

**Dependency analysis:**
- `WireApi` is imported by 8 test files and 2 source files (`model_provider_info.rs`, `client.rs`)
- `WireApi` is re-exported from `lib.rs`
- `wire_api` field is set in every `ModelProviderInfo` construction across the test suite
- `create_oss_provider()` and `create_oss_provider_with_base_url()` take a `WireApi` parameter

**Backwards compatibility:**
- `ModelProviderInfo` does NOT have `#[serde(deny_unknown_fields)]`, so serde ignores unknown fields by default
- Existing configs with `wire_api = "chat"` or `wire_api = "responses"` will silently ignore the field — this is actually better than the current runtime error for Chat
- E2E tests reference `wire_api = "acp"` in comments/configs — these are already broken/ignored since there's no Acp variant

**Files to modify (source):**
1. `core/src/model_provider_info.rs` — Remove `WireApi` enum, remove `wire_api` field from `ModelProviderInfo`, remove `WireApi` param from `create_oss_provider*`, remove `Chat` check from `to_api_provider()`
2. `core/src/client.rs` — Remove `WireApi` import, remove match on `wire_api` in `stream()`, call `stream_responses_api` directly
3. `core/src/lib.rs` — Remove `WireApi` re-export

**Files to modify (tests):**
1. `core/tests/suite/client/mod.rs` — Remove `WireApi` import, remove `wire_api` field from provider
2. `core/tests/suite/client/part3.rs` — Remove `wire_api` field
3. `core/tests/suite/client/part4.rs` — Remove `wire_api` field
4. `core/tests/suite/stream_error_allows_next_turn.rs` — Remove `WireApi` import, `wire_api` field
5. `core/tests/suite/stream_no_completed.rs` — Remove `WireApi` import, `wire_api` field
6. `core/tests/responses_headers.rs` — Remove `WireApi` import, `wire_api` field
7. `core/src/config/tests/mod.rs` — Remove `wire_api` from test fixture, remove `chat_wire_api_config_deserializes_but_fails_to_create_provider` test

**Tests to remove:**
- `chat_wire_api_config_deserializes_but_fails_to_create_provider` — tests removed behavior
- `ollama_builtin_provider_creates_successfully` — still valid but `wire_api` field gone from assertion

**Docs to update:**
- `core/docs.md` — Remove WireApi references

## Feature-gating the HTTP backend: detailed cascade analysis

### Downstream crate dependencies on HTTP-backend types

**Critical finding:** None of the downstream crates (tui, cli, acp) import ANY of these HTTP-backend types:
- `ModelClient`, `Prompt`, `ResponseEvent`, `ResponseStream`
- `CodexConversation`, `ConversationManager`, `NewConversation`
- `ModelProviderInfo`, `built_in_model_providers`, `create_oss_provider_with_base_url`

The TUI uses: `codex_core::protocol::*`, `codex_core::config::*`, `codex_core::auth::*`, `codex_core::rollout::*`, and utility modules.
The CLI uses: `codex_core::config::*`, `codex_core::auth::*`, sandbox-related modules.
The ACP uses: `codex_core::config::types::McpServerConfig`, `codex_core::compact::{SUMMARIZATION_PROMPT, SUMMARY_PREFIX}`.

### Cascade from gating `codex/` module

**Problem:** `codex::Session` and `codex::TurnContext` permeate almost every module:
- `tools/` (context.rs, events.rs, sandboxing.rs, parallel.rs, router.rs, orchestrator.rs, handlers/*)
- `tasks/` (all submodules)
- `state/` (session.rs, turn.rs)
- `compact.rs`, `apply_patch.rs`, `environment_context.rs`, `user_shell_command.rs`
- `mcp_tool_call.rs`, `mcp_connection_manager.rs`, `context_manager/`, `unified_exec/`

Gating `codex/` would cascade to gating most of the crate. Not viable for a single commit.

### Safe leaf modules (no reverse dependencies from non-gated code)

These modules are at the "top" of the dependency chain — nothing in core/src/ imports FROM them except lib.rs:
1. `conversation_manager.rs` — imported only by lib.rs
2. `codex_conversation.rs` — imported only by conversation_manager.rs and lib.rs
3. `api_bridge.rs` — imported only by client.rs (HTTP-only, but client.rs is not being gated yet)

### Strategy: Incremental feature-gating

Phase 1 (done): Introduce `legacy-http-backend` feature. Gate leaf modules and HTTP-only re-exports.
Phase 2 (done): Move `to_api_provider()` from `model_provider_info.rs` to `client.rs`, removing codex-api from the shared config module.
Phase 3 (done): Gate `sandboxing/assessment.rs` behind the feature.
Phase 4a (next): Gate HTTP-specific compact functions behind `legacy-http-backend` — `run_inline_auto_compact_task`, `run_compact_task`, `run_compact_task_inner`, `drain_to_completed` use `ModelClient`/`ResponseEvent` and are only called from within the `codex/` module.
Phase 4b (future): Gate `client.rs` and `api_bridge.rs` behind the feature (requires also gating codex/ module).
Phase 5 (future): Make `codex-api` an optional dependency (`dep:codex-api`).
Phase 6 (future): Gate the codex/ module and its cascade (requires separating Session/TurnContext from shared infrastructure).
Phase 7 (future): Remove codex-api and codex-client crates entirely.

## Gating `sandboxing/assessment.rs` behind `legacy-http-backend` (next removal target)

### Why this component

`sandboxing/assessment.rs` is a self-contained HTTP-backend component that:
1. Creates a `ModelClient` (HTTP-backend type) and makes direct HTTP API calls
2. Has exactly one caller: `codex/approval.rs:assess_sandbox_command()`, which is called from `tools/orchestrator.rs`
3. Is behind the `experimental_sandbox_command_assessment` config flag (default: false)
4. Provides no shared functionality used by ACP — the `SandboxCommandAssessment` result TYPE lives in `codex-protocol` and is independent

### Cascade analysis

- `sandboxing/mod.rs:9`: `pub mod assessment;` — gate this
- `codex/approval.rs:4-28`: `assess_sandbox_command()` method on `Session` — the only caller of `assessment::assess_command()`
- `tools/orchestrator.rs:70-79, 151-159`: Two call sites of `sess.assess_sandbox_command()`

### Approach

1. Gate `pub mod assessment;` in `sandboxing/mod.rs` behind `#[cfg(feature = "legacy-http-backend")]`
2. In `codex/approval.rs`: Gate `assess_sandbox_command` behind `#[cfg(feature = "legacy-http-backend")]`, add a `#[cfg(not(...))]` stub that returns `None` — this avoids cascading changes to `tools/orchestrator.rs`
3. The `SandboxCommandAssessment` type, config field, and feature flag remain unchanged (they're shared protocol/config types)

### Why stub instead of gating callers

The `assess_sandbox_command` method is called from `tools/orchestrator.rs` which is compiled unconditionally (shared code). Adding `#[cfg]` to call sites in orchestrator.rs would be messy and brittle. A stub method that always returns `None` when the feature is off:
- Preserves the existing call sites unchanged
- Has the same behavior as `experimental_sandbox_command_assessment = false` (the default)
- Cleanly eliminates the HTTP-backend dependency (`ModelClient`, `codex-api`) from the non-feature-flagged build

## Gating HTTP-specific compact functions behind `legacy-http-backend` (Phase 4a)

### Why this component

`compact.rs` contains a mix of shared and HTTP-backend-specific code. The shared code (constants, utility functions) is used by ACP. The HTTP-specific functions make direct model calls via `ModelClient.stream()` and process `ResponseEvent`s — pure HTTP-backend code.

### HTTP-specific functions (to be gated)

1. `run_inline_auto_compact_task(sess, turn_context)` — auto-compaction triggered during turn execution
2. `run_compact_task(sess, turn_context, input)` — manual compaction task
3. `run_compact_task_inner(sess, turn_context, input)` — shared implementation
4. `drain_to_completed(sess, turn_context, prompt)` — streams model response to completion

HTTP-specific imports used only by these functions:
- `use crate::client_common::Prompt;` — constructs prompts for model calls
- `use crate::client_common::ResponseEvent;` — re-export of `codex_api::common::ResponseEvent`
- `use crate::codex::get_last_assistant_message_from_turn;`

### Shared functions (remain ungated)

- `SUMMARIZATION_PROMPT`, `SUMMARY_PREFIX` — constants used by ACP
- `content_items_to_text()` — utility
- `collect_user_messages()` — utility
- `is_summary_message()` — utility
- `build_compacted_history()` / `build_compacted_history_with_limit()` — history construction

### Callers

- `run_inline_auto_compact_task`: `codex/mod.rs:10` (import), `codex/turn_execution.rs:91` (call)
- `run_compact_task`: `tasks/compact.rs:28` (call)

All callers are inside the `codex/` module — HTTP-backend code compiled unconditionally.

### Approach: Stub pattern (same as sandboxing/assessment.rs)

1. Gate the 4 HTTP-specific functions and their imports behind `#[cfg(feature = "legacy-http-backend")]`
2. Add `#[cfg(not(feature = "legacy-http-backend"))]` no-op stubs for the 2 public functions (`run_inline_auto_compact_task`, `run_compact_task`)
3. Callers don't need to change — stubs have identical signatures, return `()`

### Why stubs work here

- `run_inline_auto_compact_task` and `run_compact_task` both return `()` — no-op stubs are trivially correct
- Nori uses ACP, never reaches these codex/ code paths — the stubs are never called in production
- Dev-dependencies enable `legacy-http-backend`, so all tests use the real implementations
- Stub behavior is equivalent to "compaction not available" — safe for ACP path

### Also gate unused imports in compact.rs

Several imports at the top of compact.rs are only used by the HTTP-specific functions:
- `crate::client_common::Prompt`
- `crate::client_common::ResponseEvent`
- `crate::codex::get_last_assistant_message_from_turn`
- `crate::protocol::{CompactedItem, ContextCompactedEvent, EventMsg, TaskStartedEvent, TurnContextItem, WarningEvent}`
- `crate::truncate::TruncationPolicy`
- `crate::util::backoff`
- `codex_protocol::items::TurnItem` (actually only used by `collect_user_messages` — keep)
- `codex_protocol::protocol::RolloutItem` (only in HTTP functions — gate)
- `futures::prelude::*` (only in HTTP functions — gate)
- `tracing::error` (only in HTTP functions — gate)

## Moving `to_api_provider()` out of `model_provider_info.rs` (Phase 2 analysis)

### Why this step

`model_provider_info.rs` is a shared module — its `ModelProviderInfo` struct is used by Config for all backends. But it imports `codex_api::Provider` and `codex_api::provider::RetryConfig` solely for the `to_api_provider()` method, which converts the config struct into an HTTP API client provider. This couples a shared module to the HTTP backend.

Moving `to_api_provider()` and its helper `build_header_map()` into `client.rs` (where the only production caller lives) concentrates HTTP-backend code in the HTTP-backend module and removes codex-api from the shared config module.

### Current state

- `to_api_provider()` is `pub(crate)` on `ModelProviderInfo` (line 106)
- `build_header_map()` is private, `#[allow(dead_code)]`, called only by `to_api_provider()` (line 81)
- Only production caller: `client.rs:168`
- Test callers: 4 tests in `model_provider_info.rs` that verify Azure detection and Ollama provider creation
- codex-api imports: `Provider as ApiProvider`, `provider::RetryConfig as ApiRetryConfig` (lines 8-9)

### Approach

1. Move `to_api_provider()` to a standalone `pub(crate)` function in `client.rs`: `fn create_api_provider(info: &ModelProviderInfo, auth_mode: Option<AuthMode>) -> Result<ApiProvider>`
2. Move `build_header_map()` as a private helper alongside it
3. Move the 4 HTTP-backend tests (`legacy_wire_api_field_in_config_is_silently_ignored`, `ollama_builtin_provider_creates_successfully`, `detects_azure_responses_base_urls`) to the `client.rs` test module
4. Remove codex-api imports from `model_provider_info.rs`

### Cascading effects

- None. The function signature changes from `self.to_api_provider(auth_mode)` to `create_api_provider(&self.provider, auth_mode)` at one call site in `client.rs`
- Tests move but logic is unchanged
- No external crates call `to_api_provider()` (it's `pub(crate)`)

## Moving ResponseEvent and ResponseStream from `client_common.rs` to `client.rs`

### Why this step

`client_common.rs` is a shared module — its `tools` submodule (ToolSpec, FreeformTool, etc.) is used by both backends. But it has one production import from `codex-api`: `pub use codex_api::common::ResponseEvent;` (line 4). The `ResponseStream` type (lines 233-243) wraps `ResponseEvent` in a channel-based stream. Both types are pure HTTP-backend types never used by ACP.

Moving these two types into `client.rs` (which is already a pure HTTP-backend module) removes `codex-api` from the shared module's production code. This directly enables making `codex-api` an optional dependency in a future commit.

### Current state

- `client_common.rs:4`: `pub use codex_api::common::ResponseEvent;` — sole production codex-api import
- `client_common.rs:233-243`: `ResponseStream` struct and `Stream` impl — depends on `ResponseEvent`
- `client.rs:39-40`: imports both from `crate::client_common`
- Tests in `client_common.rs:245-419`: three tests use `codex_api` types (ResponsesApiRequest, etc.) — already `#[cfg(test)]`, harmless

### Consumers of ResponseEvent from client_common

1. `client.rs:39` — `use crate::client_common::ResponseEvent;` (will become local)
2. `codex/mod.rs:59` — `use crate::client_common::ResponseEvent;` → change to `crate::client::ResponseEvent`
3. `compact.rs:16` — gated: `use crate::client_common::ResponseEvent;` → change to `crate::client::ResponseEvent`
4. `sandboxing/assessment.rs:11` — gated: `use crate::client_common::ResponseEvent;` → change to `crate::client::ResponseEvent`
5. `lib.rs:116` — gated: `pub use client_common::ResponseEvent;` → change to `pub use client::ResponseEvent;`

### Consumers of ResponseStream from client_common

1. `client.rs:40` — `use crate::client_common::ResponseStream;` (will become local)
2. `lib.rs:118` — gated: `pub use client_common::ResponseStream;` → change to `pub use client::ResponseStream;`

### Approach

1. Add `pub use codex_api::common::ResponseEvent;` to `client.rs`
2. Move `ResponseStream` struct and `Stream` impl to `client.rs`
3. Remove both from `client_common.rs`
4. Remove `use crate::client_common::{ResponseEvent, ResponseStream}` from `client.rs` (now local)
5. Update import paths in consumers (codex/mod.rs, compact.rs, sandboxing/assessment.rs, lib.rs)

### Cascading effects

- Pure import-path changes. No behavioral changes.
- All consumers of `ResponseEvent`/`ResponseStream` already import from `crate::client_common` — they just change to `crate::client`
- The `ResponseStream` struct's `rx_event` field is `pub(crate)` and only accessed in `client.rs` — moving it there makes the field truly private
- Test code in `client_common.rs` is unaffected (tests import codex-api types via `#[cfg(test)]`)

### After this change

- `client_common.rs` production imports: zero from codex-api
- `client.rs` + `api_bridge.rs` are the only two source files with production codex-api imports
- Direct prerequisite for making `codex-api` an optional dependency (`dep:codex-api`) behind `legacy-http-backend`

## Making `codex-api` an optional dependency (Phase 5 — current target)

### Why this step

After all previous gating work, only `client.rs` and `api_bridge.rs` have production `codex-api` imports. Both are HTTP-backend-only modules. Making `codex-api` optional behind `legacy-http-backend` means the nori binary's dependency tree no longer includes `codex-api` (or its transitive deps like `codex-client`, `eventsource-stream` SSE parser, etc.).

### Prerequisites verified

1. `client_common.rs` — zero production codex-api imports (done in previous commit)
2. `model_provider_info.rs` — codex-api imports moved to client.rs (done)
3. `compact.rs` — HTTP-specific functions gated (done)
4. `sandboxing/assessment.rs` — gated at module level (done)

### Modules to gate behind `legacy-http-backend`

**Must gate (directly import codex-api):**
1. `mod client;` (lib.rs:12)
2. `pub(crate) mod api_bridge;` (lib.rs:8)

**Must gate (import from `crate::client`):**
3. `pub(crate) mod codex;` (lib.rs:14) — imports `ModelClient`, `ResponseEvent` from client.rs

**Must gate (import from `crate::codex`):**
The `codex/` module's `Session` and `TurnContext` types permeate these modules:
4. `mod tools;` (lib.rs:78)
5. `mod state;` (lib.rs:92)
6. `mod tasks;` (lib.rs:93)
7. `mod function_tool;` (lib.rs:91)
8. `mod mcp_tool_call;` (lib.rs:38)
9. `mod mcp_connection_manager;` (lib.rs:34) — has public re-exports: `MCP_SANDBOX_STATE_CAPABILITY`, `MCP_SANDBOX_STATE_NOTIFICATION`, `SandboxState`
10. `mod context_manager;` (lib.rs:22, line 22)
11. `mod unified_exec;` (lib.rs:48)
12. `mod user_shell_command;` (lib.rs:97)
13. `mod response_processing;` (lib.rs:43)
14. `mod event_mapping;` (lib.rs:59)
15. `mod message_history;` (lib.rs:39)
16. `mod user_notification;` (lib.rs:94) — has public re-exports: `UserNotification`, `UserNotifier`
17. `mod apply_patch;` (lib.rs:9) — has public re-export: `CODEX_APPLY_PATCH_ARG1`
18. `mod environment_context;` (lib.rs:24)
19. `mod truncate;` (lib.rs:47)

### Key insight: Public re-exports from gated modules

Several gated modules have public re-exports in lib.rs:
- `mcp_connection_manager`: `MCP_SANDBOX_STATE_CAPABILITY`, `MCP_SANDBOX_STATE_NOTIFICATION`, `SandboxState`
- `user_notification`: `UserNotification`, `UserNotifier`
- `apply_patch`: `CODEX_APPLY_PATCH_ARG1`
- `event_mapping`: `parse_turn_item`

**These re-exports may be used by downstream crates (tui, cli, acp).** Must verify before gating.

### Downstream crate usage (verified)

- TUI uses: `protocol::*`, `config::*`, `auth::*`, `rollout::*`, utility modules
- CLI uses: `config::*`, `auth::*`, sandbox-related modules
- ACP uses: `config::types::McpServerConfig`, `compact::{SUMMARIZATION_PROMPT, SUMMARY_PREFIX}`

None of them use `ModelClient`, `ResponseEvent`, `TurnContext`, `Session`, or any of the HTTP-backend types. But we need to check if they use the re-exported types from modules we'd be gating.

### Approach: Compiler-driven gating

Rather than tracing every dependency manually:
1. Gate `client`, `api_bridge`, `codex` in lib.rs
2. Make `codex-api` optional in Cargo.toml
3. Run `cargo check -p codex-core` (without features) to see what breaks
4. Gate each broken module, and also gate its re-exports if they're only used by HTTP-backend code
5. For re-exports used by downstream crates, move the underlying types to shared modules
6. Iterate until it compiles cleanly

### Cargo.toml changes

```toml
# In [dependencies]:
codex-api = { workspace = true, optional = true }

# In [features]:
legacy-http-backend = ["dep:codex-api"]
```

Dev-dependencies already enable `legacy-http-backend`, so all tests will continue to compile.

## Fix compilation without `legacy-http-backend` (current target)

### Problem

After the WIP commit that gated many modules behind `legacy-http-backend`, `cargo check -p codex-core` (without features) fails with 21 errors. Two root causes:

### Root cause 1: `compact.rs` stubs reference gated types

The `#[cfg(not(feature = "legacy-http-backend"))]` stubs for `run_inline_auto_compact_task` and `run_compact_task` reference `Session` and `TurnContext` types from the `codex/` module, which is itself gated behind `legacy-http-backend`.

**Why the stubs are unnecessary:** ALL callers are in gated modules:
- `run_inline_auto_compact_task`: called from `codex/turn_execution.rs` (gated via `codex/` module)
- `run_compact_task`: called from `tasks/compact.rs` (gated via `tasks/` module)

When the feature is off, no code calls these functions. The stubs serve no purpose.

**Fix:** Remove both stubs entirely.

### Root cause 2: `tools/spec/mod.rs` imports from gated modules

`tools/spec/mod.rs` is always compiled (`pub mod spec;` in `tools/mod.rs`), but it imports from:
- `tools::handlers` (gated) — `PLAN_TOOL`, `ApplyPatchToolType`, `create_apply_patch_*_tool`
- `tools::registry` (gated) — `ToolRegistryBuilder`

These imports are used in two places:
1. `ToolsConfig` struct — uses `ApplyPatchToolType` (available from `tool_types.rs`)
2. `build_specs()` function — constructs `ToolRegistryBuilder`, registers handlers

**Fix:**
1. Change `ApplyPatchToolType` import from `crate::tools::handlers::apply_patch` to `crate::tool_types` (always available)
2. Gate `build_specs()` function behind `#[cfg(feature = "legacy-http-backend")]`
3. Gate the remaining `handlers`/`registry` imports behind the feature
4. The `PLAN_TOOL` import (used only by `build_specs`) gets gated along with `build_specs`
5. The `create_apply_patch_*` imports (used only by `build_specs`) get gated too

### Verification

After fixes:
- `cargo check -p codex-core` (no features) should succeed
- `cargo check -p codex-core --features legacy-http-backend` should succeed
- `cargo test -p codex-core` (dev-deps enable the feature) should pass all tests
- `cargo check -p nori-tui` and `cargo check -p codex-acp` should succeed

## Fix workspace compilation with gated modules (current target)

### Problem

After the WIP commit that gated many modules and made `codex-api` optional, `cargo check --workspace` reveals two remaining breakages:

### Root cause 1: `CODEX_APPLY_PATCH_ARG1` gated behind `legacy-http-backend`

`codex-arg0` imports `codex_core::CODEX_APPLY_PATCH_ARG1` unconditionally (used for argv dispatch and Windows batch scripts), but the re-export at `lib.rs:119-120` is gated behind `legacy-http-backend`. The constant is defined in `apply_patch.rs` which is entirely gated because it imports `Session`/`TurnContext`.

The constant itself (`"--codex-run-as-apply-patch"`) has zero HTTP-backend dependency. It's a simple CLI argument string.

**Fix:** Move `CODEX_APPLY_PATCH_ARG1` to `tool_types.rs` (the shared module for types extracted from gated modules — same pattern used for `ApplyPatchToolType`, `ConfigShellToolType`, etc.). Then:
1. Add `pub const CODEX_APPLY_PATCH_ARG1: &str = "--codex-run-as-apply-patch";` to `tool_types.rs`
2. Change `apply_patch.rs` to import from `crate::tool_types::CODEX_APPLY_PATCH_ARG1`
3. Change `lib.rs` re-export from gated `apply_patch::CODEX_APPLY_PATCH_ARG1` to ungated `tool_types::CODEX_APPLY_PATCH_ARG1`
4. `codex-arg0` import remains unchanged

### Root cause 2: `core_test_support` imports gated types without feature

`core_test_support` (at `core/tests/common/`) imports `CodexConversation` and `ConversationManager` which are gated behind `legacy-http-backend`. This crate is purely a test helper — it makes sense for it to require the feature.

**Fix:** Add `features = ["legacy-http-backend"]` to `core_test_support`'s `codex-core` dependency in `core/tests/common/Cargo.toml`.

### Verification

- `cargo check --workspace` should succeed with no errors
- `cargo test -p codex-core` should pass all tests
- `cargo check -p nori-tui` should succeed (via codex-arg0)
- `cargo check -p codex-acp` should succeed
