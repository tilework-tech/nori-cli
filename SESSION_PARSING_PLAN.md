# Session Transcript Parsing Implementation Plan

**Goal:** Design and implement proper parsing logic for session transcript formats from Codex, Gemini, and Claude agents to extract key metadata, especially context usage metrics (tokens in/out).

**Architecture:** Create agent-specific parsers that handle the unique JSONL/JSON formats for each agent. Codex uses JSONL with `event_msg` events containing token counts. Gemini uses JSON with per-message token objects. Claude uses JSONL with usage objects embedded in assistant message entries. Each parser extracts session metadata and normalizes token usage information into a common format. Parsers will be integrated into the existing `codex-acp` module.

**Tech Stack:** Rust with serde for JSON/JSONL parsing, existing codex-rs infrastructure (particularly `codex-protocol` for `TokenUsage` structs and `codex-acp` for ACP integration)

---

## Context: Why This Matters

The Nori tool interacts with some agents (like Codex) as subprocesses with opaque state through the Agent Client Protocol (ACP). While ACP provides a standard interface, **it is still necessary to parse session transcripts directly** when tracking tokens and context usage from these agents. This is because:

1. Agents maintain their own internal session state
2. Token tracking needs to rely on each agent's own measurements (not estimates)
3. Different agents report context usage in different formats

**IMPORTANT:** `codex-core` is a deprecated module used mainly for HTTP interactions with OpenAI models. The codebase now primarily interacts with subprocess agents through ACP, not HTTP queries to model providers.

## Session Transcript Formats

### Codex Session Format
**File:** `session-codex.jsonl` (JSONL - newline-delimited JSON)

**Key Characteristics:**
- Each line is a separate JSON object with `timestamp` and `type` fields
- Token usage reported in `event_msg` events with `payload.type = "token_count"`
- Contains `total_token_usage` (cumulative) and `last_token_usage` (per-turn)
- Includes `model_context_window` for context limit
- Events include: `response_item`, `event_msg`, `turn_context`

**Token Usage Location:**
```json
{
  "timestamp": "2025-12-12T21:24:49.666Z",
  "type": "event_msg",
  "payload": {
    "type": "token_count",
    "info": {
      "total_token_usage": {
        "input_tokens": 5178,
        "cached_input_tokens": 0,
        "output_tokens": 35,
        "reasoning_output_tokens": 0,
        "total_tokens": 5213
      },
      "last_token_usage": {
        "input_tokens": 5178,
        "cached_input_tokens": 0,
        "output_tokens": 35,
        "reasoning_output_tokens": 0,
        "total_tokens": 5213
      },
      "model_context_window": 258400
    }
  }
}
```

**Parsing Strategy:**
- Stream JSONL line-by-line
- Filter for `type == "event_msg"` AND `payload.type == "token_count"`
- Extract `last_token_usage` from the LAST token_count event as final session total
- Track `model_context_window` for context capacity

### Gemini Session Format
**File:** `session-gemini.json` (Single JSON object)

**Key Characteristics:**
- Root object with `sessionId`, `startTime`, `lastUpdated`, `messages` array
- Token usage in `tokens` field of each Gemini message
- Must aggregate token usage across all messages
- Includes `thoughts` field for reasoning (tracked separately in token counts)

**Token Usage Location:**
```json
{
  "sessionId": "d126c5e7-62ae-471a-8a5e-2cf6ddac8a9b",
  "messages": [
    {
      "id": "ce9d1a40-c1aa-4bea-9b7c-e08a248f6db9",
      "type": "gemini",
      "tokens": {
        "input": 8425,
        "output": 126,
        "cached": 7887,
        "thoughts": 197,
        "tool": 0,
        "total": 8748
      }
    }
  ]
}
```

**Parsing Strategy:**
- Parse full JSON object
- Iterate `messages` array
- Filter for `type == "gemini"` messages
- Sum `tokens.total` across all Gemini messages for session total
- Note: `cached` is subset of `input`, don't double-count

### Claude Session Format  
**File:** `session-claude.jsonl` (JSONL - newline-delimited JSON)

**Key Characteristics:**
- Each line is a JSON object with `type`, `message`, `sessionId`, `timestamp` fields
- Token usage in `message.usage` field of assistant message objects
- Tracks cache creation and reads separately (`cache_creation`, `cache_read_input_tokens`)
- Uses multi-tier caching (ephemeral_5m, ephemeral_1h)

