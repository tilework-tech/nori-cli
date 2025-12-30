# Nori CLI Analytics Event Params Standardization

**Goal:** Standardize Rust CLI analytics event parameters using `tilework_cli_` prefix for all CLI-specific fields, removing redundant fields.

**Architecture:** Modify `analytics.rs` to update the `eventParams` structure in `create_install_event` and `create_session_event` functions. The top-level `TrackEventRequest` structure remains unchanged. All changes are confined to the parameter names within the JSON payload.

**Tech Stack:** Rust, serde_json

---

## Current State

**File:** `/home/clifford/Documents/source/nori/cli/.worktrees/install-tracking/codex-rs/installed/src/analytics.rs`

Current `eventParams` for install events:
```json
{
  "install_type": "free",
  "install_source": "npm",
  "installed_version": "1.0.0",
  "is_first_install": true,
  "previous_version": "0.9.0"
}
```

Current `eventParams` for session events:
```json
{
  "install_type": "free",
  "installed_version": "1.0.0",
  "install_source": "npm",
  "days_since_install": 5
}
```

## Target State

**Install events** (`plugin_install_completed`):
```json
{
  "tilework_user_id": "sha256:abc123...",
  "tilework_cli_installed_version": "1.0.0",
  "tilework_cli_install_source": "npm",
  "tilework_cli_is_first_install": true,
  "tilework_cli_days_since_install": 0,
  "tilework_cli_previous_version": "0.9.0"
}
```

**Session events** (`nori_session_started`):
```json
{
  "tilework_user_id": "sha256:abc123...",
  "tilework_cli_installed_version": "1.0.0",
  "tilework_cli_install_source": "npm",
  "tilework_cli_days_since_install": 5
}
```

## Field Mapping

| Old Name | New Name | Notes |
|----------|----------|-------|
| `install_type` | _(removed)_ | Always "free", redundant |
| `installed_version` | `tilework_cli_installed_version` | Prefixed |
| _(from state.user_id)_ | `tilework_user_id` | New field |
| `install_source` | `tilework_cli_install_source` | Prefixed |
| `is_first_install` | `tilework_cli_is_first_install` | Prefixed |
| `previous_version` | `tilework_cli_previous_version` | Prefixed |
| `days_since_install` | `tilework_cli_days_since_install` | Prefixed, now in all events |

**Removed fields:**
- `install_type` - always "free", provides no value
- `non_interactive` - CLI is inherently non-interactive, redundant

---

## Testing Plan

I will update the existing unit tests in `analytics.rs` to verify the new parameter names are correctly serialized. The tests already check the `event_params` JSON structure, so they need to be updated to expect the new field names.

**Tests to modify:**
1. `test_create_first_install_event` - verify new field names, removal of `install_type`
2. `test_create_upgrade_event` - verify `tilework_cli_previous_version` is included
3. `test_create_session_event` - verify all `tilework_cli_*` field names
4. `test_install_source_unknown` - verify `tilework_cli_install_source` field name

**New assertions to add:**
- `tilework_user_id` field is present and matches `state.user_id`
- `tilework_cli_days_since_install` is present in install events (value 0 for first install)
- All fields use `tilework_cli_` prefix
- `install_type` and `non_interactive` are NOT present

NOTE: I will write *all* tests before I add any implementation behavior.

---

## Implementation Steps

### Step 1: Update test expectations for `test_create_first_install_event`

**File:** `/home/clifford/Documents/source/nori/cli/.worktrees/install-tracking/codex-rs/installed/src/analytics.rs`

**Location:** `mod tests`, function `test_create_first_install_event` (around line 128)

**Changes:**
- Remove assertion for `params["install_type"]`
- Change `params["installed_version"]` to `params["tilework_cli_installed_version"]`
- Change `params["install_source"]` to `params["tilework_cli_install_source"]`
- Change `params["is_first_install"]` to `params["tilework_cli_is_first_install"]`
- Add assertion for `params["tilework_user_id"]` equals the test state's `user_id`
- Add assertion for `params["tilework_cli_days_since_install"]` equals `0`
- Add assertion that `params.get("install_type")` is `None`

