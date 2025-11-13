# ACP Integration Implementation Plan

## Status: Phase 1 Complete ✅

This document outlines the implementation plan for integrating the Agent Client Protocol (ACP) into nori-cli.

## Completed Work

### Phase 1: Foundation & Client Handler ✅

**Files Modified:**
- `src/conversation.rs` - Extended `ConversationEvent` enum with ACP-specific events
- `src/acp_runner.rs` - New module with event translation and client handler
- `src/main.rs` - Added acp_runner module declaration

**Implemented:**
1. **Extended ConversationEvent enum** with new variants:
   - `ToolCallStarted` - When agent initiates a tool call
   - `ToolCallProgress` - Tool execution progress updates
   - `AgentPlan` - Agent's execution plan with multiple steps
   - `AgentThinking` - Agent's internal reasoning/thought process
   - `PlanEntry` - Individual steps in agent's plan

2. **Event Translation Layer** (`translate_session_update`):
   - Converts ACP `SessionUpdate` → `ConversationEvent`
   - Handles all message types (agent, user, thought chunks)
   - Translates tool call lifecycle events
   - Maps plan entries with status and priority

3. **AcpClientHandler** - Full `Client` trait implementation:
   - `request_permission()` - Auto-approves by selecting first "allow" option
   - `session_notification()` - Forwards updates to event stream via mpsc channel
   - `read_text_file()` - Reads files from working directory (auto-approved)
   - `write_text_file()` - Writes files with directory creation (auto-approved)
   - Terminal methods blocked (return `method_not_found` errors)

4. **Tests** - 7 passing unit tests:
   - Agent/user/thought message chunk translation
   - Tool call start/update translation
   - Plan translation with priorities
   - Non-text content returns None

**Architecture:**
```
ConversationEvent (UI) ← translate_session_update ← SessionUpdate (ACP)
                                                           ↑
                                                    AcpClientHandler
                                                      implements Client
```

---

## Remaining Work

### Phase 2: Agent Runner Core (HIGH PRIORITY)

**File:** `src/acp_runner.rs`

**Implement `AcpAgentRunner::spawn_stream()`:**

This is the core async method that manages the full ACP lifecycle:

```rust
pub async fn spawn_stream(
    &mut self,
    prompt: String,
    cancel_token: CancellationToken,
) -> Result<Pin<Box<dyn Stream<Item = ConversationEvent> + Send>>, String>
```

**Step-by-step implementation:**

1. **Spawn agent subprocess:**
   ```rust
   let mut cmd = Command::new(&self.config.command);
   cmd.args(&self.config.args)
      .stdin(Stdio::piped())
      .stdout(Stdio::piped())
      .stderr(Stdio::null());
   let mut child = cmd.spawn()?;
   ```

2. **Create JSON-RPC transport:**
   - Need to add dependency: `agent-client-protocol` has RPC helpers
   - Create bidirectional JSON-RPC stream over stdin/stdout
   - Use `agent_client_protocol::rpc` module

3. **Perform initialization handshake:**
   ```rust
   // Send initialize request
   let init_request = InitializeRequest {
       protocol_version: 1,
       client_capabilities: ClientCapabilities {
           fs: Some(FsCapabilities {
               read_text_file: true,
               write_text_file: true,
           }),
           terminal: false, // Blocked
       },
       client_info: Some(Implementation {
           name: "nori-cli".to_string(),
           title: Some("Nori CLI".to_string()),
           version: env!("CARGO_PKG_VERSION").to_string(),
       }),
       meta: None,
   };

   let init_response: InitializeResponse = rpc.call("initialize", init_request).await?;
   ```

4. **Create session:**
   ```rust
   let session_request = SessionNewRequest {
       cwd: self.cwd.clone(),
       mcp_servers: vec![], // Empty for now, add config later
   };

   let session_response: SessionNewResponse = rpc.call("session/new", session_request).await?;
   let session_id = session_response.session_id;
   ```

5. **Create client handler and event channel:**
   ```rust
   let (update_tx, mut update_rx) = mpsc::unbounded_channel();
   let client = AcpClientHandler::new(self.cwd.clone(), update_tx, cancel_token.clone());
   ```

6. **Send prompt:**
   ```rust
   let prompt_request = SessionPromptRequest {
       session_id: session_id.clone(),
       prompt: vec![ContentBlock::Text(TextContent {
           annotations: None,
           text: prompt,
           meta: None,
       })],
   };

   // This is async and will return when the turn completes
   tokio::spawn(async move {
       let result = rpc.call("session/prompt", prompt_request).await;
       // Handle result
   });
   ```

7. **Create event stream:**
   ```rust
   let stream = stream! {
       while let Some(update) = update_rx.recv().await {
           if let Some(event) = translate_session_update(update) {
               yield event;
           }
       }
   };

   Ok(Box::pin(stream))
   ```

**Challenges:**
- Need to understand `agent-client-protocol::rpc` module API
- Must handle bidirectional JSON-RPC (client calls agent, agent calls client)
- Session lifecycle management (store session_id for reuse)
- Error handling at each step

**Dependencies to add:**
- May need `serde_json` for JSON-RPC serialization (likely already available)
- Check if `agent-client-protocol` v0.7.0 includes RPC helpers

---

### Phase 3: Main Integration (MEDIUM PRIORITY)

