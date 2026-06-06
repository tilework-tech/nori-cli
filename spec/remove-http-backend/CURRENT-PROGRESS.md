# Current Progress

## Status: Twenty-fourth component removed

### Completed: Remove `chatgpt_base_url`, dead error variants, and stale `wire_api` references

Three surgical removals of HTTP backend remnants:

**1. Removed `chatgpt_base_url` field**

This config field was set during config loading but never read at runtime — it was part of the HTTP backend's ChatGPT API URL configuration.

- `core/src/config/mod.rs` — Removed from `Config` struct (field), `ConfigToml` struct (field), and resolution logic (3 lines)
- `core/src/config/profile.rs` — Removed from `ConfigProfile` struct (field) and `From<ConfigProfile> for Profile` impl (1 line)
- `app-server-protocol/src/protocol/v1.rs` — Removed from ASP `Profile` struct (field)
- `core/src/config/tests/part3.rs`, `part4.rs` — Removed from expected `Config` struct literals (4 sites)

Backwards compat: existing config.toml files with `chatgpt_base_url` are silently ignored by serde (no `deny_unknown_fields`).

**2. Removed dead error variants `ResponseStreamConnectionFailed` and `ResponseStreamDisconnected`**

These `CodexErrorInfo` variants were never constructed or matched — HTTP streaming error types with zero consumers.

- `protocol/src/protocol/mod.rs` — Removed both variants (8 lines)
- `app-server-protocol/src/protocol/v2.rs` — Removed both ASP variants and two `From` conversion arms (18 lines)

**3. Cleaned up stale `wire_api` references**

Three leftover references after the `WireApi` enum was removed in a prior commit.

- `core/src/config/tests/mod.rs` — Removed `wire_api = "chat"` from test TOML fixture (silently ignored)
- `tui-pty-e2e/tests/live_acp.rs` — Removed `wire_api = "acp"` from config generator (silently ignored)
- `tui-pty-e2e/tests/acp_mode.rs` — Updated doc comment: `wire_api = "acp"` → `with a mock agent`

**Documentation updated:**
- `protocol/docs.md` — Removed stale reference to "legacy Codex backend path"
- `nori-protocol/docs.md` — Updated `parsed_cmd` description to remove Codex backend reference

**Impact:** ~50 lines of dead config/error/reference code removed. codex-core unit tests: 371 pass. Integration tests: 14 pass. codex-protocol: 20 pass. ASP: 18 pass. nori binary builds successfully.

### Suggested next steps for future commits
1. Investigate whether `ModelProviderInfo` fields `base_url`, `experimental_bearer_token`, `query_params`, `http_headers`, `env_http_headers` are functionally dead — they were consumed by the HTTP client but may still be read by auth or config validation
2. Remove `forced_chatgpt_workspace_id` and `forced_login_method` if dead — investigate whether these are still consumed by auth flow
3. Remove remaining HTTP-specific `CodexErrorInfo` variants (`ContextWindowExceeded`, `UsageLimitExceeded`, `HttpConnectionFailed`, `ResponseTooManyFailedAttempts`) if confirmed dead

### Completed: Delete orphaned test files from core_test_support

### Completed: Remove dead HTTP retry/timeout methods from ModelProviderInfo

Removed 3 dead accessor methods and 5 dead constants from `model_provider_info.rs` that configured the now-deleted HTTP streaming client's retry/timeout behavior. These methods had zero callers anywhere in the workspace after the HTTP backend removal.

**What was removed:**
- `request_max_retries()` method — applied default (4) and cap (100) to raw field
- `stream_max_retries()` method — applied default (5) and cap (100) to raw field
- `stream_idle_timeout()` method — converted ms field to `Duration` with default (300s)
- `DEFAULT_STREAM_IDLE_TIMEOUT_MS` constant (300,000)
- `DEFAULT_STREAM_MAX_RETRIES` constant (5)
- `DEFAULT_REQUEST_MAX_RETRIES` constant (4)
- `MAX_STREAM_MAX_RETRIES` constant (100)
- `MAX_REQUEST_MAX_RETRIES` constant (100)
- `use std::time::Duration` import

