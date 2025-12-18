# Proposal: Context Remaining for ACP Agents

## Problem Statement

The "context remaining" feature in the status bar currently only works for HTTP provider models. When using ACP (Agent Context Protocol) agents exclusively, the status bar shows no context usage information because:

1. **HTTP providers** extract token usage from API responses (e.g., OpenAI's `usage` field in responses) and emit `TokenCountEvent` events
2. **ACP agents** manage their own context internally and don't expose token usage through the current protocol
3. The `TaskStartedEvent` for ACP always has `model_context_window: None`

## Current Architecture

### Token Usage Flow (HTTP Mode)

```
┌─────────────────────────┐     ResponseCompletedUsage      ┌─────────────────────────┐
│   HTTP Provider API     │──────────────────────────────►  │   sse/responses.rs      │
│   (OpenAI, etc.)        │     { input_tokens,             │   parse usage from SSE  │
│                         │       output_tokens, ... }      │                         │
└─────────────────────────┘                                 └───────────┬─────────────┘
                                                                        │
                                                                        ▼
                                        ┌─────────────────────────────────────────────┐
                                        │   TokenUsageInfo                            │
                                        │   - total_token_usage: TokenUsage           │
                                        │   - last_token_usage: TokenUsage            │
                                        │   - model_context_window: Option<i64>       │
                                        └───────────────────────┬─────────────────────┘
                                                                │
                                                                ▼
                                        ┌─────────────────────────────────────────────┐
                                        │   Status Bar (status/card.rs)               │
                                        │   - percent_remaining                       │
                                        │   - tokens_in_context / window              │
                                        └─────────────────────────────────────────────┘
```

### ACP Mode (Current - No Token Tracking)

```
┌─────────────────────────┐     SessionUpdate               ┌─────────────────────────┐
│   ACP Agent             │──────────────────────────────►  │   backend.rs            │
│   (Claude, Gemini, etc.)│     (no token info)             │   translate_session_    │
│                         │                                 │   update_to_events()    │
└─────────────────────────┘                                 └───────────┬─────────────┘
                                                                        │
                                                                        ▼
                                        ┌─────────────────────────────────────────────┐
                                        │   TaskStartedEvent                          │
                                        │   - model_context_window: None  ❌          │
                                        └─────────────────────────────────────────────┘
```

## Proposed Solutions

### Solution 1: Agent Slash Commands (Short-term, Agent-specific)

**Approach**: Send agent-native slash commands to query context usage.

Different ACP agents have their own internal commands for showing status:
- **Claude (Claude Code ACP)**: `/context` - shows context usage
- **Codex**: `/status` - shows session configuration and token usage
- **Gemini**: `/stats` - shows statistics

**Implementation**:

1. Add an `ACP_CONTEXT_COMMAND` registry mapping agent types to their context commands:
   ```rust
   // In acp/src/registry.rs
   pub struct AcpAgentConfig {
       // ... existing fields
       pub context_command: Option<String>,  // e.g., "/context", "/status", "/stats"
   }
   ```

2. Create a `query_context_usage()` method in `AcpBackend`:
   ```rust
   impl AcpBackend {
       pub async fn query_context_usage(&self) -> Option<ContextUsageInfo> {
           let cmd = self.get_context_command()?;
           // Send command to agent and parse response
           // Different agents will have different response formats
       }
   }
   ```

3. Parse agent-specific responses to extract context info

**Pros**:
- Works with existing agent capabilities
- No protocol changes needed
- Can be implemented incrementally per-agent

**Cons**:
- Fragile: depends on agent output format staying consistent
- Agent-specific parsing required for each agent type
- May interfere with normal conversation flow
- Response format may change between agent versions

---

### Solution 2: ACP Protocol Extension (Medium-term, Standardized)

**Approach**: Extend ACP to include token usage in session updates or add a dedicated query method.

**Option 2a: Token usage in SessionUpdate**

Add a new `SessionUpdate` variant for token information:

```rust
// In agent-client-protocol
pub enum SessionUpdate {
    // ... existing variants
    TokenUsage(TokenUsageUpdate),
}

pub struct TokenUsageUpdate {
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub total_tokens: i64,
    pub context_window: Option<i64>,
}
```

Agents would emit this after each turn completion.

**Option 2b: Query method**

Add a new ACP method for querying session state:

```rust
// New ACP method
pub struct GetSessionStateRequest {
    pub session_id: SessionId,
}

pub struct GetSessionStateResponse {
    pub token_usage: Option<TokenUsageInfo>,
    pub context_window: Option<i64>,
    // ... other session state
}
```

**Implementation in Nori**:

```rust
// In acp/src/backend.rs
impl AcpBackend {
    pub async fn get_session_state(&self) -> Option<SessionState> {
        // New ACP method call
        self.connection.get_session_state(&self.session_id).await.ok()
    }
}
```

**Pros**:
- Standardized across all ACP agents
- Clean protocol-level solution
- Future-proof

**Cons**:
- Requires ACP protocol version bump
- Requires all agent implementations to be updated
- Coordination needed across agent providers (Anthropic, Google, OpenAI)

---

### Solution 3: Client-side Token Estimation (Short-term, Approximate)

**Approach**: Estimate token usage on the client side using tokenizer libraries.

**Implementation**:

1. Add a tokenizer dependency (e.g., `tiktoken-rs` for OpenAI-compatible models):
   ```toml
   # Cargo.toml
   tiktoken-rs = "0.5"
   ```

2. Track messages and estimate tokens:
   ```rust
   // In acp/src/backend.rs
   struct TokenEstimator {
       model_context_window: i64,
       estimated_input_tokens: i64,
       estimated_output_tokens: i64,
   }

   impl TokenEstimator {
       fn estimate_text_tokens(&self, text: &str) -> i64 {
           // Use tiktoken or char-based estimation
       }

       fn add_input(&mut self, text: &str) {
           self.estimated_input_tokens += self.estimate_text_tokens(text);
       }

       fn add_output(&mut self, text: &str) {
           self.estimated_output_tokens += self.estimate_text_tokens(text);
       }
   }
   ```

3. Track all `AgentMessageChunk` and user inputs to accumulate estimates

**Pros**:
- Works immediately with all agents
- No agent changes needed
- Provides useful approximation

**Cons**:
- Estimates may be inaccurate (especially for non-OpenAI tokenizers)
- Doesn't account for agent's internal processing, tool results, system prompts
- Different models use different tokenizers
- May significantly underestimate actual context usage

---

### Solution 4: Experimental ACP Feature (Future, Best Solution)

**Approach**: Wait for and adopt the experimental ACP feature for queryable context info.

The ACP protocol is under active development. An experimental feature could provide:

1. **Per-turn token usage** in `PromptResponse`:
   ```rust
   pub struct PromptResponse {
       pub stop_reason: StopReason,
       pub usage: Option<Usage>,  // NEW
   }

   pub struct Usage {
       pub input_tokens: i64,
       pub output_tokens: i64,
       pub cache_read_tokens: Option<i64>,
       pub cache_write_tokens: Option<i64>,
   }
   ```

2. **Session-level context query**:
   ```rust
   // New ACP method
   async fn get_context_usage(session_id: SessionId) -> ContextUsage;
   ```

**Implementation plan**:

1. Monitor `agent-client-protocol` crate for new features
2. Add feature flag: `acp_context_usage`
3. When available, integrate into `AcpBackend`:
   ```rust
   #[cfg(feature = "acp_context_usage")]
   fn update_token_usage(&mut self, usage: &acp::Usage) {
       // Update TokenUsageInfo and emit TokenCountEvent
   }
   ```

**Pros**:
- Proper protocol-level solution
- Accurate token counts from the agent itself
- Standardized across all compliant agents

**Cons**:
- Depends on external protocol development timeline
- May take time to be implemented by all agents

---

### Solution 5: Hybrid Approach (Recommended)

**Approach**: Combine multiple solutions for immediate value with a path to proper support.

**Phase 1 (Immediate)**: Client-side estimation
- Implement basic token estimation using character count (~4 chars per token)
- Show estimate with visual indicator (e.g., "~75% remaining")
- Clear indication this is an estimate, not exact

**Phase 2 (Short-term)**: Agent-specific slash commands
- Add support for querying agents that expose context commands
- Use actual data when available, fall back to estimation

**Phase 3 (Medium-term)**: ACP protocol integration
- When ACP adds token usage support, integrate properly
- Replace estimation with actual values
- Remove agent-specific workarounds

**Implementation**:

```rust
// In acp/src/backend.rs

pub struct ContextUsageTracker {
    /// Estimated tokens from client-side counting
    estimated_tokens: i64,
    /// Actual tokens from agent (if available)
    actual_tokens: Option<i64>,
    /// Model's context window size
    context_window: i64,
    /// Whether we're using estimates
    is_estimated: bool,
}

impl ContextUsageTracker {
    pub fn get_context_remaining(&self) -> ContextRemaining {
        ContextRemaining {
            percent: self.calculate_percent(),
            tokens_used: self.actual_tokens.unwrap_or(self.estimated_tokens),
            context_window: self.context_window,
            is_estimated: self.actual_tokens.is_none(),
        }
    }
}
```

Update status bar to show estimation indicator:
```rust
// In tui/src/status/card.rs
fn context_window_spans(&self) -> Option<Vec<Span<'static>>> {
    let context = self.token_usage.context_window.as_ref()?;
    let prefix = if context.is_estimated { "~" } else { "" };
    Some(vec![
        Span::from(format!("{prefix}{percent}% left")),
        // ...
    ])
}
```

## Recommendation

**Start with Solution 5 (Hybrid Approach)** because:

1. **Immediate value**: Users get context awareness right away with estimates
2. **Progressive enhancement**: Accuracy improves as better data sources become available
3. **Future-proof**: Clean path to proper ACP integration
4. **User transparency**: Clear indication when showing estimates vs actual values

## File Changes Required

| File | Changes |
|------|---------|
| `acp/src/backend.rs` | Add `ContextUsageTracker`, integrate with event translation |
| `acp/src/registry.rs` | Add `context_window` and optionally `context_command` to `AcpAgentConfig` |
| `protocol/src/protocol.rs` | Add `is_estimated` field to `TokenUsageInfo` or create new type |
| `tui/src/status/card.rs` | Update display to show estimation indicator |
| `tui/src/chatwidget.rs` | Pass estimated context info to status display |

## Open Questions

1. Should we show any context info if we're highly uncertain about the estimate?
2. How should we handle agents with unknown context window sizes?
3. Should the estimation be configurable/disableable?
4. What's the acceptable margin of error for estimates?

## References

- Current token tracking: `codex-rs/protocol/src/protocol.rs` (lines 759-923)
- HTTP provider parsing: `codex-rs/codex-api/src/sse/responses.rs`
- Status bar display: `codex-rs/tui/src/status/card.rs`
- ACP backend: `codex-rs/acp/src/backend.rs`
- ACP protocol: `agent-client-protocol` crate v0.9.0