**File:** `src/main.rs` (around line 232)

**Make spawn_stream async:**

Current code:
```rust
let backend = get_backend(&model);
let stream = backend.spawn_stream(prompt, cancel_token);
```

New code:
```rust
let agent_config = get_agent_config(&model);
let mut acp_runner = AcpAgentRunner::new(agent_config, model.cwd.clone());
let stream = acp_runner.spawn_stream(prompt, cancel_token).await?;
```

**Changes needed:**
1. Make the message handler async or spawn a task
2. Handle the `Result` from `spawn_stream`
3. Display errors to user if initialization fails

---

### Phase 4: Agent Configurations (LOW PRIORITY)

**File:** `src/backends.rs`

**Add agent configurations:**
```rust
pub const CLAUDE_CONFIG: AcpAgentConfig = AcpAgentConfig {
    name: "Claude Code",
    command: "claude",
    args: vec![],
    install_url: "https://code.claude.com",
    install_command: None,
};

pub const CODEX_CONFIG: AcpAgentConfig = AcpAgentConfig {
    name: "GPT Codex",
    command: "codex",
    args: vec![],
    install_url: "",
    install_command: Some(vec![
        "npm".to_string(),
        "install".to_string(),
        "-g".to_string(),
        "@openai/codex".to_string(),
    ]),
};

pub fn get_agent_config(model: &Model) -> AcpAgentConfig {
    match model.selected_model {
        0 => CLAUDE_CONFIG,
        1 => CODEX_CONFIG,
        _ => CLAUDE_CONFIG,
    }
}
```

---

### Phase 5: Testing & Documentation

**Testing:**
1. Unit tests for `AcpAgentRunner` methods
2. Integration test with mock ACP agent binary
3. Manual test with real Claude Code installation

**Documentation:**
1. Update `src/backends/docs.md` to explain ACP architecture
2. Add comments to complex parts of `spawn_stream()`
3. Document the event translation layer
4. Update README if needed

---

## Migration Strategy

**Gradual rollout:**
1. Keep old `AgentBackend` trait alongside new ACP implementation
2. Add `--use-acp` flag to enable new backend
3. Test thoroughly with both implementations
4. Make ACP the default after 1-2 releases
5. Remove old implementations

**Backward compatibility:**
- Keep `MockBackend` for testing
- Existing commands and UI remain unchanged
- Only internal backend mechanism changes

---

## Open Questions

1. **Does agent-client-protocol v0.7.0 include RPC helpers?**
   - Need to check crate docs
   - May need to implement JSON-RPC layer manually
   - Alternative: Use existing JSON-RPC library

2. **Session persistence:**
   - Currently out of scope
   - Future: Store session IDs in `~/.config/nori-cli/sessions.json`
   - Implement `/resume <session-id>` command

3. **MCP server configuration:**
   - Currently passing empty array
   - Future: Read from `~/.config/nori-cli/mcp-servers.json`
   - Allow per-project MCP configuration

4. **Error handling granularity:**
   - Should we show raw JSON-RPC errors or user-friendly messages?
   - Add debug mode flag for detailed errors

5. **Cancellation handling:**
   - Need to send `session/cancel` notification on Ctrl-C
   - Verify agent properly cleans up after cancellation

---

## Next Steps

**Immediate (Blocking):**
1. Research `agent-client-protocol::rpc` module or find JSON-RPC library
2. Implement `spawn_stream()` core logic
3. Test with mock responses first, then real agent

**Short-term:**
1. Update main.rs integration
2. Add agent configurations
3. Write integration tests

**Long-term:**
1. Session persistence
2. MCP server configuration
3. UI improvements for tool calls and plans
4. Remove old backend implementations

---

## Technical Notes

**Key files:**
- `/home/clifford/Documents/source/codex/nori-cli/.worktrees/setup-acp-backend/src/acp_runner.rs`
- `/home/clifford/Documents/source/codex/nori-cli/.worktrees/setup-acp-backend/src/main.rs`
- `/home/clifford/Documents/source/codex/nori-cli/.worktrees/setup-acp-backend/src/backends.rs`

**Dependencies:**
- `agent-client-protocol = "0.7.0"` ✅ already in Cargo.toml
- `async-trait` - check if needed (already used implicitly)
- JSON-RPC library - TBD

**Testing approach:**
- Unit tests for event translation ✅ (7 tests passing)
- Unit tests for client handler methods (TODO)
- Integration test with mock agent (TODO)
- Manual test with Claude Code (TODO)

---

## Lessons Learned

1. **ACP types are tuple variants, not struct variants:**
   - `ContentBlock::Text(TextContent)` not `ContentBlock::Text { text: ... }`
   - `ToolCall::id` not `ToolCall::tool_call_id`

2. **Permission handling uses Selected not Approved:**
   - Must provide `option_id` from request
   - Options: `AllowOnce`, `AllowAlways`, `RejectOnce`, `RejectAlways`

3. **All response structs have `meta` field:**
   - Always set `meta: None` for simple responses
   - Use for custom extensions if needed

4. **Error construction:**
   - `Error::internal_error()` takes no message parameter
   - Use `Error::new(ErrorCode::X)` for custom errors

---

**Last Updated:** 2025-11-12
**Status:** Phase 1 Complete, Phase 2 Ready to Start