**What was preserved:**
- `ModelProviderInfo` struct fields (`request_max_retries`, `stream_max_retries`, `stream_idle_timeout_ms`) — kept for config deserialization backwards compatibility
- `api_key()` method — still used by auth flow
- All built-in provider definitions
- `create_oss_provider()` and `create_oss_provider_with_base_url()` functions

**Documentation updated:**
- `core/docs.md` — removed "retry/timeout settings" from `model_provider_info.rs` description

**Impact:** ~25 lines of dead HTTP retry/timeout code removed. codex-core unit tests: 360 pass. Integration tests: 14 pass. nori binary builds successfully.

### Suggested next steps for future commits
1. Investigate whether `ModelProviderInfo` fields `base_url`, `experimental_bearer_token`, `query_params`, `http_headers`, `env_http_headers` are functionally dead — they were consumed by the HTTP client but may still be read by auth or config validation
2. Continue identifying and removing other HTTP-backend remnants in codex-core

### Completed: Remove dead test-only code from compact.rs and safety.rs

Removed dead test-only functions from `compact.rs` and `safety.rs` that tested HTTP-backend behavior deleted in earlier commits, plus the unused `content_items_to_text` function.

**What was removed from `compact.rs`:**
- `content_items_to_text` — `pub fn` re-exported from `lib.rs` but with zero external callers
- `collect_user_messages` — test-only helper for extracting user messages from ResponseItems
- `is_summary_message` — test-only helper for checking summary prefix
- `build_compacted_history` / `build_compacted_history_with_limit` — test-only helpers for building compacted histories with token budgets
- `COMPACT_USER_MESSAGE_MAX_TOKENS` — test-only constant
- 6 tests exercising the above functions

**What was removed from `safety.rs`:**
- `is_write_patch_constrained_to_writable_paths` — test-only function checking whether file patches were constrained to writable paths under sandbox policy
- `test_writable_roots_constraint` — test for above function

**What was removed from `lib.rs`:**
- `pub use compact::content_items_to_text;` re-export

**What was preserved:**
- `SUMMARIZATION_PROMPT` and `SUMMARY_PREFIX` constants in `compact.rs` (used by ACP backend)
- `get_platform_sandbox` and `set_windows_sandbox_enabled` in `safety.rs` (used by core, tui, tests)

**Documentation updated:**
- `core/docs.md` — updated `compact.rs` description to reflect only constants remain

**Impact:** ~390 lines of dead test infrastructure removed. codex-core unit tests: 360 pass (down from 367 — the 7 removed tests were dead HTTP-backend tests). Integration tests: 14 pass. nori binary builds successfully.

### Suggested next steps for future commits
1. Remove remaining dead code: investigate `event_mapping::parse_turn_item` — now that its only test consumer (`collect_user_messages` in compact.rs) is gone, verify whether it still has production callers
2. Continue identifying and removing other HTTP-backend remnants in codex-core

### Completed: Remove unused dependencies from codex-core Cargo.toml

Removed 6 unused dependencies from `core/Cargo.toml` and 3 unused workspace-level declarations from `codex-rs/Cargo.toml`. These dependencies became dead after the HTTP backend removal — their consumers were deleted but the dependency declarations remained.

**What was removed from `core/Cargo.toml` `[dependencies]`:**
- `askama` — HTML template engine (zero usage anywhere in workspace)
- `async-trait` — async trait support (zero usage in core; still used by async-utils, utils/readiness, mock-acp-agent)
- `indexmap` — ordered hash map (zero usage anywhere in workspace)
- `strum_macros` — enum derive macros (zero usage in core; still used by protocol, app-server-protocol, tui, otel)
- `test-case` — parameterized test macro (zero usage anywhere; was incorrectly listed as production dep)
- `test-log` — test logging setup (zero usage anywhere; was incorrectly listed as production dep)

**What was removed from workspace `Cargo.toml` `[workspace.dependencies]`:**
- `askama` — zero usage anywhere
- `indexmap` — zero usage anywhere
- `test-log` — zero usage anywhere

**What was preserved in workspace `Cargo.toml`:**
- `async-trait` — still used by `async-utils`, `utils/readiness`, `mock-acp-agent`
- `strum_macros` — still used by `protocol`, `app-server-protocol`, `tui`, `otel`

