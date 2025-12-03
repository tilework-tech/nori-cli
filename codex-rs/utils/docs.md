# Noridoc: utils

Path: @/codex-rs/utils

### Overview

The `utils` directory contains small, focused utility crates that provide specific functionality used across the Codex workspace. Each crate is a standalone library with minimal dependencies.

### How it fits into the larger codebase

Utils crates are workspace members imported by crates that need their functionality:

- **Core** uses git, cache, image, tokenizer, string
- **TUI** uses pty for terminal emulation
- **Various** crates use readiness for async coordination

### Crates

| Crate | Purpose |
|-------|---------|
| `git` | Git repository operations (status, diff, log) |
| `cache` | Generic caching utilities |
| `image` | Image processing and encoding |
| `json-to-toml` | JSON to TOML conversion |
| `pty` | Pseudo-terminal handling |
| `readiness` | Async readiness signaling |
| `string` | String manipulation utilities |
| `tokenizer` | Token counting for context management |

### Things to Know

**Workspace Dependencies:**

Each util is available as `codex-utils-<name>` in Cargo.toml:
```toml
codex-utils-git = { path = "utils/git" }
codex-utils-image = { path = "utils/image" }
```

**Minimal Dependencies:**

Utils are designed with minimal dependencies to avoid bloating crates that import them.

Created and maintained by Nori.