**Run tests:** `cargo test -p nori-installed` — expect failure

---

### Step 2: Update test expectations for `test_create_upgrade_event`

**File:** Same as Step 1

**Location:** `mod tests`, function `test_create_upgrade_event` (around line 143)

**Changes:**
- Remove assertion for `params["install_type"]`
- Change `params["installed_version"]` to `params["tilework_cli_installed_version"]`
- Change `params["install_source"]` to `params["tilework_cli_install_source"]`
- Change `params["is_first_install"]` to `params["tilework_cli_is_first_install"]`
- Change `params["previous_version"]` to `params["tilework_cli_previous_version"]`
- Add assertion for `params["tilework_user_id"]`
- Add assertion for `params["tilework_cli_days_since_install"]`

**Run tests:** `cargo test -p nori-installed` — expect failure

---

### Step 3: Update test expectations for `test_create_session_event`

**File:** Same as Step 1

**Location:** `mod tests`, function `test_create_session_event` (around line 160)

**Changes:**
- Remove assertion for `params["install_type"]`
- Change `params["installed_version"]` to `params["tilework_cli_installed_version"]`
- Change `params["install_source"]` to `params["tilework_cli_install_source"]`
- Change `params["days_since_install"]` to `params["tilework_cli_days_since_install"]`
- Add assertion for `params["tilework_user_id"]`
- Add assertion that `params.get("install_type")` is `None`

**Run tests:** `cargo test -p nori-installed` — expect failure

---

### Step 4: Update test expectations for `test_install_source_unknown`

**File:** Same as Step 1

**Location:** `mod tests`, function `test_install_source_unknown` (around line 175)

**Changes:**
- Change `event.event_params["install_source"]` to `event.event_params["tilework_cli_install_source"]`

**Run tests:** `cargo test -p nori-installed` — expect failure (all 4 tests should now fail)

---

### Step 5: Update `create_install_event` function signature

**File:** `/home/clifford/Documents/source/nori/cli/.worktrees/install-tracking/codex-rs/installed/src/analytics.rs`

**Location:** Function `create_install_event` (around line 45)

The function needs an additional parameter for `days_since_install`. Update the signature:

**Current:**
```rust
pub fn create_install_event(
    state: &InstallState,
    event_type: InstallEventType,
) -> TrackEventRequest {
```

**New:**
```rust
pub fn create_install_event(
    state: &InstallState,
    event_type: InstallEventType,
    days_since_install: i64,
) -> TrackEventRequest {
```

---

### Step 6: Update `create_install_event` function body

**File:** Same as Step 5

**Current code:**
```rust
let mut params = serde_json::json!({
    "install_type": "free",
    "install_source": install_source_to_string(state.install_source),
    "installed_version": state.installed_version,
    "is_first_install": is_first_install,
});

if let Some(prev) = previous_version {
    params["previous_version"] = serde_json::Value::String(prev);
}
```

**New code:**
```rust
let mut params = serde_json::json!({
    "tilework_user_id": state.user_id,
    "tilework_cli_installed_version": state.installed_version,
    "tilework_cli_install_source": install_source_to_string(state.install_source),
    "tilework_cli_is_first_install": is_first_install,
    "tilework_cli_days_since_install": days_since_install,
});

if let Some(prev) = previous_version {
    params["tilework_cli_previous_version"] = serde_json::Value::String(prev);
}
```

---

### Step 7: Update `create_session_event` function

**File:** Same as Step 5

**Location:** Function `create_session_event` (around line 62)

**Current code:**
```rust
let params = serde_json::json!({
    "install_type": "free",
    "installed_version": state.installed_version,
    "install_source": install_source_to_string(state.install_source),
    "days_since_install": days_since_install,
});
```

