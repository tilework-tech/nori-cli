# Agent Session ID Discovery and Token Usage Implementation Plan

**Goal:** Enable the `/status` command to display token usage by discovering the current ACP session ID, locating the corresponding session transcript, and parsing token usage data.

**Architecture:** The ACP protocol provides session IDs at session creation. We will add a session discovery module to locate transcript files based on agent type and session ID, then integrate parsed token usage into the Nori session header displayed by `/status`.

**Tech Stack:** Rust, codex-acp, codex-tui, session_parser module, tokio async

---

## Testing Plan

I will add unit tests that ensure:
1. Session transcript path building works correctly for each agent type (Claude, Codex, Gemini)
2. The session discovery module can find transcripts given session ID and agent kind
3. Token usage integration in session_header displays correctly

I will add integration tests that ensure:
1. End-to-end flow: session ID from ACP → transcript discovery → token usage parsing → display
2. Edge cases: missing transcripts, malformed session IDs, no token data available

NOTE: I will write *all* tests before I add any implementation behavior.

---

## Phase 1: Session Discovery Module

### Step 1.1: Create session_discovery.rs module skeleton
- **File**: `/home/user/nori-cli/codex-rs/acp/src/session_discovery.rs`
- **Actions**:
  1. Create the file with module documentation
  2. Define `discover_transcript_path(agent_kind: AgentKind, session_id: &str, cwd: &Path) -> Option<PathBuf>`
  3. Export from `/home/user/nori-cli/codex-rs/acp/src/lib.rs`

### Step 1.2: Write failing tests for Claude transcript discovery
- **File**: `/home/user/nori-cli/codex-rs/acp/tests/session_discovery_test.rs`
- **Test cases**:
  1. `test_claude_transcript_path_format` - verify path format `~/.claude/projects/<PROJECT_PATH>/<SESSIONID>.jsonl`
  2. `test_claude_transcript_discovery_with_valid_session` - given session ID, find transcript
  3. `test_claude_transcript_discovery_missing_file` - return None when file not found

### Step 1.3: Write failing tests for Codex transcript discovery
- **Test cases**:
  1. `test_codex_transcript_path_discovery` - search by session GUID in filename
  2. `test_codex_transcript_discovery_by_date` - find in date-organized directory

### Step 1.4: Write failing tests for Gemini transcript discovery
- **Test cases**:
  1. `test_gemini_transcript_path_format` - verify path format with hashed paths
  2. `test_gemini_transcript_discovery_with_session_id`

### Step 1.5: Run tests, verify they fail
```bash
cargo test -p codex-acp session_discovery
```

### Step 1.6: Implement Claude transcript discovery
- **File**: `/home/user/nori-cli/codex-rs/acp/src/session_discovery.rs`
- **Logic**:
  1. Expand `~/.claude/projects/` base path
  2. Build relative project path from `cwd` (hash or relativize)
  3. Check for `<SESSION_ID>.jsonl` file existence
  4. Return path if found

### Step 1.7: Implement Codex transcript discovery
- **Logic**:
  1. Expand `~/.codex/sessions/` base path
  2. Search recursively for files matching `*-<SESSION_GUID>.jsonl`
  3. Return first match or None

### Step 1.8: Implement Gemini transcript discovery
- **Logic**:
  1. Expand `~/.gemini/tmp/` base path
  2. Hash the cwd to get the hashed path component
  3. Search for `session-*-<SESSION_ID>.json`
  4. Return path if found

### Step 1.9: Run tests, verify they pass
```bash
cargo test -p codex-acp session_discovery
```

### Step 1.10: Commit Phase 1
```bash
git add -A && git commit -m "feat(acp): Add session transcript discovery module"
```

---

## Phase 2: Expose Session ID from ACP Backend

### Step 2.1: Write failing test for session_id accessor
- **File**: `/home/user/nori-cli/codex-rs/acp/tests/backend_test.rs` (or add to existing)
- **Test**: Verify `AcpBackend::session_id()` returns the session ID string

### Step 2.2: Run test, verify it fails

### Step 2.3: Add session_id() method to AcpBackend
- **File**: `/home/user/nori-cli/codex-rs/acp/src/backend.rs`
- **Current**: `AcpBackend` already has `session_id: Arc<acp::SessionId>` (backend.rs:102-111)
- **Add**: Public accessor `pub fn session_id(&self) -> &str`

### Step 2.4: Run test, verify it passes

### Step 2.5: Add agent_kind() method to AcpBackend
- **File**: `/home/user/nori-cli/codex-rs/acp/src/backend.rs`
- **Add**: Store and expose `AgentKind` from the `AcpAgentConfig`

### Step 2.6: Commit Phase 2
```bash
git add -A && git commit -m "feat(acp): Expose session_id and agent_kind from AcpBackend"
```

---

## Phase 3: Extend TUI to Track ACP Session Info