**Impact:** 6 fewer dependencies for codex-core, 3 fewer workspace declarations. Reduces build times and eliminates confusion. codex-core unit tests: 367 pass. Integration tests: 14 pass. nori binary builds successfully.

### Suggested next steps for future commits
1. Clean up test-only code in `compact.rs` — `build_compacted_history`, `collect_user_messages`, `is_summary_message` are test-only functions testing dead HTTP-backend behavior; also `content_items_to_text` is exported but has zero external consumers
2. Clean up test-only code in `safety.rs` — `is_write_patch_constrained_to_writable_paths` is test-only, testing dead HTTP-backend patch validation logic

### Completed: Remove dead rollout persistence policy from rollout/

Removed `rollout/policy.rs` from codex-core. This module defined three filtering functions (`is_persisted_response_item`, `should_persist_response_item`, `should_persist_event_msg`) that determined which rollout items should be persisted to disk. The entire module was declared with `#[allow(dead_code)]` and had zero callers anywhere in the codebase.

**What was removed:**
- `rollout/policy.rs` (~93 lines) — three `pub(crate)` functions for filtering rollout items
- `#[allow(dead_code)]` and `pub(crate) mod policy;` from `rollout/mod.rs`

**What was preserved:**
- `rollout/list.rs` — rollout file discovery (used by ACP/TUI)
- `rollout/recorder.rs` — rollout recording (used by ACP/TUI)
- `rollout/tests.rs` — existing rollout tests

**Documentation updated:**
- `app-server-protocol/src/protocol/thread_history.rs` — removed stale doc comment referencing deleted `policy.rs`
- `protocol/docs.md` — changed "Not persisted to rollout policy" to "Not persisted to rollout files" in 4 places

**Impact:** ~93 lines of dead code removed. codex-core unit tests: 367 pass. Integration tests: 14 pass.

### Suggested next steps for future commits
1. Clean up test-only code in `compact.rs` — `build_compacted_history`, `collect_user_messages`, `is_summary_message` are test-only functions testing dead HTTP-backend behavior
2. Clean up test-only code in `safety.rs` — `is_write_patch_constrained_to_writable_paths` is test-only
3. Clean up `Cargo.toml` — remove unused dependencies: `askama`, `async-trait`, `indexmap`, `strum_macros`, `test-case`, `test-log` (all confirmed zero references in source code)

### Completed: Remove dead command danger assessment from command_safety/

Removed `is_dangerous_command.rs` and `windows_dangerous_commands.rs` from the `command_safety/` module. These files implemented the HTTP backend's "is this command dangerous?" assessment pipeline — the counterpart to `is_safe_command.rs`. All code in `is_dangerous_command.rs` was wrapped in `#[cfg(test)]` with zero production callers. `windows_dangerous_commands.rs` was only imported by `is_dangerous_command.rs` on Windows.

**What was removed:**
- `is_dangerous_command.rs` (~155 lines) — test-only functions: `requires_initial_appoval`, `command_might_be_dangerous`, `is_dangerous_to_call_with_exec`, plus 9 test cases
- `windows_dangerous_commands.rs` (~316 lines) — `is_dangerous_command_windows`, PowerShell/cmd.exe/GUI danger detection, URL pattern matching
- `pub mod is_dangerous_command;` declaration from `command_safety/mod.rs`

**What was preserved:**
- `is_safe_command.rs` — production code for auto-approving safe commands (used by ACP)
- `windows_safe_commands.rs` — Windows PowerShell safety checks (used by `is_safe_command`)

**Documentation updated:**
- `core/docs.md` — refined `command_safety/` description to reflect that only safe-command auto-approval logic remains

**Impact:** ~471 lines of dead code removed. codex-core unit tests: 367 (down from 376 — the 9 removed tests were dead HTTP-backend tests). Integration tests: 14 pass. E2E: 9 pass.

### Suggested next steps for future commits
1. Remove `rollout/policy.rs` — dead code with `#[allow(dead_code)]`, zero callers anywhere in the codebase
2. Clean up test-only code in `compact.rs` — `build_compacted_history`, `collect_user_messages`, `is_summary_message` are test-only functions testing dead HTTP-backend behavior
3. Clean up test-only code in `safety.rs` — `is_write_patch_constrained_to_writable_paths` is test-only
4. Clean up `Cargo.toml` — remove unused dependencies: `askama`, `async-trait`, `indexmap`, `strum_macros`, `test-case`, `test-log` (all confirmed zero references in source code)

