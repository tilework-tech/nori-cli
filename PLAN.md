# Remove Legacy Crates - Phase 2: Clean Up Dead Code

**Goal:** Complete the legacy code removal by deleting feature-gated dead code that remains after Phase 1 crate deletions.

**Architecture:** Phase 1 (already done in working tree) deleted the legacy crate directories and updated Cargo.toml files. However, source files still contain `#[cfg(feature = "...")]` guards for features that no longer exist. Since these features are never enabled, the guarded code is dead. We need to:
1. Keep code guarded by `#[cfg(not(feature = "..."))]` (Nori replacements)
2. Delete code guarded by `#[cfg(feature = "...")]` (unused Codex code)
3. Remove the cfg guards from code that should always run
4. Delete files that are entirely gated behind unused features
5. Clean up the `unexpected_cfgs` lint workaround
6. Update any stale documentation references

**Tech Stack:** Rust, conditional compilation

---

## Testing Plan

Since we are removing dead code (code that never compiles), existing tests serve as verification. The key tests are:

1. `cargo check -p codex-tui` - Verify TUI compiles
2. `cargo check -p codex-cli` - Verify CLI compiles  
3. `cargo test -p codex-tui` - Verify all TUI tests pass
4. `cargo test -p codex-cli` - Verify CLI tests pass
5. `cargo test -p tui-pty-e2e` - Verify E2E tests pass

NOTE: I will run tests after each phase to catch regressions early. No new tests are needed because we're removing code, not adding behavior.

---

## Phase 1: Remove `feedback` Feature Dead Code

The `feedback` feature gates Sentry-based log upload. Nori uses GitHub Discussions instead.

### Step 1.1: Delete feedback_view.rs and its snapshots
**Files to delete:**
- `codex-rs/tui/src/bottom_pane/feedback_view.rs`
- `codex-rs/tui/src/bottom_pane/snapshots/codex_tui__bottom_pane__feedback_view__tests__feedback_view_bad_result.snap`
- `codex-rs/tui/src/bottom_pane/snapshots/codex_tui__bottom_pane__feedback_view__tests__feedback_view_good_result.snap`
- `codex-rs/tui/src/bottom_pane/snapshots/codex_tui__bottom_pane__feedback_view__tests__feedback_view_bug.snap`
- `codex-rs/tui/src/bottom_pane/snapshots/codex_tui__bottom_pane__feedback_view__tests__feedback_view_other.snap`

### Step 1.2: Clean up bottom_pane/mod.rs
**File:** `codex-rs/tui/src/bottom_pane/mod.rs`
- Remove lines 31-35 (gated imports for feedback_view)
- Remove line 43 (`#[cfg(feature = "feedback")]` pub use)

### Step 1.3: Delete feedback_compat.rs
**File:** `codex-rs/tui/src/feedback_compat.rs`
- Delete entire file (contains stub implementation for disabled feedback)

### Step 1.4: Clean up slash_command.rs
**File:** `codex-rs/tui/src/slash_command.rs`
- Line 97: Remove `#[cfg(not(feature = "feedback"))]` guard (feedback is always disabled)
- Line 141: Remove `#[cfg(not(feature = "feedback"))]` guard
- Line 154: Remove `#[cfg(feature = "feedback")]` and the following SlashCommand::Feedback variant

### Step 1.5: Clean up app_event.rs
**File:** `codex-rs/tui/src/app_event.rs`
- Lines 181-188: Delete the two `#[cfg(feature = "feedback")]` gated enum variants (OpenFeedbackConsent, OpenFeedbackNote)

### Step 1.6: Clean up app_backtrack.rs
**File:** `codex-rs/tui/src/app_backtrack.rs`
- Line 349: Delete `#[cfg(feature = "feedback")]` gated match arm

### Step 1.7: Clean up chatwidget.rs feedback references
**File:** `codex-rs/tui/src/chatwidget.rs`
- Lines 324, 391, 505, 529: Remove feedback-related fields/imports gated by `#[cfg(feature = "feedback")]`
- Lines 1492, 1549, 1582, 1641, 1773: Remove feedback handling code gated by `#[cfg(feature = "feedback")]`
- Line 1781: Remove `#[cfg(not(feature = "feedback"))]` guard (always active)

