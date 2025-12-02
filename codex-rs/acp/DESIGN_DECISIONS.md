# ACP Integration: Critical Design Decisions

## Core Principle: Minimal Footprint

The ACP implementation is designed to **minimize changes to codex-core** to ease the burden of accepting upstream changes. This drives all other decisions.

---

## Architecture Decisions

### 1. Parallel Crate Structure (Not Trait Abstraction)

**Decision:** `codex-acp` is a parallel crate to `codex-core`, NOT integrated via a shared trait.

**Rejected Alternative:** An `AgentBackend` trait that would unify HTTP and ACP:
```rust
// REJECTED - pollutes core with ACP concerns
trait AgentBackend {
    fn owns_tool_execution(&self) -> bool;      // ACP-specific branching
    fn supports_session_resume(&self) -> bool;  // ACP capability check
    fn permission_requests(&self) -> Receiver;  // ACP protocol detail
}
```

**Why:** Every method except `execute_turn()` would exist to handle ACP's differences, causing:
- Core logic branches on `owns_tool_execution()`
- Testing must cover both paths
- Upstream changes risk breaking ACP paths

**Result:** Zero changes to codex-core. ACP is self-contained.

---

### 2. Mode Selection: Config-Only at Startup

**Decision:** ACP vs HTTP mode is determined at startup via configuration. No mid-session switching.

**Implications:**
- Model picker stays HTTP-only
- No changes to `Op::OverrideTurnContext`
- TUI branches once at startup, not per-turn

**How core calling code changes:**
```rust
// In TUI/CLI main():
if config.acp_agent.is_some() {
    // ACP mode: use codex-acp directly
    let connection = AcpConnection::spawn(config).await;
    run_acp_event_loop(connection);
} else {
    // HTTP mode: use codex-core (unchanged)
    let codex = Codex::spawn(config);
    run_event_loop(codex.events());
}
```

---

### 3. History Ownership: ACP Owns It

**Decision:** ACP agents fully manage their own history. Codex does not persist or convert ACP conversations.

**Implications:**
- Zero history conversion code in core
- If user switches backends, history doesn't transfer (by design)
- ACP agents may have proprietary context handling

**Future Placeholders Added:**
- `connection.rs:343-350` - Resume/fork integration point
- `connection.rs:385-394` - Codex-format history persistence
- `connection.rs:220-234` - History export for backend handoff

---

### 4. Approval Bridging: Codex Intercedes via Event Translation

**Decision:** ACP permission requests flow through Codex's approval UI via channel-based event translation.

**Flow:**
```
ACP Worker Thread                    Main Thread (TUI)
       │                                    │
       │  ApprovalRequest                   │
       │ ──────────────────────────────────►│
       │  (ExecApprovalRequestEvent +       │ Display approval UI
       │   options + oneshot::Sender)       │ Get user decision
       │                                    │
       │  ReviewDecision                    │
       │ ◄──────────────────────────────────│
       │  (via oneshot channel)             │
       │                                    │
       ▼                                    ▼
   Translate to ACP outcome          Continue processing
```

**Key Types:**
```rust
pub struct ApprovalRequest {
    pub event: ExecApprovalRequestEvent,  // Codex format
    pub options: Vec<acp::PermissionOption>,  // For response translation
    pub response_tx: oneshot::Sender<ReviewDecision>,
}
```

**How TUI integrates:**
```rust
let mut connection = AcpConnection::spawn(config).await?;
let approval_rx = connection.take_approval_receiver();

// In event loop:
tokio::select! {
    Some(approval) = approval_rx.recv() => {
        // Show approval UI
        let decision = show_approval_popup(approval.event).await;
        // Send decision back
        let _ = approval.response_tx.send(decision);
    }
    // ... other events
}
```

---

### 5. Translation Layer: Bi-directional Type Conversion

**Decision:** `translator.rs` handles all ACP ↔ Codex type conversions.

**Functions:**
| Function | Direction |
|----------|-----------|
| `permission_request_to_approval_event()` | ACP → Codex |
| `review_decision_to_permission_outcome()` | Codex → ACP |
| `response_items_to_content_blocks()` | Codex → ACP |
| `translate_session_update()` | ACP → Codex |

**Approval Translation Logic:**
- `Approved`/`ApprovedForSession` → Find `AllowOnce`/`AllowAlways` option
- `Denied`/`Abort` → Find `RejectOnce`/`RejectAlways` option
- Fallback: text matching ("allow", "approve", "yes" vs "deny", "reject", "no")
- Last resort: first option for approve, last for deny

---

## How Core Module Calling Code Must Change

### Current State (HTTP-only)
```rust
// codex-tui/src/main.rs (simplified)
let codex = Codex::spawn(config);
let events = codex.events();
run_event_loop(events);
```

### Required Changes

1. **Add ACP config detection:**
```rust
// Add to config parsing
struct Config {
    // existing fields...
    acp_agent: Option<AcpAgentConfig>,  // NEW
}
```

2. **Branch at startup:**
```rust
async fn main() {
    let config = load_config();

    if let Some(acp_config) = &config.acp_agent {
        run_acp_mode(acp_config, &config).await;
    } else {
        run_http_mode(&config).await;  // Existing code path
    }
}
```

3. **ACP mode implementation:**
```rust
async fn run_acp_mode(acp_config: &AcpAgentConfig, config: &Config) {
    // Spawn ACP connection
    let mut connection = AcpConnection::spawn(acp_config, &config.cwd).await?;

    // Take approval receiver for UI integration
    let approval_rx = connection.take_approval_receiver();

    // Create session
    let session_id = connection.create_session(&config.cwd).await?;

    // Event loop with approval handling
    loop {
        tokio::select! {
            Some(approval) = approval_rx.recv() => {
                handle_approval_request(approval);
            }
            user_input = get_user_input() => {
                let (update_tx, mut update_rx) = mpsc::channel(32);
                connection.prompt(&session_id, prompt, update_tx).await?;

                while let Some(update) = update_rx.recv().await {
                    let events = translator::translate_session_update(update);
                    for event in events {
                        display_event(event);
                    }
                }
            }
        }
    }
}
```

4. **HTTP mode unchanged:**
```rust
async fn run_http_mode(config: &Config) {
    // Existing codex-core code path - UNCHANGED
    let codex = Codex::spawn(config);
    run_event_loop(codex.events());
}
```

---

## Files Modified/Created

| File | Change |
|------|--------|
| `codex-rs/acp/src/connection.rs` | Added `ApprovalRequest`, approval channel, placeholders |
| `codex-rs/acp/src/translator.rs` | Added approval translation functions |
| `codex-rs/acp/src/lib.rs` | Exported `ApprovalRequest` |
| `codex-rs/acp/docs.md` | Updated documentation |

## Files NOT Modified

| File | Why |
|------|-----|
| `codex-rs/core/*` | **Zero changes** - minimal footprint principle |
| `codex-rs/tui/*` | TUI integration is separate task |

---

## Summary

The ACP integration follows a **parallel architecture** where:
- `codex-core` remains unchanged (upstream compatibility)
- `codex-acp` is self-contained with its own session management
- TUI/CLI chooses mode at startup based on config
- Approval bridging is the single integration point, using channel-based event translation
- History/resume features are stubbed with clear placeholder comments for future work