**Token Usage Location:**
```json
{
  "type": "assistant",
  "sessionId": "ccded934-ae45-4ef6-9271-950657d2161a",
  "message": {
    "model": "claude-opus-4-5-20251101",
    "usage": {
      "input_tokens": 3,
      "cache_creation_input_tokens": 22285,
      "cache_read_input_tokens": 0,
      "cache_creation": {
        "ephemeral_5m_input_tokens": 22285,
        "ephemeral_1h_input_tokens": 0
      },
      "output_tokens": 1,
      "service_tier": "standard"
    }
  }
}
```

**Parsing Strategy:**
- Stream JSONL line-by-line
- Filter for `type == "assistant"` entries
- Extract `message.usage` from the LAST assistant message as final session total
- Sum `input_tokens + cache_creation_input_tokens + cache_read_input_tokens` for total input
- Add `output_tokens` for complete token count

## Testing Plan

**Unit Tests:**
1. **Codex Parser Tests** (`codex-acp/src/session_parser/codex.rs`)
   - Parse `session-codex.jsonl` fixture and verify token extraction
   - Test that parser correctly identifies last `token_count` event
   - Verify `last_token_usage` extraction (not `total_token_usage`)
   - Test handling of missing `cached_input_tokens` (defaults to 0)
   - Test handling of missing `reasoning_output_tokens` (defaults to 0)
   - Verify assertion that `total_tokens == input + output` holds

2. **Gemini Parser Tests** (`codex-acp/src/session_parser/gemini.rs`)
   - Parse `session-gemini.json` fixture and verify token extraction
   - Test token aggregation across multiple Gemini messages
   - Verify that `cached` tokens are NOT double-counted with `input`
   - Test sessions with only user messages (no Gemini responses)
   - Verify assertion that `total == input + output + thoughts + tool` holds

3. **Claude Parser Tests** (`codex-acp/src/session_parser/claude.rs`)
   - Parse `session-claude.jsonl` fixture and verify token extraction
   - Test that parser correctly identifies last `assistant` message
   - Verify cache token aggregation (creation + read)
   - Test handling of nested `cache_creation` object
   - Test sessions with tool calls (verify usage still extracted)

**Integration Tests:**
1. Parse all three example files and verify correct final token counts
2. Test error handling for malformed JSON/JSONL
3. Test error handling for missing usage fields (return `Option::None`)

**NOTE:** I will write *all* tests before I add any implementation behavior.

## Implementation Tasks

### Phase 1: Data Structure Design
1. Create `/home/clifford/Documents/source/nori/cli/.worktrees/session-transcript-parsing/codex-rs/acp/src/session_parser/mod.rs`
2. Define `TokenUsageReport` struct that wraps existing `TokenUsage` from `codex-protocol`
3. Add enum `AgentSessionFormat { Codex, Gemini, Claude }` to distinguish parsers
4. Export public API: `parse_session_transcript(format: AgentSessionFormat, path: PathBuf) -> Result<Option<TokenUsageReport>>`

### Phase 2: Codex Parser Implementation  
1. Create `/home/clifford/Documents/source/nori/cli/.worktrees/session-transcript-parsing/codex-rs/acp/src/session_parser/codex.rs`
2. Write failing test: `test_parse_codex_session_extracts_last_token_usage()`
3. Implement JSONL streaming parser using `serde_json::Deserializer::from_reader`
4. Filter for `event_msg` with `token_count` type
5. Track last `last_token_usage` object encountered
6. Assert `last_token_usage.total_tokens == input + cached_input + output + reasoning_output`
7. Return `TokenUsageReport` populated from `last_token_usage`
8. Run test and verify it passes

### Phase 3: Gemini Parser Implementation
1. Create `/home/clifford/Documents/source/nori/cli/.worktrees/session-transcript-parsing/codex-rs/acp/src/session_parser/gemini.rs`
2. Write failing test: `test_parse_gemini_session_aggregates_tokens()`
3. Implement JSON parser for root object using `serde_json::from_reader`
4. Iterate `messages` array and filter for `type == "gemini"`
5. Sum `tokens.input`, `tokens.output`, `tokens.thoughts`, `tokens.tool` separately
6. Assert `total_calculated == sum_of_totals` from message tokens
7. Return `TokenUsageReport` with aggregated values
8. Run test and verify it passes