### Step 3.1: Write failing test for session info in ChatWidget
- **File**: `/home/user/nori-cli/codex-rs/tui/src/chatwidget/tests.rs`
- **Test**: Verify ChatWidget can access session ID when in ACP mode

### Step 3.2: Run test, verify it fails

### Step 3.3: Add session info channel/handle to ACP agent spawning
- **File**: `/home/user/nori-cli/codex-rs/tui/src/chatwidget/agent.rs`
- **Modify**: `SpawnAgentResult` to include optional `AcpSessionInfo { session_id: String, agent_kind: AgentKind }`
- **Modify**: `spawn_acp_agent` to extract and return session info

### Step 3.4: Store session info in ChatWidget
- **File**: `/home/user/nori-cli/codex-rs/tui/src/chatwidget.rs`
- **Add**: Optional field `acp_session_info: Option<AcpSessionInfo>`
- **Update**: On agent spawn, store the session info

### Step 3.5: Run test, verify it passes

### Step 3.6: Commit Phase 3
```bash
git add -A && git commit -m "feat(tui): Track ACP session info for token usage display"
```

---

## Phase 4: Integrate Token Usage into /status Command

### Step 4.1: Write failing snapshot test for status with token usage
- **File**: `/home/user/nori-cli/codex-rs/tui/src/nori/session_header.rs`
- **Test**: Verify token usage section appears in `/status` output when available

### Step 4.2: Run test, verify it fails

### Step 4.3: Modify new_nori_status_output signature
- **File**: `/home/user/nori-cli/codex-rs/tui/src/nori/session_header.rs`
- **Change**: Add optional `TokenUsageReport` parameter
- **Display**: Add "Token Usage" section showing:
  - Total tokens (input + output)
  - Input tokens (with cached breakdown if available)
  - Output tokens (with reasoning breakdown if available)
  - Context window usage percentage if available

### Step 4.4: Update add_status_output in ChatWidget
- **File**: `/home/user/nori-cli/codex-rs/tui/src/chatwidget.rs`
- **Modify**: `add_status_output()` to:
  1. Check if ACP session info is available
  2. If yes, call session discovery to find transcript path
  3. Parse token usage using `parse_session_transcript()`
  4. Pass token usage to `new_nori_status_output()`

### Step 4.5: Run tests, verify they pass
```bash
cargo test -p codex-tui nori_session_header
```

### Step 4.6: Update snapshot tests
```bash
cargo test -p codex-tui -- --update-snapshots
```

### Step 4.7: Commit Phase 4
```bash
git add -A && git commit -m "feat(tui): Display token usage in /status command for ACP sessions"
```

---

## Phase 5: Handle Edge Cases

### Step 5.1: Add test for missing transcript file
- Verify graceful handling when transcript file doesn't exist yet (new session)

### Step 5.2: Add test for malformed/empty transcript
- Verify graceful handling when transcript is empty or malformed

### Step 5.3: Add test for HTTP mode (non-ACP)
- Verify `/status` still works when not using ACP

### Step 5.4: Implement error handling
- Return None for token usage if discovery or parsing fails
- Log warnings for debugging
- Display "Token usage unavailable" or similar when not available

### Step 5.5: Commit Phase 5
```bash
git add -A && git commit -m "fix(tui): Handle edge cases for token usage display"
```

---

## Testing Details

Tests will verify:
1. **Session discovery behavior**: Given agent type + session ID + cwd, correct transcript path is found
2. **Token usage display**: Snapshot tests verify the formatting of token usage in /status output
3. **Edge case handling**: Graceful degradation when transcripts are missing/malformed

## Implementation Details

- Session ID comes from `AcpBackend::session_id()` which wraps `acp::SessionId` from the protocol
- Transcript paths differ per agent:
  - Claude: `~/.claude/projects/<PROJECT_PATH>/<SESSIONID>.jsonl`
  - Codex: `~/.codex/sessions/<YEAR>/<MM>/<DD>/rollout-*-<SESSION_GUID>.jsonl`
  - Gemini: `~/.gemini/tmp/<HASHED_PATHS>/chats/session-*-<SESSIONID>.json`
- `parse_session_transcript()` from `session_parser.rs` already handles token aggregation
- Token usage display reuses existing formatting from `status/card.rs` patterns

## Questions

1. **Claude project path hashing**: Claude stores projects in hashed paths. Need to verify the exact hashing algorithm used (likely SHA-256 or similar). Should check Claude Code source or existing transcript fixtures.

2. **Async vs sync transcript reading**: Should transcript discovery and parsing be async? Currently `parse_session_transcript` uses sync I/O. For responsiveness, might want async with timeout.

3. **Caching**: Should we cache the parsed token usage to avoid re-parsing on every `/status` call? The transcript file grows during the session, so fresh reads may be needed.

---
