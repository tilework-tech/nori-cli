# Noridoc: utils/cache

Path: @/codex-rs/utils/cache

### Overview

The `codex-utils-cache` crate provides a Tokio-aware LRU cache implementation. It wraps standard LRU caching with async-friendly locking and graceful degradation when no runtime is available.

### How it fits into the larger codebase

Cache is a foundational utility used by other utils:

- **Tokenizer** uses for model cache
- **Image** uses for processed image cache
- **Provides** `sha1_digest()` for content-based keys

### Core Implementation

**BlockingLruCache:**

```rust
let cache = BlockingLruCache::new(NonZeroUsize::new(64).unwrap());

// Get or compute
let value = cache.get_or_insert_with(key, || compute());

// Fallible computation
let value = cache.get_or_try_insert_with(key, || try_compute())?;

// Direct access
cache.insert(key, value);
let value = cache.get(&key);
cache.remove(&key);
cache.clear();
```

### Things to Know

**Runtime Detection:**

Operations check for Tokio runtime. Without a runtime, operations become no-ops and factories are called directly.

**Blocking Lock:**

Uses `tokio::task::block_in_place()` for safe blocking within async context.

**SHA-1 Digest:**

`sha1_digest(bytes)` returns `[u8; 20]` for content-based cache keys.

Created and maintained by Nori.