### Phase 4: Claude Parser Implementation
1. Create `/home/clifford/Documents/source/nori/cli/.worktrees/session-transcript-parsing/codex-rs/acp/src/session_parser/claude.rs`
2. Write failing test: `test_parse_claude_session_extracts_last_usage()`
3. Implement JSONL streaming parser
4. Filter for `type == "assistant"` entries
5. Track last `message.usage` object encountered
6. Calculate total input as: `input_tokens + cache_creation_input_tokens + cache_read_input_tokens`
7. Assert `calculated_total == input_total + output_tokens`
8. Return `TokenUsageReport` with aggregated cache tokens
9. Run test and verify it passes

### Phase 5: Integration and Error Handling
1. Update `/home/clifford/Documents/source/nori/cli/.worktrees/session-transcript-parsing/codex-rs/acp/src/session_parser/mod.rs` with all parser modules
2. Implement `parse_session_transcript()` dispatcher function
3. Add error type `SessionParseError` for IO errors, JSON errors, format errors
4. Write test for malformed JSON handling (returns error)
5. Write test for missing token fields (returns `None`)
6. Run all tests and verify they pass

### Phase 6: Documentation
1. Add rustdoc comments to all public types and functions
2. Document expected file formats for each parser
3. Add usage examples to module-level documentation
4. Document that parsers use final/last token counts (not cumulative sums for Codex/Claude)

---

## Edge Cases

1. **Missing token usage data:**
   - Parser returns `Option::None` if no token usage found
   - Test with sessions that have no `token_count` events (Codex)
   - Test with sessions that have no Gemini messages (Gemini)
   - Test with sessions that have no assistant messages (Claude)

2. **Malformed files:**
   - Truncated JSONL (incomplete line) - return IO error
   - Invalid JSON within line - return parse error
   - Missing required fields - return parse error or `None` based on severity

3. **Cache token accounting:**
   - Gemini: `cached` is subset of `input`, don't add separately
   - Claude: cache tokens are separate fields, must sum all three input types
   - Codex: `cached_input_tokens` is separate count

4. **Assertion failures:**
   - When `total != input + output`, surface to engineer via test failure
   - Document discrepancy for investigation
   - Do NOT silently ignore inconsistencies

5. **Large session files:**
   - Use streaming parsers (`Deserializer::from_reader`) to avoid loading entire file
   - For Claude/Codex: only keep last relevant event in memory
   - For Gemini: must load full JSON (but this is bounded by session size)

6. **Empty sessions:**
   - Sessions with no turns/messages should return `None`
   - Don't error on empty but valid JSON/JSONL

---

**Testing Details:**
- Unit tests verify JSON/JSONL parsing for each agent format using provided example files as fixtures
- Unit tests verify token field extraction and arithmetic (assert totals match)
- Unit tests verify error handling for malformed input
- Integration tests verify end-to-end: read file → parse → extract final token count
- All tests verify BEHAVIOR (can we get correct token counts?) NOT implementation details (did we iterate correctly?)

**Implementation Details:**
- Add new `session_parser` module to `codex-acp` crate (NOT `codex-core` - that's deprecated)
- Reuse existing `TokenUsage` and `TokenUsageInfo` structs from `codex-protocol` 
- Create `TokenUsageReport` wrapper to provide `Option<TokenUsage>` return type
- Use streaming JSONL parsers (`serde_json::Deserializer::from_reader`) for Codex and Claude to handle large files
- Use full JSON parsing (`serde_json::from_reader`) for Gemini since it's a single object
- Focus on `last_token_usage` for Codex (per-turn final count, not cumulative `total_token_usage`)
- Gemini parser must sum tokens across all messages (session total is not pre-computed)
- Claude parser extracts from last assistant message usage object
- Token aggregation uses checked arithmetic to avoid overflow
- All parsers return `Result<Option<TokenUsageReport>, SessionParseError>`:
  - `Ok(Some(...))` = success with token data
  - `Ok(None)` = valid file but no token usage found
  - `Err(...)` = parse error or IO error
- Assert that token totals are internally consistent (e.g., `total = input + output`)

---