### Completed: Remove orphaned Feature enum variants from features.rs

Removed 4 dead `Feature` enum variants whose consumers were deleted with the HTTP backend. These variants existed in the `FEATURES` registry array but no code ever called `.enabled(Feature::X)` for any of them.

**What was removed:**
- `Feature::GhostCommit` (key: `"undo"`, default: true) — ghost commit functionality operates unconditionally via `codex_git`; this flag was never checked
- `Feature::ExecPolicy` (key: `"exec_policy"`, default: true) — exec policy enforcement runs unconditionally via `codex-execpolicy` crate
- `Feature::ParallelToolCalls` (key: `"parallel"`, default: false) — parallel tool calls were an HTTP-backend concept with no ACP equivalent
- `Feature::ShellTool` (key: `"shell_tool"`, default: true) — shell tool types (`ConfigShellToolType`) exist independently; never gated by this flag

**What was preserved:**
- 7 remaining `Feature` variants: `UnifiedExec`, `RmcpClient`, `ApplyPatchFreeform`, `ViewImageTool`, `WebSearchRequest`, `SandboxCommandAssessment`, `WindowsSandbox` — all have active consumers via `.enabled()` calls
- All legacy aliases in `legacy.rs` (none referenced the removed variants)
- All existing feature-related tests pass unchanged

**Impact:** ~30 lines of dead feature flag definitions removed. The `Feature` enum is now 7 variants instead of 11. Existing config files with `undo = false` or `shell_tool = true` in `[features]` will now see "unknown feature key" warnings — correct, since these keys had no effect. All existing tests pass (codex-core: 376 unit + 14 integration; E2E: 9 tests).

### Suggested next steps for future commits
1. Remove or decide on `is_dangerous_command.rs` — entire file is now `#[cfg(test)]`, functions are dead production logic
2. Clean up test-only code in `compact.rs` and `safety.rs` — functions like `build_compacted_history`, `is_write_patch_constrained_to_writable_paths` moved to `#[cfg(test)]` but test dead behavior
3. Remove `rollout/policy.rs` if unused — currently `pub(crate)` functions, need to verify callers
4. Clean up `Cargo.toml` — remove any workspace-level dependencies that are no longer used

### Completed: Remove dead test-only infrastructure from codex-core

Removed test-only modules that survived the HTTP backend removal as `#[cfg(test)]` code. These defined OpenAI Responses API types and tested MCP-to-OpenAI tool conversion — functionality that no longer exists in production.

**What was removed:**
- `client_common.rs` — test-only `ToolSpec`, `FreeformTool`, `ResponsesApiTool` type definitions
- `tools/spec/mod.rs` — test-only `JsonSchema`, `AdditionalProperties`, `mcp_tool_to_openai_tool()`, `sanitize_json_schema()`, `create_shell_tool()`, `create_shell_command_tool()`
- `tools/spec/tests.rs` — 8 tests for dead MCP-to-OpenAI conversion
- `tools/runtimes/` — orphaned directory (never declared in `tools/mod.rs`): `apply_patch.rs`, `shell.rs`, `unified_exec.rs`, `mod.rs`
- `tools/mod.rs` — only declared `pub mod spec;`
- `rollout/error.rs` — empty 2-line placeholder module

**What was preserved (moved):**
- `model_family_apply_patch_instructions` test — moved from `client_common.rs` to `model_family.rs`, simplified to remove `ToolSpec` dependency. Tests that `find_family_for_model()` returns correct `needs_special_apply_patch_instructions` values.

**Documentation updated:**
- `core/docs.md` — removed `client_common.rs` description, `tools/spec/` module structure reference, stale `exec_policy.rs` reference, stale `ApprovalRequirement`/`SandboxablePreference` references
- `execpolicy/docs.md` — updated integration reference from `exec_policy.rs` to `command_safety/`