**New code:**
```rust
let params = serde_json::json!({
    "tilework_user_id": state.user_id,
    "tilework_cli_installed_version": state.installed_version,
    "tilework_cli_install_source": install_source_to_string(state.install_source),
    "tilework_cli_days_since_install": days_since_install,
});
```

---

### Step 8: Update caller in `lib.rs`

**File:** `/home/clifford/Documents/source/nori/cli/.worktrees/install-tracking/codex-rs/installed/src/lib.rs`

**Location:** Function `track_launch_inner` (around line 85-95)

The call to `create_install_event` needs the new `days_since_install` parameter.

**Current code:**
```rust
let analytics_event = match &event {
    LaunchEvent::FirstInstall => {
        create_install_event(&new_state, InstallEventType::FirstInstall)
    }
    LaunchEvent::Upgrade { previous_version } => create_install_event(
        &new_state,
        InstallEventType::Upgrade {
            previous_version: previous_version.clone(),
        },
    ),
    LaunchEvent::Session { days_since_install } => {
        create_session_event(&new_state, *days_since_install)
    }
};
```

**New code:**
```rust
let days = new_state.days_since_install(now);
let analytics_event = match &event {
    LaunchEvent::FirstInstall => {
        create_install_event(&new_state, InstallEventType::FirstInstall, days)
    }
    LaunchEvent::Upgrade { previous_version } => create_install_event(
        &new_state,
        InstallEventType::Upgrade {
            previous_version: previous_version.clone(),
        },
        days,
    ),
    LaunchEvent::Session { days_since_install } => {
        create_session_event(&new_state, *days_since_install)
    }
};
```

---

### Step 9: Update test helper calls

**File:** `/home/clifford/Documents/source/nori/cli/.worktrees/install-tracking/codex-rs/installed/src/analytics.rs`

**Location:** `mod tests`

Update test functions that call `create_install_event` to pass the new `days_since_install` parameter:

- `test_create_first_install_event`: pass `0` (first install = day 0)
- `test_create_upgrade_event`: pass `0` or a test value
- `test_install_source_unknown`: pass `0`

---

### Step 10: Run full test suite

**Command:** `cargo test -p nori-installed`

**Expected:** All tests pass

---

### Step 11: Run clippy and fmt

**Commands:**
```bash
cargo fmt -p nori-installed
cargo clippy -p nori-installed
```

**Expected:** No warnings or errors

---

## Edge Cases

1. **`tilework_cli_previous_version` only on upgrades:** The field should only be present when `InstallEventType::Upgrade` is used. The existing conditional logic handles this correctly.

2. **`tilework_cli_days_since_install` is 0 on first install:** For first install events, `days_since_install` will be 0 since install just happened.

3. **Empty/null `user_id`:** The `user_id` in `InstallState` is always populated by `generate_user_id()` which returns a deterministic hash. It cannot be empty.

4. **`tilework_cli_is_first_install` not in session events:** Session events don't include this field since they only occur after initial install.

---

## Testing Details

The existing test suite covers the JSON structure of event params. Updates verify:
- All CLI-specific fields use `tilework_cli_` prefix
- `tilework_user_id` is present in all events
- `tilework_cli_days_since_install` is present in all events (install and session)
- Conditional fields (`tilework_cli_previous_version`) only appear when appropriate
- Removed fields (`install_type`, `non_interactive`) are NOT present
- The actual serialized JSON matches expected format

## Implementation Details

- `analytics.rs` needs modification (tests + 2 functions)
- `lib.rs` needs minor update to pass `days_since_install` to `create_install_event`
- `create_install_event` signature changes to add `days_since_install: i64` parameter
- No changes to `TrackEventRequest` struct or its serde attributes
- No changes to `InstallState` or `detection.rs`
- Removed `install_type` (always "free", redundant)
- Removed `non_interactive` (CLI is inherently non-interactive)

## Questions

None - all decisions have been made.

---
