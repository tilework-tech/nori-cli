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

## Remove remaining dead test infrastructure (fourteenth removal)

### Why this component

After removing `codex-api` and all `legacy-http-backend` gated code (~45,000 lines), several modules survive purely as `#[cfg(test)]` infrastructure for testing HTTP-backend behavior that no longer exists in production. These modules confuse anyone reading the codebase because they define OpenAI Responses API types (`ToolSpec`, `JsonSchema`, `ResponsesApiTool`) that have no production consumers.

### Files to remove entirely

1. **`tools/spec/tests.rs`** — 8 tests for `mcp_tool_to_openai_tool()` conversion (dead production code)
2. **`tools/spec/mod.rs`** — 330 lines, entirely `#[cfg(test)]`: `JsonSchema`, `AdditionalProperties`, `create_shell_tool()`, `create_shell_command_tool()`, `mcp_tool_to_openai_tool()`, `sanitize_json_schema()`
3. **`client_common.rs`** — 127 lines, entirely `#[cfg(test)]`: `ToolSpec`, `FreeformTool`, `ResponsesApiTool` types + one test
4. **`rollout/error.rs`** — 2-line empty placeholder module

### Files to edit

1. **`tools/mod.rs`** — remove `pub mod spec;` (module becomes empty, remove entirely)
2. **`lib.rs`** — remove `mod client_common;`, `mod tools;`
3. **`rollout/mod.rs`** — remove `pub(crate) mod error;`
4. **`core/docs.md`** — remove references to `client_common.rs`, `tools/spec/`, update module structure description

### Test to preserve (move to `model_family.rs`)