**Impact:** ~1,270 lines of dead test infrastructure removed. All existing tests pass (codex-core: 376 unit + 14 integration; 2 ignored live CLI tests).

### Suggested next steps for future commits
1. Remove orphaned `Feature` enum variants (`GhostCommit`, `ExecPolicy`, `ParallelToolCalls`, `ShellTool`) from `features.rs` — their consumers were deleted with the HTTP backend
2. Remove or decide on `is_dangerous_command.rs` — entire file is now `#[cfg(test)]`, functions are dead production logic
3. Clean up test-only code in `compact.rs` and `safety.rs` — functions like `build_compacted_history`, `is_write_patch_constrained_to_writable_paths` moved to `#[cfg(test)]` but test dead behavior
4. Remove `rollout/policy.rs` or integrate it — currently `#[allow(dead_code)]`
5. Clean up `Cargo.toml` — remove any workspace-level dependencies that are no longer used

### Completed: Remove `codex-api` crate and all `legacy-http-backend` gated code

Removed the `codex-api` crate from the workspace entirely, along with the `legacy-http-backend` feature flag and all code gated behind it in codex-core. This is the culmination of 12 previous incremental commits that isolated the HTTP backend behind feature gates.

**What was removed:**
- `codex-api/` crate (27 files, ~2,658 lines) — the HTTP API client layer
- `legacy-http-backend` feature flag from codex-core
- 16 gated source modules from codex-core (~16,245 lines): `api_bridge`, `apply_patch`, `client`, `codex/`, `codex_conversation`, `context_manager/`, `conversation_manager`, `environment_context`, `function_tool`, `mcp_connection_manager`, `mcp_tool_call`, `message_history`, `response_processing`, `state/`, `tasks/`, `unified_exec/`, `user_shell_command`, `sandboxing/assessment`
- 8 gated tool submodules (~5,587 lines): `tools/context`, `tools/events`, `tools/handlers/`, `tools/orchestrator`, `tools/parallel`, `tools/registry`, `tools/router`, `tools/runtimes`, `tools/sandboxing`
- 17 HTTP-specific `CodexErr` variants and 5 helper structs from `error.rs`
- HTTP-specific compact functions from `compact.rs`
- `build_specs()` function from `tools/spec/mod.rs`
- 32 integration test files + `responses_headers.rs` standalone test
- `core_test_support` modules: `test_codex.rs`, `responses.rs`
- `eventsource-stream` dependency from codex-core

**What was preserved:**
- All shared code: config, auth, compact utilities (SUMMARIZATION_PROMPT, SUMMARY_PREFIX, content_items_to_text), protocol re-exports, event_mapping, sandboxing (minus assessment), tools/spec, default_client
- 6 integration tests: auth_refresh, exec, live_cli, rollout_list_find, seatbelt, text_encoding_fix
- `core_test_support` crate (simplified — config helpers, macros, fs_wait)
- `reqwest` dependency (used by auth and default_client)
- MCP tool spec tests rewritten to call `mcp_tool_to_openai_tool` directly

**Impact:** ~45,000 lines of HTTP backend code removed. The `codex-api` crate no longer exists in the workspace. `CodexErr` has only shared variants. All existing tests pass (codex-core: 392 unit + 14 integration; E2E: 6 tests). Many dead-code warnings remain for modules that lost their consumers (client_common, command_safety, exec_policy, tools/spec, etc.) — these should be cleaned up in future commits.

### Suggested next steps for future commits
1. Remove newly-dead code: `client_common.rs` (Prompt, ToolSpec types), `command_safety`, `exec_policy`, unused functions in `tools/spec`, `tools/mod.rs`, `util.rs`, `rollout/error.rs`, `safety.rs`
2. Clean up `Cargo.toml` — remove any workspace-level dependencies that are no longer used (e.g., `eventsource-stream` if no other crate uses it)

### Completed: Gate HTTP-specific error types behind `legacy-http-backend`

Gated the HTTP-specific error types, variants, methods, and tests in `codex-core/src/error.rs` behind the `legacy-http-backend` feature flag. This removes HTTP concepts from the public error type for non-HTTP builds.

