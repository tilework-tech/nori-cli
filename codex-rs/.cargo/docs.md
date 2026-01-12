# Noridoc: Cargo Workspace Configuration

Path: @/codex-rs/.cargo

### Overview

- Contains workspace-level Cargo configuration for all Rust builds
- Enforces sccache compilation caching and mold linker for build optimization
- Applies to all crates within @/codex-rs workspace and git worktrees

### How it fits into the larger codebase

- `config.toml` is automatically read by Cargo for all builds in the workspace
- Impacts CI/CD workflows in @/.github/workflows which must install sccache
- Affects local development by requiring sccache installation for builds to succeed
- Enables efficient multi-worktree workflows by sharing compilation cache across all worktrees

### Core Implementation

**config.toml Structure:**

1. **Platform-specific Linkers**
   - Linux: Uses `clang` with `mold` linker for 2-5x faster linking
   - Windows MSVC: Sets stack size to 8MB (`/STACK:8388608`)
   - Windows GNU: Sets stack size to 8MB (`--stack,8388608`)

2. **Build-time Compilation Cache (sccache)**
   ```toml
   [build]
   rustc-wrapper = "sccache"
   rustflags = ["--remap-path-prefix", "$CARGO_MANIFEST_DIR=."]
   ```
   - `rustc-wrapper = "sccache"` - **MANDATORY**: All rustc invocations go through sccache
   - Path remapping ensures cache hits across worktrees with different absolute paths
   - Added in commit `7cbd16f5c` on January 8, 2026

3. **Test Parallelism**
   ```toml
   [env]
   RUST_TEST_THREADS = "4"
   ```
   - Limits test threads to 4 to prevent CPU exhaustion on 8-core systems
   - Prevents system from becoming unusable during test suite runs

**Why sccache is Mandatory:**
When Cargo sees `rustc-wrapper = "sccache"` in config.toml, it expects sccache to be available in PATH. If sccache is not installed, Cargo fails with:
```
error: could not execute process `sccache /path/to/rustc -vV` (never executed)
Caused by: No such file or directory (os error 2)
```

**sccache Cache Location:**
- Default: `~/.cache/sccache/` on Linux, `~/Library/Caches/sccache/` on macOS
- Can be overridden with `SCCACHE_DIR` environment variable
- GitHub Actions workflows use workspace-local cache for actions/cache integration

### Things to Know

- **Local Development Setup**: Developers must install sccache before building: `cargo install sccache` or `brew install sccache`
- **CI/CD Requirement**: All GitHub Actions workflows that build Rust code must install sccache (version 0.7.5) before running cargo commands
- **Cache Effectiveness**: Path remapping (`--remap-path-prefix`) is critical for cache hits across worktrees - without it, each worktree would have separate cache entries
- **Mold Linker**: Only applies on Linux; macOS and Windows use default linkers. Mold must be installed separately (`apt install mold` or via nix flake)
- **Stack Size**: Windows stack size is set to 8MB to prevent stack overflows in deeply nested async code
- **audit.toml**: Contains `cargo-deny` configuration for dependency auditing (separate from build config)
- **No Per-Crate Override**: These settings apply workspace-wide; individual crates cannot opt out without modifying this file

Created and maintained by Nori.