The test `get_full_instructions_no_user_content` in `client_common.rs` tests `find_family_for_model()` and `needs_special_apply_patch_instructions` — this is still-valuable model family behavior testing. However, it uses `ToolSpec` unnecessarily (creates an empty vec, checks it's empty). The test should be simplified and moved to `model_family.rs`.

### Dependency analysis (verified)

- `client_common::tools::*` is only imported by `tools/spec/mod.rs` (test-only)
- `tools::spec::JsonSchema` is only imported by `client_common.rs` (test-only)
- Circular test-only dependency between `client_common` and `tools/spec` — both removable together
- `rollout/error.rs` is only referenced by `rollout/mod.rs` via `pub(crate) mod error;`
- No downstream crates reference any of these modules

## Remove orphaned Feature enum variants (fifteenth removal)

### Why this component

After removing the HTTP backend, four `Feature` enum variants in `features.rs` have zero consumers anywhere in the codebase. They exist only in the `FEATURES` registry array but no code ever calls `.enabled(Feature::X)` for them. They confuse readers by implying nori has capabilities (ghost commit gating, exec policy gating, parallel tool calls, shell tool gating) that are either unconditional or removed.

### Variants to remove

| Variant | Key | Default | Stage | Why dead |
|---------|-----|---------|-------|----------|
| `GhostCommit` | `"undo"` | `true` | Stable | Ghost commit functionality in `codex_git` and `acp/src/undo.rs` operates unconditionally — never consulted this flag |
| `ExecPolicy` | `"exec_policy"` | `true` | Experimental | Exec policy enforcement runs unconditionally via `codex-execpolicy` crate |
| `ParallelToolCalls` | `"parallel"` | `false` | Experimental | No `.enabled()` call exists; parallel tool calls were an HTTP-backend concept |
| `ShellTool` | `"shell_tool"` | `true` | Stable | Shell tool types (`ConfigShellToolType`) exist independently; never gated by this flag |

### Verification

- Searched all `.rs` files for `Feature::GhostCommit`, `Feature::ExecPolicy`, `Feature::ParallelToolCalls`, `Feature::ShellTool` — only hits are in `features.rs` FEATURES array
- Searched for feature key strings `"undo"`, `"exec_policy"`, `"parallel"`, `"shell_tool"` in config/tests — no matches
- No legacy aliases reference these features in `legacy.rs`
- No test fixtures or TOML files reference these keys
- No downstream crates reference these variants

### Files to modify

1. **`core/src/features.rs`** — Remove 4 enum variants and their corresponding `FeatureSpec` entries from the `FEATURES` array
2. **`core/docs.md`** — Remove any feature flag references if present (verified: none specific to these variants)

### Impact

- Pure dead code removal: ~30 lines
- No behavioral changes — these flags were never read
- Existing config files with `undo = false` or `shell_tool = false` in `[features]` will trigger "unknown feature key in config" warning instead of silently being accepted — this is the correct behavior since the flags had no effect anyway

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

## Remove `codex-client` crate by inlining into `codex-api` (current target)

### Why this component

`codex-client` is a 331-line crate (7 source files) providing HTTP transport, SSE streaming, retry logic, and request/response types. It has exactly ONE consumer: `codex-api`. Since `codex-api` is already behind the `legacy-http-backend` feature flag and optional in `codex-core`, `codex-client` is effectively dead code for the nori binary. Having it as a separate workspace member adds confusion.

### What `codex-client` exports (used by `codex-api`)

| Type | File | Usage in codex-api |
|------|------|--------------------|
| `HttpTransport` trait | transport.rs | endpoint/responses.rs, endpoint/streaming.rs |
| `ReqwestTransport` struct | transport.rs | lib.rs (re-export) |
| `ByteStream` type alias | transport.rs | sse/responses.rs |
| `StreamResponse` struct | transport.rs | sse/responses.rs, endpoint/streaming.rs, telemetry.rs |
| `Request` struct | request.rs | telemetry.rs, auth.rs, provider.rs |
| `Response` struct | request.rs | telemetry.rs |
| `TransportError` enum | error.rs | sse/responses.rs, error.rs, telemetry.rs, lib.rs (re-export) |
| `StreamError` enum | error.rs | NOT used by codex-api |
| `RetryPolicy` struct | retry.rs | provider.rs, telemetry.rs |
| `RetryOn` struct | retry.rs | provider.rs |
| `run_with_retry` fn | retry.rs | telemetry.rs |
| `backoff` fn | retry.rs | NOT used by codex-api |
| `RequestTelemetry` trait | telemetry.rs | endpoint/responses.rs, endpoint/streaming.rs, telemetry.rs, lib.rs (re-export) |
| `sse_stream` fn | sse.rs | NOT used by codex-api |

### Approach: Inline as `codex-api/src/client/` submodule

1. Create `codex-api/src/client/` directory with all 7 source files from `codex-client/src/`
2. Add `pub(crate) mod client;` to `codex-api/src/lib.rs`
3. Change all `codex_client::X` imports to `crate::client::X`
4. Change 3 re-exports in lib.rs from `codex_client::X` to `crate::client::X`
5. Remove `codex-client` dep from `codex-api/Cargo.toml`
6. Add missing deps to `codex-api/Cargo.toml`: `reqwest` (json, stream), `rand`
7. Remove `codex-client` from workspace members + workspace deps in root `Cargo.toml`
8. Delete `codex-client/` directory entirely
9. Remove `codex-client` from `workspace.metadata.cargo-shear.ignored` if present
10. Update docs: `codex-client/docs.md`, `codex-client/README.md` (deleted with crate), `codex-rs/docs.md`, `codex-rs/core/docs.md`

### Dependencies to add to codex-api

codex-client's Cargo.toml deps not already in codex-api:
- `reqwest = { workspace = true, features = ["json", "stream"] }` — needed by `ReqwestTransport`
- `rand = { workspace = true }` — needed by `backoff()` jitter

Already present in codex-api: async-trait, bytes, futures, http, serde, serde_json, thiserror, tokio, eventsource-stream

### Backwards compatibility

- No external consumers of `codex-client` exist (only `codex-api`)
- `codex-api` re-exports `RequestTelemetry`, `ReqwestTransport`, `TransportError` from `codex-client` — these become re-exports from `crate::client` instead; external API unchanged
- `codex-core` imports these re-exports from `codex-api`, not `codex-client` directly — no changes needed

### Edge cases

- `sse_stream` and `backoff` are public in `codex-client` but NOT used by `codex-api` — they become `pub(crate)` or dead code in the inlined module. Keep them as-is since the module is `pub(crate)`.
- `StreamError` is exported from `codex-client` but NOT used by `codex-api` — it's used by `sse_stream` internally, so it stays in the inlined module.
- The `eventsource-stream` dependency is used by both `codex-client/src/sse.rs` and `codex-api/src/sse/responses.rs` — already in codex-api's deps, no conflict.

## Full removal of codex-api crate and all gated code (current target)

### Why this step

After 12 incremental commits, ALL HTTP-backend code in codex-core is properly gated behind `legacy-http-backend`. The `codex-api` crate is optional. No production binary enables the feature. The gating was preparation for this: delete everything gated.

### Scope

**Crate to remove:**
- `codex-api/` — 27 files, ~2,658 lines

**Gated source modules to delete (~16,245 lines):**
- `api_bridge.rs`, `apply_patch.rs`, `client.rs`, `codex_conversation.rs`, `conversation_manager.rs`
- `environment_context.rs`, `function_tool.rs`, `mcp_connection_manager.rs`, `mcp_tool_call.rs`
- `message_history.rs`, `response_processing.rs`, `user_shell_command.rs`
- `codex/` directory (10 files), `context_manager/` (4 files), `state/` (4 files)
- `tasks/` (6 files), `unified_exec/` (4 files)
- `sandboxing/assessment.rs`

**Gated tool submodules to delete (~5,587 lines):**
- `tools/context.rs`, `tools/events.rs`, `tools/handlers/` (12 files)
- `tools/orchestrator.rs`, `tools/parallel.rs`, `tools/registry.rs`
- `tools/router.rs`, `tools/runtimes.rs`, `tools/sandboxing.rs`

**Integration tests to remove (~21,233 lines):**
- 32 test modules that depend on `TestCodex`/`ConversationManager`/`CodexConversation`
- `core_test_support` modules: `test_codex.rs`, `responses.rs`
- Standalone: `responses_headers.rs`

**Files to EDIT (remove gated blocks):**
- `core/Cargo.toml` — remove feature, remove optional codex-api dep
- `core/src/lib.rs` — remove all `#[cfg(feature = "legacy-http-backend")]` blocks
- `core/src/error.rs` — remove gated variants, structs, impls, tests
- `core/src/compact.rs` — remove gated functions and imports
- `core/src/sandboxing/mod.rs` — remove gated module declaration
- `core/src/tools/mod.rs` — remove gated submodule declarations
- `core/src/tools/spec/mod.rs` — remove gated function and imports
- `core/tests/suite/mod.rs` — remove test module declarations
- `core/tests/common/lib.rs` — remove gated exports
- `core/tests/common/Cargo.toml` — remove feature reference
- Workspace `Cargo.toml` — remove codex-api member and dependency

**Tests that survive (6 files):**
- `suite/auth_refresh.rs`, `suite/exec.rs`, `suite/live_cli.rs`
- `suite/rollout_list_find.rs`, `suite/seatbelt.rs`, `suite/text_encoding_fix.rs`

### Backwards compatibility

- Config files with `wire_api = "chat"` or `wire_api = "responses"` silently ignore the unknown field (serde default behavior)
- `experimental_sandbox_command_assessment` config field remains (in codex-protocol), but the implementation is removed — effectively always disabled
- `reqwest` dependency stays (used by `default_client.rs` and `auth.rs`)

### Risk mitigation

- `cargo build --bin nori` must succeed (this was already true with feature off)
- `cargo test -p codex-core` with remaining tests must pass
- E2E tests (`tui-pty-e2e`) must pass
- The removed integration tests tested HTTP-backend behavior, not ACP behavior

## Gate HTTP-specific error types behind `legacy-http-backend` (current target)

### Why this component

`error.rs` defines `CodexErr`, the public error enum of codex-core. It contains ~15 variants that are only constructed in HTTP-backend code (already gated behind the feature), plus ~5 supporting struct types that reference `reqwest` types. None of these are used by TUI, CLI, or ACP crates. Interns and agents reading the error type see HTTP concepts (StatusCode, reqwest::Error, "stream disconnected", "retry limit") that don't apply to ACP — a direct source of confusion.

### Variants only constructed in gated (HTTP-backend) code

Verified by searching all non-gated modules. Only these are constructed in non-gated code: `Io`, `Sandbox(*)`, `LandlockSandboxExecutableNotProvided`, `UnsupportedOperation`, `Fatal`, `EnvVar`, `Json` (from), `TokioJoin` (from), `LandlockRuleset` (from), `LandlockPathFd` (from).

All other variants are ONLY constructed in feature-gated modules: `client.rs`, `api_bridge.rs`, `codex/`, `compact.rs` (gated functions), `codex_conversation.rs`, etc.

### Methods that use HTTP-specific variants

- `to_codex_protocol_error()` — matches on HTTP-specific variants to map to protocol errors
- `to_error_event()` — calls `to_codex_protocol_error()`
- `http_status_code_value()` — matches on `RetryLimit`, `UnexpectedStatus`, `ConnectionFailed`, `ResponseStreamFailed`
- `get_error_message_ui()` — only matches `Sandbox` variants (shared), wildcard for rest

All EXTERNAL callers of these methods are in gated modules:
- `tools/orchestrator.rs` → `get_error_message_ui` (gated)
- `compact.rs:143,160` → `to_error_event` (gated functions)
- `codex/event_emission.rs:78` → `http_status_code_value` (gated)
- `codex/turn_execution.rs:120` → `to_error_event` (gated)

### Helper structs to gate

1. `UnexpectedResponseError` — uses `reqwest::StatusCode`
2. `ConnectionFailedError` — uses `reqwest::Error`
3. `ResponseStreamFailed` — uses `reqwest::Error`
4. `RetryLimitReachedError` — uses `reqwest::StatusCode`
5. `UsageLimitReachedError` — uses token_data types (HTTP-specific usage limits)
6. `RefreshTokenFailedError` / `RefreshTokenFailedReason` — HTTP auth refresh

### Approach

1. Gate ~15 HTTP-specific `CodexErr` variants behind `#[cfg(feature = "legacy-http-backend")]`
2. Gate the 6 helper structs and their Display/Error impls
3. Gate `to_codex_protocol_error()`, `to_error_event()`, `http_status_code_value()` methods entirely (all callers are in gated code)
4. Keep `get_error_message_ui()` always available but note it already uses wildcard for non-sandbox errors
5. Gate the `reqwest::StatusCode` import (production), `codex_protocol::ConversationId` import, and `From<CancelErr>` impl (already gated)
6. Gate the `ProcessedResponseItem` import (already gated)
7. Gate the HTTP-specific test functions that construct gated error types
8. Keep the `to_error_event_handles_response_stream_failed` test and other HTTP tests gated

### Backwards compatibility

- `reqwest` remains a non-optional dependency (used by `default_client.rs` and `auth.rs`)
- `CodexErr` still has all shared variants for non-HTTP builds
- dev-dependencies enable `legacy-http-backend`, so all tests compile unchanged
- No downstream crate (TUI, CLI, ACP) references any of the gated variants or methods

### Edge cases

- `get_error_message_ui` has a wildcard `_` arm, so it works regardless of how many variants exist
- `downcast_ref` method is generic and doesn't reference specific variants — stays un-gated
- The `CLOUDFLARE_BLOCKED_MESSAGE` const is only used by `UnexpectedResponseError::friendly_message` — gate with the struct
- `retry_suffix`, `retry_suffix_after_or`, `format_retry_timestamp`, `day_suffix` helpers are only used by `UsageLimitReachedError::Display` — gate with the struct
- `NOW_OVERRIDE` test thread-local is only used by gated tests — gate with tests
- `with_now_override` test helper — gate with tests

## `is_dangerous_command.rs` removal (next removal target)

### What it is

The `is_dangerous_command.rs` file and its companion `windows_dangerous_commands.rs` form the "command danger assessment" component of the HTTP backend. This was the counterpart to `is_safe_command.rs` — while `is_safe_command` determines commands that are safe to auto-approve, `is_dangerous_command` determined commands that require extra caution.

### Current state

- **`is_dangerous_command.rs`**: Entire file content (beyond a Windows-only path attribute at lines 1-3) is wrapped in `#[cfg(test)] mod tests`. All functions (`requires_initial_appoval`, `command_might_be_dangerous`, `is_dangerous_to_call_with_exec`) are test-only. No production code.
- **`windows_dangerous_commands.rs`**: Contains production-level code (`is_dangerous_command_windows`, URL detection, PowerShell danger patterns) but its **only** importer is `is_dangerous_command.rs` via `#[cfg(windows)] #[path = ...]`. Since the importer is entirely `#[cfg(test)]`, this file only compiles in Windows test builds.
- **Zero callers**: No production code anywhere calls any function from either file.
- `is_dangerous_command` is declared `pub` in `command_safety/mod.rs` but the parent `command_safety` module is private in `lib.rs`, so it's not reachable from outside the crate.

### Dependency graph

```
command_safety/mod.rs
  ├── is_safe_command.rs   ← KEEP (production, re-exported in lib.rs)
  │   └── windows_safe_commands.rs  ← KEEP (used by is_safe_command)
  ├── is_dangerous_command.rs  ← REMOVE (entirely #[cfg(test)])
  │   └── windows_dangerous_commands.rs  ← REMOVE (only imported by is_dangerous_command)
  └── windows_safe_commands.rs  ← KEEP (declared as module, used by is_safe_command)
```

### Files to modify

1. Delete `core/src/command_safety/is_dangerous_command.rs`
2. Delete `core/src/command_safety/windows_dangerous_commands.rs`
3. `core/src/command_safety/mod.rs` — remove `pub mod is_dangerous_command;` line

### Dependencies potentially affected

`windows_dangerous_commands.rs` uses `once_cell`, `regex`, and `url` crates. However:
- These are only compiled on Windows in test builds
- `once_cell` and `regex` may be used elsewhere in the crate
- Even if they become unused, the compiler won't flag them on Linux builds
- Dependency cleanup is a separate task

### Verification

- All existing tests pass (the removed tests were testing dead HTTP-backend behavior)
- `is_safe_command.rs` and `windows_safe_commands.rs` are completely unaffected
- No production behavior changes

## Remove `rollout/policy.rs` (seventeenth removal)

### Why this component

`rollout/policy.rs` defines three filtering functions (`is_persisted_response_item`, `should_persist_response_item`, `should_persist_event_msg`) that determine which rollout items should be persisted. The entire module is declared with `#[allow(dead_code)]` in `rollout/mod.rs` (line 11-12) and has **zero callers** anywhere in the codebase.

### What it contains

- `is_persisted_response_item(item: &RolloutItem) -> bool` — top-level dispatcher
- `should_persist_response_item(item: &ResponseItem) -> bool` — filters ResponseItem variants
- `should_persist_event_msg(ev: &EventMsg) -> bool` — filters EventMsg variants (~35 variants return false)

All three functions are `pub(crate)` but never imported or called.

### References (non-import)

1. `app-server-protocol/src/protocol/thread_history.rs:47` — doc comment: `/// See should_persist_event_msg in codex-rs/core/rollout/policy.rs.` This is a cross-reference comment, not an import. Needs updating.
2. `protocol/docs.md` (lines 75, 111, 118, 124) — Four event types annotated "Not persisted to rollout policy." These refer to the filtering logic. Needs rewording.

### Files to modify

1. **Delete** `core/src/rollout/policy.rs`
2. **Edit** `core/src/rollout/mod.rs` — remove `#[allow(dead_code)]` and `pub(crate) mod policy;` (lines 11-12)
3. **Edit** `app-server-protocol/src/protocol/thread_history.rs:47` — update doc comment
4. **Edit** `protocol/docs.md` — reword "rollout policy" annotations
5. **Edit** `core/docs.md` — remove any policy.rs references if present

### Verification

- `cargo test -p codex-core` — all unit + integration tests pass
- `cargo build --bin nori` — binary still compiles
- No behavioral changes — these functions were never called

## Remove unused dependencies from codex-core Cargo.toml (eighteenth removal)

### Why this component

After 17 prior removal commits, several dependencies in `core/Cargo.toml` have zero consumers in codex-core source code. These are artifacts of the HTTP backend removal — their consumers were deleted but the dependency declarations remained. They increase build times, confuse readers about what the crate actually uses, and pollute `cargo tree` output.

### Dependencies to remove from `core/Cargo.toml`

| Dependency | Type | Status |
|---|---|---|
| `askama` | production dep (line 17) | Zero `use askama` or `#[template]` in any `.rs` file across entire workspace |
| `async-trait` | production dep (line 19) | Zero usage in `core/src/`. Still used by `async-utils`, `utils/readiness`, `mock-acp-agent` |
| `indexmap` | production dep (line 43) | Zero `use indexmap` in any `.rs` file across entire workspace |
| `strum_macros` | production dep (line 57) | Zero `strum_macros` or `#[strum` usage in `core/src/`. Still used by `protocol`, `app-server-protocol`, `tui`, `otel` |
| `test-case` | production dep (line 63) | Zero `#[test_case]` or `use test_case` in any `.rs` file across entire workspace |
| `test-log` | production dep (line 64) | Zero `#[test_log]` or `use test_log` in any `.rs` file across entire workspace |

### Dependencies to remove from workspace `Cargo.toml`

| Dependency | Reason |
|---|---|
| `askama` | Zero usage anywhere in workspace |
| `indexmap` | Zero usage anywhere in workspace |
| `test-log` | Zero usage anywhere in workspace |

### Dependencies to KEEP in workspace `Cargo.toml`

| Dependency | Reason |
|---|---|
| `async-trait` | Used by `async-utils`, `utils/readiness`, `mock-acp-agent` |
| `strum_macros` | Used by `protocol`, `app-server-protocol`, `tui`, `otel` |
| `test-case` | Not in workspace Cargo.toml (hardcoded version in core only) |

### Verification

- `cargo check -p codex-core` — compiles without errors
- `cargo test -p codex-core` — all tests pass
- `cargo build --bin nori` — binary still compiles
- No behavioral changes — pure dependency cleanup

## Remove dead HTTP retry/timeout methods from ModelProviderInfo (twentieth removal)

### Why this component

`ModelProviderInfo` in `core/src/model_provider_info.rs` has three accessor methods and five constants that existed solely to configure the HTTP streaming client's retry/timeout behavior. The HTTP client (`ModelClient`) was removed in earlier commits, so these methods have zero callers anywhere in the workspace.

### What to remove

**Constants (lines 15-21):**
- `DEFAULT_STREAM_IDLE_TIMEOUT_MS: u64 = 300_000`
- `DEFAULT_STREAM_MAX_RETRIES: u64 = 5`
- `DEFAULT_REQUEST_MAX_RETRIES: u64 = 4`
- `MAX_STREAM_MAX_RETRIES: u64 = 100`
- `MAX_REQUEST_MAX_RETRIES: u64 = 100`

**Methods (lines 100-119):**
- `pub fn request_max_retries(&self) -> u64` — applied default + cap to raw field
- `pub fn stream_max_retries(&self) -> u64` — applied default + cap to raw field
- `pub fn stream_idle_timeout(&self) -> Duration` — converted ms field to `Duration`

**Import (line 12):**
- `use std::time::Duration;` — only used by `stream_idle_timeout()`

### What to keep

**Fields on `ModelProviderInfo` struct (lines 56-63):**
- `request_max_retries: Option<u64>`
- `stream_max_retries: Option<u64>`
- `stream_idle_timeout_ms: Option<u64>`

These fields must remain for config deserialization backwards compatibility. Existing user config TOML files may include these keys. Since `ModelProviderInfo` does NOT use `#[serde(deny_unknown_fields)]`, removing the fields would silently drop the values — but keeping them prevents any surprise. The ACP backend has its own `AcpProviderInfo` struct in `acp/src/registry.rs` with separate retry fields.

### Verification

- Zero external callers for all three methods (confirmed via grep)
- All five constants are only referenced within the methods being removed
- `Duration` import is only used by `stream_idle_timeout()`
- Config deserialization tests pass unchanged (they test field deserialization, not methods)
- ACP backend is unaffected (uses its own `AcpProviderInfo`)

## Delete orphaned test files from core_test_support (twenty-first removal)

### Why this component

Two files exist on disk in `core/tests/common/` but are not declared as modules in `lib.rs`:
- `test_codex.rs` (~360 lines) — HTTP backend test harness using removed types `CodexConversation`, `ConversationManager`
- `responses.rs` (~715 lines) — HTTP Responses API mock infrastructure (SSE builders, request capture)

These files reference each other (`crate::test_codex`, `crate::responses`) but since neither is declared in `lib.rs`, they are never compiled. They are pure dead files left over from the `codex-api` removal.

### Verification

- `lib.rs` has no `mod test_codex` or `mod responses` declaration
- No external crate imports `core_test_support::test_codex` or `core_test_support::responses`
- The files reference removed types (`CodexConversation`, `ConversationManager`) and would not compile if re-included
- Deleting them has zero effect on compilation or tests