**What was gated:**
- ~15 `CodexErr` enum variants: `Stream`, `ContextWindowExceeded`, `ConversationNotFound`, `SessionConfiguredNotFirstEvent`, `Timeout`, `Spawn`, `Interrupted`, `UnexpectedStatus`, `UsageLimitReached`, `ResponseStreamFailed`, `ConnectionFailed`, `QuotaExceeded`, `UsageNotIncluded`, `InternalServerError`, `RetryLimit`, `InternalAgentDied`
- 5 helper structs and their impls: `UnexpectedResponseError`, `ConnectionFailedError`, `ResponseStreamFailed`, `RetryLimitReachedError`, `UsageLimitReachedError`
- Methods on `CodexErr`: `to_codex_protocol_error()`, `to_error_event()`, `http_status_code_value()`
- Helper functions: `retry_suffix`, `retry_suffix_after_or`, `format_retry_timestamp`, `day_suffix`, `now_for_retry`, `CLOUDFLARE_BLOCKED_MESSAGE`
- Imports: `reqwest::StatusCode`, `codex_protocol::ConversationId`, `chrono::*`, `codex_protocol::protocol::{CodexErrorInfo, ErrorEvent, RateLimitSnapshot}`, `std::time::Duration`
- 14 HTTP-specific tests moved to a separate `http_tests` module with `#[cfg(all(test, feature = "legacy-http-backend"))]`

**What was preserved (always available):**
- Shared error types: `SandboxErr`, `EnvVarError`, `RefreshTokenFailedError`, `RefreshTokenFailedReason`
- Shared `CodexErr` variants: `Sandbox`, `LandlockSandboxExecutableNotProvided`, `UnsupportedOperation`, `Fatal`, `Io`, `Json`, `TokioJoin`, `EnvVar`, `RefreshTokenFailed`, `TurnAborted` (already gated)
- Shared methods: `downcast_ref()`, `get_error_message_ui()`
- 4 sandbox-related tests in un-gated `tests` module

**Impact:** When `legacy-http-backend` is off (nori/tui/cli/acp binaries), `CodexErr` has only ~10 shared variants instead of ~25. All existing tests pass (codex-core: 537 unit, 441 integration pass, 21 pre-existing nvm environment failures; E2E: 6 tests).

### Suggested next steps for future commits
1. Gate the `codex/` module and its cascade (Session/TurnContext permeate tools/, tasks/, state/, etc. — requires separating shared infrastructure from HTTP-specific orchestration)
2. Eventually: remove the `codex-api` crate entirely

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

### Completed: Move `to_api_provider()` out of shared config module

Moved the `to_api_provider()` method and `build_header_map()` helper from `model_provider_info.rs` to `client.rs` as standalone functions (`create_api_provider()` and `build_header_map()`). This removes the `codex-api` dependency from the shared configuration module.

**What was moved:**
- `to_api_provider()` → `create_api_provider()` standalone function in `client.rs`
- `build_header_map()` → private helper in `client.rs`
- 3 HTTP-backend tests (`legacy_wire_api_field_in_config_is_silently_ignored`, `ollama_builtin_provider_creates_successfully`, `detects_azure_responses_base_urls`) → `client.rs` test module

**What was removed from `model_provider_info.rs`:**
- `use codex_api::Provider as ApiProvider;`
- `use codex_api::provider::RetryConfig as ApiRetryConfig;`
- `build_header_map()` method
- `to_api_provider()` method
- 3 HTTP-backend tests

**What was simplified:**
- `model_provider_info.rs` is now a pure shared configuration module — no `codex-api` dependency
- HTTP-backend conversion logic is concentrated in `client.rs` where `ModelClient` lives

**Impact:** `model_provider_info.rs` no longer imports from `codex-api`. Reduces the codex-api dependency surface in shared modules. All existing tests pass (codex-core: 537 unit, 441 integration pass, 21 pre-existing nvm environment failures).

### Completed: Gate `sandboxing/assessment` behind `legacy-http-backend`

Gated the sandbox assessment module behind the `legacy-http-backend` feature flag. This module creates a `ModelClient` and makes direct HTTP API calls to evaluate command safety — pure HTTP-backend code that nori never uses.

