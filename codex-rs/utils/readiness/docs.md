# Noridoc: utils/readiness

Path: @/codex-rs/utils/readiness

### Overview

The `codex-utils-readiness` crate provides an async-aware readiness flag with token-based authorization. It enables coordination between async tasks where one task signals readiness for others to proceed.

### How it fits into the larger codebase

Readiness utils is used for async coordination:

- **Core** may use for initialization synchronization
- **Provides** once-only readiness signaling
- **Supports** multiple subscribers

### Core Implementation

**Readiness Trait:**

```rust
pub trait Readiness: Send + Sync + 'static {
    fn is_ready(&self) -> bool;
    async fn subscribe(&self) -> Result<Token, ReadinessError>;
    async fn mark_ready(&self, token: Token) -> Result<bool, ReadinessError>;
    async fn wait_ready(&self);
}
```

**ReadinessFlag:**

- `subscribe()` - Get authorization token
- `mark_ready(token)` - Signal readiness (requires valid token)
- `wait_ready()` - Async wait until ready
- `is_ready()` - Check current state

### Things to Know

**Token Authorization:**

Only subscribed tokens can mark ready. Prevents unauthorized state changes.

**Once-Only:**

Once marked ready, state is irreversible. Further subscriptions fail.

**Empty Subscribers:**

If no subscribers exist when `is_ready()` is checked, flag becomes ready automatically.

**Lock Timeout:**

Token lock has 1-second timeout to prevent deadlocks.

Created and maintained by Nori.
