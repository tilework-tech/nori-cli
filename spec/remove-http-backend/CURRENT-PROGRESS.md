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

### Suggested next steps for future commits
1. Remove the `ModelClient::compact_conversation_history` method in `client.rs` (the HTTP compact endpoint caller)
2. Remove the `codex_conversation.rs` and `conversation_manager.rs` wrappers (only used by HTTP backend tests - would need to inline into test helpers)
3. Eventually: feature-gate or remove the `codex-api` and `codex-client` crates entirely
