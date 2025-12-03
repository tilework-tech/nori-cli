# Noridoc: utils/tokenizer

Path: @/codex-rs/utils/tokenizer

### Overview

The `codex-utils-tokenizer` crate provides token counting and encoding utilities using tiktoken-rs. It wraps tokenizer functionality with caching for performance and model-based encoding selection.

### How it fits into the larger codebase

Tokenizer is used throughout Codex for context management:

- **Core** uses for token counting in context windows
- **Context manager** uses for fitting content within limits
- **Enables** accurate token usage tracking

### Core Implementation

**Tokenizer Struct:**

```rust
pub struct Tokenizer { inner: CoreBPE }

// Create from encoding
Tokenizer::new(EncodingKind::O200kBase)

// Create for model (with fallback)
Tokenizer::for_model("gpt-4o")

// Operations
tokenizer.encode(text, with_special_tokens) -> Vec<i32>
tokenizer.count(text) -> i64
tokenizer.decode(tokens) -> String
```

**Encoding Support:**

- `O200kBase` - GPT-4o models (default)
- `Cl100kBase` - GPT-3.5/GPT-4 models

### Things to Know

**Model Caching:**

Uses `BlockingLruCache` with capacity 64 to avoid reloading tokenizers. `warm_model_cache()` pre-warms cache on startup.

**Unknown Models:**

Unknown models fall back to `O200kBase` encoding.

**Token Counting:**

`count()` returns `i64` to match Codex style preferences (signed integers for counts).

Created and maintained by Nori.
