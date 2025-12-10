# Feature Flag Gating for Sentry and OSS Providers - Implementation Plan

**Goal:** Add feature flags to gate Sentry (codex-feedback) and OSS providers (codex-ollama, codex-lmstudio) so they can be excluded from minimal builds, reducing binary size while preserving the UI structure for future Nori feedback implementation.

**Architecture:** Create optional dependency features with stub module fallbacks. When `feedback` feature is disabled, provide a compatibility layer with no-op implementations that maintain API compatibility. When `oss-providers` feature is disabled, provide stub functions that return appropriate error/empty results.

**Tech Stack:** Rust conditional compilation (`#[cfg(feature = "...")]`), Cargo optional dependencies (`optional = true`, `dep:crate-name` syntax)

---

## Testing Plan

I will add compile-time tests to verify:
1. The TUI crate compiles with `--no-default-features` (feedback and oss-providers disabled)
2. The TUI crate compiles with `--features feedback` (feedback enabled, oss-providers disabled)
3. The TUI crate compiles with `--features oss-providers` (feedback disabled, oss-providers enabled)
4. The codex-common crate compiles with `--no-default-features` (oss-providers disabled)

I will add unit tests for:
1. `feedback_compat.rs` stub implementations return expected no-op behavior
2. `oss.rs` stub implementations return expected `None`/errors when disabled

NOTE: I will write *all* tests before I add any implementation behavior.

---

## Part 1: Gate Sentry (codex-feedback) in TUI

### Step 1.1: Create feedback_compat.rs stub module

**File:** `/home/clifford/Documents/source/nori/cli/.worktrees/feature-release-feature-flags/codex-rs/tui/src/feedback_compat.rs`

Create a compatibility layer that:
- When `feedback` feature enabled: re-exports from `codex_feedback`
- When `feedback` feature disabled: provides stub implementations

Key types to stub:
- `CodexFeedback` - struct with `new()`, `make_writer()`, `snapshot()` methods
- `CodexLogSnapshot` - struct with `thread_id` field and `upload_feedback()` method

### Step 1.2: Update TUI Cargo.toml to make codex-feedback optional

**File:** `/home/clifford/Documents/source/nori/cli/.worktrees/feature-release-feature-flags/codex-rs/tui/Cargo.toml`

Changes:
1. Change `codex-feedback = { workspace = true, optional = true }` (already done)
2. Update the `feedback` feature to use `dep:codex-feedback` syntax
3. Add `feedback` to the `full` feature bundle

### Step 1.3: Update lib.rs to use feedback_compat module

**File:** `/home/clifford/Documents/source/nori/cli/.worktrees/feature-release-feature-flags/codex-rs/tui/src/lib.rs`

Changes:
1. Add `mod feedback_compat;`
2. Replace `codex_feedback::CodexFeedback` usage with `crate::feedback_compat::CodexFeedback`
3. The existing `#[cfg(feature = "feedback")]` guards should remain

### Step 1.4: Update app.rs to use feedback_compat

**File:** `/home/clifford/Documents/source/nori/cli/.worktrees/feature-release-feature-flags/codex-rs/tui/src/app.rs`

Changes:
1. Replace `codex_feedback::CodexFeedback` with `crate::feedback_compat::CodexFeedback`
2. Keep existing `#[cfg(feature = "feedback")]` guards

### Step 1.5: Update chatwidget.rs to use feedback_compat

**File:** `/home/clifford/Documents/source/nori/cli/.worktrees/feature-release-feature-flags/codex-rs/tui/src/chatwidget.rs`

Changes:
1. Replace `codex_feedback::CodexFeedback` with `crate::feedback_compat::CodexFeedback`
2. Keep existing `#[cfg(feature = "feedback")]` guards

### Step 1.6: Update bottom_pane/feedback_view.rs to use feedback_compat

**File:** `/home/clifford/Documents/source/nori/cli/.worktrees/feature-release-feature-flags/codex-rs/tui/src/bottom_pane/feedback_view.rs`

