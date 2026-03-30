# Current Progress

## Status: First component removed

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

### Suggested next steps for future commits
1. Remove the `codex_conversation.rs` and `conversation_manager.rs` wrappers (only used by HTTP backend tests - would need to inline into test helpers)
2. Feature-gate `codex-api` dependency in codex-core behind a `legacy-http-backend` cargo feature
3. Eventually: remove the `codex-api` and `codex-client` crates entirely