**What was gated:**
- `sandboxing/assessment.rs` module — `#[cfg(feature = "legacy-http-backend")]` on `pub mod assessment;` in `sandboxing/mod.rs`
- `assess_sandbox_command()` method on `Session` in `codex/approval.rs` — split into feature-gated real implementation and `#[cfg(not(...))]` stub returning `None`

**What was preserved:**
- `SandboxCommandAssessment` type in codex-protocol (shared protocol type)
- `experimental_sandbox_command_assessment` config field (shared config)
- Call sites in `tools/orchestrator.rs` unchanged — stub method has same signature
- Stub behavior identical to `experimental_sandbox_command_assessment = false` (the default)

**Impact:** When `legacy-http-backend` is off (nori/tui/cli/acp binaries), `sandboxing/assessment.rs` and its `ModelClient`/`Prompt`/`ResponseEvent` imports are excluded from compilation. All existing tests pass (codex-core: 439 pass, 23 pre-existing nvm environment failures; E2E: 6 tests).

### Completed: Gate HTTP-specific compact functions behind `legacy-http-backend`

Gated the HTTP-backend-specific compaction functions in `compact.rs` behind the `legacy-http-backend` feature flag. These functions make direct model calls via `ModelClient.stream()` and process `ResponseEvent`s — pure HTTP-backend code that nori never uses.

**What was gated:**
- `run_inline_auto_compact_task()` — auto-compaction during HTTP turn execution
- `run_compact_task()` — manual compaction task
- `run_compact_task_inner()` — shared implementation
- `drain_to_completed()` — streams model response to completion
- 16 HTTP-specific imports (Prompt, ResponseEvent, protocol event types, etc.)

**What was preserved (shared, always available):**
- `SUMMARIZATION_PROMPT`, `SUMMARY_PREFIX` constants (used by ACP)
- `content_items_to_text()`, `collect_user_messages()`, `is_summary_message()` utilities
- `build_compacted_history()` / `build_compacted_history_with_limit()` history construction
- All 6 unit tests (test shared functions)

**Stub approach:**
- `#[cfg(not(feature = "legacy-http-backend"))]` no-op stubs for `run_inline_auto_compact_task` and `run_compact_task`
- Callers in `codex/mod.rs`, `codex/turn_execution.rs`, `tasks/compact.rs` unchanged
- Same pattern as `sandboxing/assessment.rs` gating

**Known limitation:** The no-op stubs are safe because the `codex/` turn execution path is unreachable from the nori binary (ACP). However, if the `codex/` module is ever called without the feature, `run_inline_auto_compact_task` no-op + `continue` in `turn_execution.rs:91` would infinite-loop. This is acceptable because the entire `codex/` module should be feature-gated in a future commit (Phase 6), eliminating the dead code path entirely.

**Impact:** 4 HTTP-backend functions and 16 imports excluded from non-feature-flagged builds. All existing tests pass (codex-core: 541 unit, 12 compact integration; E2E: 6 tests). Build without feature succeeds with 1 fewer warning than baseline.

### Completed: Move ResponseEvent/ResponseStream from `client_common.rs` to `client.rs`

Moved the `ResponseEvent` re-export (from `codex-api`) and `ResponseStream` type from `client_common.rs` into `client.rs`. This removes the sole production `codex-api` import from `client_common.rs`, making it a pure shared-code module.

**What was moved:**
- `pub use codex_api::common::ResponseEvent;` — re-export, from client_common.rs to client.rs
- `ResponseStream` struct and its `futures::Stream` impl — from client_common.rs to client.rs

**What was updated (import paths):**
- `codex/mod.rs` — `crate::client_common::ResponseEvent` → `crate::client::ResponseEvent`
- `compact.rs` — gated import path updated from `client_common` to `client`
- `sandboxing/assessment.rs` — import path updated from `client_common` to `client`
- `lib.rs` — gated re-exports updated from `client_common` to `client`

**What was removed from `client_common.rs`:**
- `pub use codex_api::common::ResponseEvent;` (line 4)
- `ResponseStream` struct and `Stream` impl
- Unused imports: `futures::Stream`, `std::pin::Pin`, `std::task::{Context, Poll}`, `tokio::sync::mpsc`, `crate::error::Result`