### Step 1.8: Clean up chatwidget/tests.rs
**File:** `codex-rs/tui/src/chatwidget/tests.rs`
- Lines 317, 382: Remove `#[cfg(feature = "feedback")]` from test helper fields
- Lines 1765, 1777: Delete entire test functions gated by `#[cfg(feature = "feedback")]`

### Step 1.9: Clean up app.rs
**File:** `codex-rs/tui/src/app.rs`
- Lines 222, 257, 302, 327, 360, 498, 686, 693, 1034, 1299, 1338: Remove all `#[cfg(feature = "feedback")]` guards and the code they protect

### Step 1.10: Clean up lib.rs feedback references
**File:** `codex-rs/tui/src/lib.rs`
- Lines 397-502: Remove feedback-related code gated by `#[cfg(feature = "feedback")]`
- Remove `#[cfg(not(feature = "feedback"))]` guards (always active)

### Step 1.11: Clean up nori/mod.rs
**File:** `codex-rs/tui/src/nori/mod.rs`
- Line 14: Remove `#[cfg(not(feature = "feedback"))]` guard - feedback module is always used

### Step 1.12: Verification
```bash
cd codex-rs && cargo check -p codex-tui
cd codex-rs && cargo test -p codex-tui
```

---

## Phase 2: Remove `codex-features` Dead Code

The `codex-features` flag gates Codex-specific CLI functionality not needed for Nori.

### Step 2.1: Delete status/card.rs, status/account.rs, status/rate_limits.rs
**Files to delete:**
- `codex-rs/tui/src/status/card.rs`
- `codex-rs/tui/src/status/account.rs`
- `codex-rs/tui/src/status/rate_limits.rs`
- Related snapshot files in `codex-rs/tui/src/status/snapshots/`

### Step 2.2: Clean up status/mod.rs
**File:** `codex-rs/tui/src/status/mod.rs`
- Remove lines 3-12 (`#[cfg(feature = "codex-features")]` imports)
- Remove line 24 (`#[cfg(feature = "codex-features")]` pub use)
- Keep: format.rs, helpers.rs, tests.rs (shared utilities)

### Step 2.3: Clean up cli.rs
**File:** `codex-rs/tui/src/cli.rs`
- Lines 3, 42, 48, 58, 63, 68, 98: Remove all `#[cfg(feature = "codex-features")]` guards and the code they protect

### Step 2.4: Clean up lib.rs codex-features references
**File:** `codex-rs/tui/src/lib.rs`
- Lines 10, 12, 22, 24, 74, 153-154, 184, 214, 247, 258, 287, 337, 339, 409: Remove `#[cfg(feature = "codex-features")]` guards and code
- Keep code guarded by `#[cfg(not(feature = "codex-features"))]` (remove just the guard)

### Step 2.5: Clean up slash_command.rs codex-features
**File:** `codex-rs/tui/src/slash_command.rs`
- Line 99: Remove `#[cfg(not(feature = "codex-features"))]` guard

### Step 2.6: Verification
```bash
cd codex-rs && cargo check -p codex-tui
```

---

## Phase 3: Remove `upstream-updates` Dead Code

The `upstream-updates` flag gates OpenAI update checking. Nori uses GitHub releases.

### Step 3.1: Delete upstream update files
**Files to delete:**
- `codex-rs/tui/src/update_action.rs` (replaced by nori/update_action.rs)
- `codex-rs/tui/src/update_prompt.rs` (replaced by nori/update_prompt.rs)
- `codex-rs/tui/src/updates.rs` (replaced by nori/updates.rs)

### Step 3.2: Clean up lib.rs upstream-updates
**File:** `codex-rs/tui/src/lib.rs`
- Line 103: Remove `#[cfg(feature = "upstream-updates")]` and code
- Line 112: Remove `#[cfg(not(feature = "upstream-updates"))]` guard

### Step 3.3: Clean up nori/mod.rs
**File:** `codex-rs/tui/src/nori/mod.rs`
- Line 19: Remove `#[cfg(not(feature = "upstream-updates"))]` guard (always active)
- Lines 21-24: Update debug_assertions guards if needed