Changes:
1. Replace `codex_feedback::CodexLogSnapshot` with `crate::feedback_compat::CodexLogSnapshot`
2. The file already has `#[cfg(feature = "feedback")]` attribute on the view test functions

### Step 1.7: Update chatwidget/tests.rs to use feedback_compat

**File:** `/home/clifford/Documents/source/nori/cli/.worktrees/feature-release-feature-flags/codex-rs/tui/src/chatwidget/tests.rs`

Changes:
1. Replace `codex_feedback::CodexFeedback` with `crate::feedback_compat::CodexFeedback`
2. Add `#[cfg(feature = "feedback")]` guards where needed

---

## Part 2: Gate OSS Providers (codex-ollama, codex-lmstudio) in codex-common

### Step 2.1: Update codex-common Cargo.toml

**File:** `/home/clifford/Documents/source/nori/cli/.worktrees/feature-release-feature-flags/codex-rs/common/Cargo.toml`

Changes:
1. Make codex-ollama and codex-lmstudio optional:
   ```toml
   codex-lmstudio = { workspace = true, optional = true }
   codex-ollama = { workspace = true, optional = true }
   ```
2. Add new feature:
   ```toml
   [features]
   default = ["oss-providers"]
   oss-providers = ["dep:codex-ollama", "dep:codex-lmstudio"]
   cli = ["clap", "serde", "toml"]
   ```

### Step 2.2: Update codex-common/src/oss.rs with conditional compilation

**File:** `/home/clifford/Documents/source/nori/cli/.worktrees/feature-release-feature-flags/codex-rs/common/src/oss.rs`

Changes:
1. Add feature guards around `codex_ollama` and `codex_lmstudio` imports
2. Provide stub implementations when `oss-providers` feature is disabled:
   - `get_default_model_for_oss_provider()` returns `None`
   - `ensure_oss_provider_ready()` returns an error

### Step 2.3: Update TUI Cargo.toml to forward oss-providers feature

**File:** `/home/clifford/Documents/source/nori/cli/.worktrees/feature-release-feature-flags/codex-rs/tui/Cargo.toml`

Changes:
1. Add `oss-providers` feature that forwards to codex-common:
   ```toml
   oss-providers = ["codex-common/oss-providers"]
   ```
2. Add `oss-providers` to the `full` feature bundle

---

## Part 3: Update CLI Cargo.toml

**File:** `/home/clifford/Documents/source/nori/cli/.worktrees/feature-release-feature-flags/codex-rs/cli/Cargo.toml`

Changes:
1. Add `feedback` and `oss-providers` features that forward to codex-tui
2. Update `full` feature to include both

---

## Verification Steps

1. Run `cargo check -p codex-tui --no-default-features` - should compile
2. Run `cargo check -p codex-tui --features feedback` - should compile
3. Run `cargo check -p codex-common --no-default-features` - should compile
4. Run `cargo test -p codex-tui` - all tests should pass
5. Run `cargo test -p codex-common` - all tests should pass
6. Run `cargo build --release -p codex-cli` - compare binary size with all features vs minimal

---

**Testing Details:**
- Compile-time tests verify the crate builds with different feature combinations
- Unit tests for stub modules ensure no-op behavior is correct
- Existing tests continue to pass with features enabled

**Implementation Details:**
- Use the same stub pattern as `codex-otel` for consistency
- Preserve all existing API surface so code compiles regardless of feature state
- Keep `#[cfg(feature = "feedback")]` guards in UI code to avoid dead code warnings
- The `feedback_compat` module provides API compatibility layer
- OSS provider stubs return `None` or errors, which matches real behavior when providers unavailable
- No changes to `codex-exec` as requested

**Questions:**
1. Should the `full` feature be the default, or should minimal be the default?
   - Current plan: `default = []` for minimal builds, `full` explicitly enables everything
2. Should we add a `nori-feedback` feature placeholder for future Nori-specific feedback UI?
   - Current plan: No, but the stub structure allows easy addition later
