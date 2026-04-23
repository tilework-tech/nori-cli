# Noridoc: codex-async-utils

Path: @/nori-rs/async-utils

### Overview

The async-utils crate provides async utilities for Tokio. Currently it contains the `OrCancelExt` trait for cancellable futures.

### How it fits into the larger codebase

Used throughout the workspace where async operations need cancellation support, particularly in `@/nori-rs/core/` and `@/nori-rs/acp/`.

### Core Implementation

**OrCancelExt Trait**: Extension trait for futures that adds cancellation support:

```rust
async fn long_operation().or_cancel(&token).await
```

Returns `Ok(result)` if the future completes, or `Err(CancelErr::Cancelled)` if the cancellation token is triggered first.

Uses `tokio::select!` internally to race between the future and the cancellation signal.

### Things to Know

- Works with any `Future + Send` where `Output: Send`
- Returns immediately if token is already cancelled
- Does not abort the underlying future - it simply stops waiting

Created and maintained by Nori.
