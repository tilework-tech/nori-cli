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

### Solution 6: Transcript File Parsing (New - Research-based)

**Approach**: Read token usage directly from agent transcript files stored in home directories.

Each ACP agent stores session transcripts locally with token usage information:

#### Claude Code (`~/.claude/`)

**Location**: `~/.claude/projects/<project_hash>/` (JSONL format)

**Token Usage Structure** (per message):
```json
{
  "role": "assistant",
  "content": "...",
  "usage": {
    "input_tokens": 1234,
    "output_tokens": 567,
    "cache_read_tokens": 890,
    "cache_creation_tokens": 123,
    "total_tokens": 2814
  }
}
```

**Budget Tracking** (internal system tags):
```
<budget:token_budget>200000</budget:token_budget>
<system-warning>Token usage: 37064/200000; 162936 remaining</system-warning>
```

**Extraction**: `jq '[.[] | select(.role=="assistant") | .usage.total_tokens] | add' transcript.jsonl`

**External Tool**: The [`ccusage`](https://github.com/ryoppippi/ccusage) tool already parses these files and provides a `statusline` command:
```json
// ~/.claude/settings.json
{
  "statusLine": {
    "type": "command",
    "command": "bun x ccusage statusline"
  }
}
```

#### Codex (`~/.codex/`)

**Location**: `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl`

**Token Usage Structure** (in `TokenCountEvent`):
```json
{
  "timestamp": "2024-01-01T00:00:00.000Z",
  "type": "event_msg",
  "payload": {
    "TokenCount": {
      "info": {
        "total_token_usage": {
          "input_tokens": 5000,
          "output_tokens": 1200,
          "cached_input_tokens": 3000,
          "reasoning_output_tokens": 0,
          "total_tokens": 6200
        },
        "last_token_usage": { ... },
        "model_context_window": 200000
      }
    }
  }
}
```

**Note**: Codex rollouts already include full token tracking with `model_context_window`.

#### Gemini CLI (`~/.gemini/`)

**Location**: `~/.gemini/tmp/<project_hash>/checkpoints/checkpoint-*.json`

**Shadow Git History**: `~/.gemini/history/<project_hash>/`

**Token Display**: Shown in CLI output but not clearly persisted:
```
Model           Requests  Input     Output
gemini-2.5-pro  12        6,082,929 17,014
Cache savings: 2,401,483 (39.5%) of input tokens served from cache
```

**Note**: Gemini checkpointing must be explicitly enabled in `~/.gemini/settings.json`.

#### Implementation

```rust
// In acp/src/transcript_reader.rs

pub struct TranscriptReader {
    agent_type: AgentType,
    home_dir: PathBuf,
}

impl TranscriptReader {
    pub fn read_current_session_usage(&self) -> Option<TokenUsageInfo> {
        match self.agent_type {
            AgentType::Claude => self.read_claude_transcript(),
            AgentType::Codex => self.read_codex_rollout(),
            AgentType::Gemini => self.read_gemini_checkpoint(),
        }
    }

    fn read_claude_transcript(&self) -> Option<TokenUsageInfo> {
        // Find most recent transcript in ~/.claude/projects/
        // Parse JSONL and sum usage.total_tokens
        // Look for budget tags if available
    }

    fn read_codex_rollout(&self) -> Option<TokenUsageInfo> {
        // Find most recent rollout in ~/.codex/sessions/
        // Find last TokenCountEvent
        // Return TokenUsageInfo directly
    }

    fn read_gemini_checkpoint(&self) -> Option<TokenUsageInfo> {
        // Find most recent checkpoint in ~/.gemini/tmp/
        // Parse JSON checkpoint file
        // May need to estimate if not explicitly stored
    }
}
```

**File Watching** (for live updates):
```rust
use notify::{Watcher, RecursiveMode};

fn watch_transcript_updates(reader: &TranscriptReader, callback: impl Fn(TokenUsageInfo)) {
    let mut watcher = notify::recommended_watcher(move |res| {
        if let Ok(event) = res {
            if let Some(usage) = reader.read_current_session_usage() {
                callback(usage);
            }
        }
    })?;

    watcher.watch(&reader.transcript_path(), RecursiveMode::NonRecursive)?;
}
```

**Pros**:
- Uses **actual token counts** from the agent (not estimates)
- Works without protocol changes
- Data already exists - just needs to be read
- Can leverage existing tools (`ccusage`) as reference
- Claude and Codex have well-defined formats

**Cons**:
- File paths may change between agent versions
- Requires file system access to user's home directory
- Race conditions possible during active writes
- Gemini format is less well-documented
- Need to identify correct session/transcript for current conversation

**Agent Transcript Summary**:

| Agent | Location | Format | Token Info | Context Window |
|-------|----------|--------|------------|----------------|
| Claude Code | `~/.claude/projects/<hash>/` | JSONL | ✅ Per-message `usage` | ✅ Budget tags |
| Codex | `~/.codex/sessions/YYYY/MM/DD/` | JSONL | ✅ `TokenCountEvent` | ✅ In event |
| Gemini | `~/.gemini/tmp/<hash>/checkpoints/` | JSON | ⚠️ Runtime only | ❌ Not persisted |

---

### Solution 5: Hybrid Approach (Updated Recommendation)

**Approach**: Combine multiple solutions with a prioritized fallback chain.

**Phase 1 (Immediate)**: Transcript file parsing (Solution 6)
- Read actual token usage from agent home directory transcripts
- Claude (`~/.claude/`) and Codex (`~/.codex/`) have reliable JSONL formats
- Use file watching for live updates during sessions

**Phase 2 (Fallback)**: Client-side estimation (Solution 3)
- For agents without accessible transcripts (e.g., Gemini)
- Basic token estimation using character count (~4 chars per token)
- Show estimate with visual indicator (e.g., "~75% remaining")

**Phase 3 (Long-term)**: ACP protocol integration (Solution 4)
- When ACP adds token usage support, integrate properly
- Replace transcript parsing with direct protocol data
- Remove file system dependencies

**Implementation**:

```rust
// In acp/src/context_tracker.rs

pub enum ContextSource {
    /// Read from agent's transcript files (most accurate)
    Transcript,
    /// Client-side estimation (fallback)
    Estimated,
    /// From ACP protocol (future)
    Protocol,
}

pub struct ContextUsageTracker {
    agent_type: AgentType,
    transcript_reader: Option<TranscriptReader>,
    estimated_tokens: i64,
    context_window: i64,
    source: ContextSource,
}

impl ContextUsageTracker {
    pub fn get_context_remaining(&self) -> ContextRemaining {
        // Priority: Transcript > Protocol > Estimation
        let (tokens_used, source) = if let Some(reader) = &self.transcript_reader {
            if let Some(usage) = reader.read_current_session_usage() {
                (usage.total_token_usage.total_tokens, ContextSource::Transcript)
            } else {
                (self.estimated_tokens, ContextSource::Estimated)
            }
        } else {
            (self.estimated_tokens, ContextSource::Estimated)
        };

        ContextRemaining {
            percent: self.calculate_percent(tokens_used),
            tokens_used,
            context_window: self.context_window,
            source,
        }
    }
}
```

Update status bar to show source indicator:
```rust
// In tui/src/status/card.rs
fn context_window_spans(&self) -> Option<Vec<Span<'static>>> {
    let context = self.token_usage.context_window.as_ref()?;
    let prefix = match context.source {
        ContextSource::Transcript => "",       // Accurate - no prefix
        ContextSource::Estimated => "~",       // Estimate indicator
        ContextSource::Protocol => "",         // Accurate from ACP
    };
    Some(vec![
        Span::from(format!("{prefix}{percent}% left")),
        // ...
    ])
}
```

## Recommendation

**Start with Solution 5 (Hybrid Approach)** with transcript parsing as primary source:

1. **Accurate data**: Transcript files contain actual token counts, not estimates
2. **Immediate availability**: Claude and Codex already have well-documented formats
3. **No protocol changes**: Works with current agent implementations
4. **Graceful degradation**: Falls back to estimation when transcripts unavailable
5. **Future-proof**: Clean path to ACP protocol integration

**Implementation Priority**:
1. Claude Code transcript parsing (best documented, `ccusage` as reference)
2. Codex rollout parsing (already has `TokenCountEvent` structure)
3. Client-side estimation fallback (for Gemini and unknown agents)
4. ACP protocol integration (when available)

## File Changes Required

| File | Changes |
|------|---------|
| `acp/src/transcript_reader.rs` | **NEW**: Agent-specific transcript parsers |
| `acp/src/context_tracker.rs` | **NEW**: Unified context tracking with fallback chain |
| `acp/src/backend.rs` | Integrate `ContextUsageTracker`, emit `TokenCountEvent` |
| `acp/src/registry.rs` | Add `home_dir_pattern` and `context_window` to `AcpAgentConfig` |
| `protocol/src/protocol.rs` | Add `ContextSource` enum to `TokenUsageInfo` |
| `tui/src/status/card.rs` | Update display to show source indicator (`~` for estimates) |
| `tui/src/chatwidget.rs` | Pass context info from transcript reader to status display |
| `Cargo.toml` | Add `notify` crate for file watching |

## Open Questions

1. How do we identify the correct transcript for the current ACP session?
2. Should transcript reading be synchronous or async with file watching?
3. How do we handle race conditions when agent is actively writing to transcript?
4. Should we cache transcript data or read fresh on each status update?
5. What's the polling interval for transcript updates vs file watching?

## References

- Current token tracking: `codex-rs/protocol/src/protocol.rs` (lines 759-923)
- HTTP provider parsing: `codex-rs/codex-api/src/sse/responses.rs`
- Status bar display: `codex-rs/tui/src/status/card.rs`
- ACP backend: `codex-rs/acp/src/backend.rs`
- ACP protocol: `agent-client-protocol` crate v0.9.0
- Claude Code transcripts: `~/.claude/projects/` (JSONL with `usage` field)
- Codex rollouts: `~/.codex/sessions/` (JSONL with `TokenCountEvent`)
- Gemini checkpoints: `~/.gemini/tmp/` (JSON, checkpointing must be enabled)
- External tool: [`ccusage`](https://github.com/ryoppippi/ccusage) - parses Claude/Codex transcripts
- Claude Code feature request: [Issue #10593](https://github.com/anthropics/claude-code/issues/10593)
