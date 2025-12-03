# Noridoc: utils/string

Path: @/codex-rs/utils/string

### Overview

The `codex-utils-string` crate provides string manipulation utilities for byte-budget truncation while respecting UTF-8 character boundaries.

### How it fits into the larger codebase

String utils is used for safe string truncation:

- **Core** uses for output truncation within token/byte limits
- **Ensures** valid UTF-8 after truncation

### Core Implementation

**Functions:**

```rust
// Truncate to prefix within byte budget
pub fn take_bytes_at_char_boundary(s: &str, maxb: usize) -> &str

// Truncate to suffix within byte budget
pub fn take_last_bytes_at_char_boundary(s: &str, maxb: usize) -> &str
```

### Things to Know

Both functions return the original string if already within budget. They iterate through char boundaries to find the maximum valid slice.

Created and maintained by Nori.