**After this change:**
- `client_common.rs` has zero production `codex-api` imports — it's a pure shared module with `Prompt` and `tools` submodule
- Only `client.rs` and `api_bridge.rs` have production `codex-api` imports in codex-core
- Direct prerequisite for making `codex-api` an optional dependency behind `legacy-http-backend`

**Impact:** Pure import reorganization, no behavioral changes. All existing tests pass (codex-core: 537 unit, 436 integration pass, 26 pre-existing nvm environment failures; codex-api: 2 tests; E2E: 5 ACP + 3 streaming pass).

### Completed: Make `codex-api` optional and fix workspace compilation

Made `codex-api` an optional dependency in codex-core behind the `legacy-http-backend` feature flag. Gated the majority of HTTP-backend modules in `lib.rs` behind the feature. Fixed two workspace compilation errors that resulted from the gating.

**What was made optional:**
- `codex-api` dependency: `codex-api = { workspace = true, optional = true }` with `legacy-http-backend = ["dep:codex-api"]`
- 18+ modules gated in `lib.rs`: `api_bridge`, `apply_patch`, `client`, `codex`, `codex_conversation`, `conversation_manager`, `context_manager`, `environment_context`, `mcp_connection_manager`, `mcp_tool_call`, `message_history`, `response_processing`, `unified_exec`, `function_tool`, `state`, `tasks`, `user_shell_command`

**What was fixed:**
- `CODEX_APPLY_PATCH_ARG1` constant moved from gated `apply_patch.rs` to shared `tool_types.rs` — `codex-arg0` imports it without enabling the feature
- `core_test_support` Cargo.toml updated to enable `legacy-http-backend` — it imports `CodexConversation` and `ConversationManager`
- `build_specs()` in `tools/spec/mod.rs` gated behind `legacy-http-backend` (imports from gated modules)
- Compact stubs removed (all callers are in gated modules, stubs were unreachable)

**Impact:** `codex-api` and its transitive dependencies (`codex-client`, `eventsource-stream`, etc.) are excluded from the nori binary's dependency tree. `cargo check --workspace` succeeds with zero errors. All existing tests pass (codex-core: 537 unit, 227+ apply_patch integration pass; E2E: pass individually).

### Completed: Remove `codex-client` crate by inlining into `codex-api`

Removed the `codex-client` crate from the workspace entirely. Its source code (7 files, 331 lines) was inlined into `codex-api` as a `pub(crate) mod client;` submodule. This eliminates one confusing HTTP-backend crate from the workspace.

**What was removed:**
- `codex-client/` crate directory (Cargo.toml, src/, docs.md, README.md)
- `codex-client` from workspace members and workspace dependencies
- `sse_stream` function (unused by codex-api — it has its own SSE parsing)
- `StreamError` enum (only used by the removed `sse_stream`)

**What was inlined:**
- `codex-api/src/client/` submodule containing: `mod.rs` (re-exports), `error.rs` (TransportError), `request.rs` (Request/Response), `retry.rs` (RetryPolicy, backoff, run_with_retry), `telemetry.rs` (RequestTelemetry trait), `transport.rs` (HttpTransport, ReqwestTransport, ByteStream, StreamResponse)

**What was updated:**
- All `codex_client::` imports in codex-api source → `crate::client::`
- All `codex_client::` imports in codex-api tests → `codex_api::` (via new re-exports)
- `codex-api/Cargo.toml`: removed `codex-client` dep, added `reqwest` and `rand` (previously transitive through codex-client)
- New public re-exports from codex-api: `HttpTransport`, `Request`, `Response`, `StreamResponse` (previously accessed via `codex_client::` in tests)
- Documentation: `codex-api/docs.md`, `codex-api/README.md`, `core/docs.md` updated to remove codex-client references

**Impact:** One fewer crate in the workspace. codex-api is now self-contained. Net -346 lines. All existing tests pass (codex-api: 5 tests, codex-core: 537 unit + 441 integration pass, 21 pre-existing nvm environment failures; E2E: 9 tests).

### Suggested next steps for future commits
1. Gate the `codex/` module and its cascade (Session/TurnContext permeate tools/, tasks/, state/, etc. — requires separating shared infrastructure from HTTP-specific orchestration)
2. Eventually: remove the `codex-api` crate entirely