### Step 3.4: Verification
```bash
cd codex-rs && cargo check -p codex-tui
```

---

## Phase 4: Remove `backend-client` Dead Code

The `backend-client` flag gates HTTP backend client for rate limit prefetching.

### Step 4.1: Clean up chatwidget.rs
**File:** `codex-rs/tui/src/chatwidget.rs`
- Line 11: Remove `#[cfg(feature = "backend-client")]` use statement
- Line 2405: Remove `#[cfg(feature = "backend-client")]` and the prefetch field
- Line 2433: Remove `#[cfg(not(feature = "backend-client"))]` guard
- Line 4182: Remove `#[cfg(feature = "backend-client")]` and gated code

### Step 4.2: Verification
```bash
cd codex-rs && cargo check -p codex-tui
```

---

## Phase 5: Remove `oss-providers` Dead Code

The `oss-providers` flag gates Ollama/LM Studio local model support.

### Step 5.1: Clean up common/src/oss.rs
**File:** `codex-rs/common/src/oss.rs`
- Lines 14, 34, 80, 87: Remove `#[cfg(feature = "oss-providers")]` and the gated code
- Lines 26, 60, 101, 115: Remove `#[cfg(not(feature = "oss-providers"))]` guards (keep the code)

### Step 5.2: Verification
```bash
cd codex-rs && cargo check -p codex-common
```

---

## Phase 6: Remove Workspace Lint Workaround

### Step 6.1: Remove unexpected_cfgs lint suppression
**File:** `codex-rs/Cargo.toml`
- Lines 212-219: Delete the `[workspace.lints.rust]` section with `unexpected_cfgs`

### Step 6.2: Verification
```bash
cd codex-rs && cargo check -p codex-tui 2>&1 | grep -i "unexpected_cfgs"
```
Should return nothing (no warnings about unknown features).

---

## Phase 7: Update Documentation

### Step 7.1: Update apply-patch/docs.md
**File:** `codex-rs/apply-patch/docs.md`
- Line 15: Remove reference to deleted `codex-chatgpt` crate

### Step 7.2: Update nori/docs.md
**File:** `codex-rs/tui/src/nori/docs.md`
- Update any references to removed features/files

### Step 7.3: Update workspace docs.md
**File:** `codex-rs/docs.md`
- Remove any references to deleted crates

---

## Phase 8: Final Verification

### Step 8.1: Run formatter
```bash
cd codex-rs && just fmt
```

### Step 8.2: Run linter
```bash
cd codex-rs && just fix -p codex-cli && just fix -p codex-tui && just fix -p codex-common
```

### Step 8.3: Run TUI tests
```bash
cd codex-rs && cargo test -p codex-tui
```

### Step 8.4: Run CLI tests
```bash
cd codex-rs && cargo test -p codex-cli
```

### Step 8.5: Run E2E tests
```bash
cd codex-rs && cargo test -p tui-pty-e2e
```

### Step 8.6: Verify clean release build
```bash
cd codex-rs && cargo build --release -p codex-cli
```

---

## Testing Details

- Existing test suite (665+ TUI tests) verifies behavior is preserved
- E2E tests verify end-to-end user flows work
- All tests verify BEHAVIOR, not implementation details
- No new tests needed since we're only removing code

## Implementation Details

- ~40 `#[cfg(feature = "feedback")]` guards and associated code to remove
- ~25 `#[cfg(feature = "codex-features")]` guards and associated code to remove  
- ~14 `#[cfg(feature = "upstream-updates")]` guards and associated code to remove
- ~3 `#[cfg(feature = "backend-client")]` guards and associated code to remove
- ~4 `#[cfg(feature = "oss-providers")]` guards and associated code to remove
- 5 files to delete entirely (feedback_view.rs, card.rs, account.rs, rate_limits.rs, feedback_compat.rs)
- 3 upstream update files to delete (update_action.rs, update_prompt.rs, updates.rs)
- The lint suppression in Cargo.toml can be removed once all dead code is cleaned

## Questions

1. **FeedbackCategory enum**: Used in app_event.rs - should this be kept for Nori's GitHub-based feedback or completely removed?

2. **Update checking**: The nori/updates.rs has its own update checking via GitHub releases. Is this working correctly?

---
