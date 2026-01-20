# Code Review: `feat(installed): align install tracking with analytics schema`

**Branch:** `codex/implement-robust-install-and-session-tracking-27izll`
**Commit:** `49e75727 feat(installed): align install tracking with analytics schema`
**Status:** All tests pass (26/26), linting clean

---

## Summary of Changes

This PR refactors the `codex-rs/installed` crate to align with a new analytics schema:

- Changed analytics payload structure from camelCase to snake_case fields
- Replaced `user_id` (sha256 hash) with `client_id` (deterministic UUID)
- Added new event types: `AppInstall`, `AppUpdate`, `SessionStart`, `UserResurrected`
- Added resurrection detection (30+ days of inactivity)
- Removed debug build analytics skip
- Changed analytics URL from demo.tilework.tech to noriskillsets.dev

---

## CRITICAL ISSUES

### 1. Missing `install_source` in Analytics Events

**File:** `installed/src/analytics.rs:49-56`

The old analytics events included `install_source` (npm/bun/unknown) in every event. The new `EventProperties` struct does NOT include this field, meaning you lose visibility into how users install the CLI.

**Current code:**
```rust
#[derive(Debug, Clone, Serialize)]
pub struct EventProperties {
    pub version: String,
    pub os: String,
    pub arch: String,
    pub node_version: String,
    pub is_ci: bool,
}
```

**Should include:**
```rust
pub struct EventProperties {
    pub version: String,
    pub os: String,
    pub arch: String,
    pub node_version: String,
    pub is_ci: bool,
    pub install_source: String,  // ADD THIS
}
```

---

### 2. Missing `previous_version` in AppUpdate Events

**File:** `installed/src/lib.rs:146-160`

`LaunchEvent::AppUpdate { previous_version }` captures the previous version, but this information is NOT included in the analytics event - it's silently discarded.

```rust
let event_type = match event {
    LaunchEvent::AppInstall => AnalyticsEventType::AppInstall,
    LaunchEvent::AppUpdate { .. } => AnalyticsEventType::AppUpdate,  // previous_version is ignored!
    // ...
};
```

**Suggestion:** Add `previous_version: Option<String>` to `EventProperties` or create a separate payload type for updates.

---

### 3. Missing `days_since_install` in Analytics Events

The old events included `tilework_cli_days_since_install` which is useful for cohort analysis. This data is now lost.

**Suggestion:** Consider adding `days_since_install: i64` to `EventProperties`.

---

## BEHAVIORAL CHANGES

### 4. Analytics Now Sends in Debug Builds

The old code had:
```rust
#[cfg(debug_assertions)]
pub async fn send_event(event: &TrackEventRequest) {
    debug!("Analytics event skipped (debug build): {}", event.event_name);
}
```

This is now removed. All builds send analytics. This may cause noise during development/testing unless developers set `NORI_NO_ANALYTICS=1`.

**Impact:** Intentional behavioral change that should be documented.

---

### 5. Timeout Reduced from 10s to 500ms

**File:** `installed/src/analytics.rs:95`

```rust
.timeout(std::time::Duration::from_millis(500))
```

500ms is aggressive. On slow networks or when the analytics server is under load, this will silently drop events. This is probably intentional (fire-and-forget, don't block startup), but worth noting.

---

### 6. All Debug Logging Removed from `send_event`

Makes debugging analytics issues harder. Consider keeping at least error-level logging for unexpected failures.

---

### 7. Downgrade is Silently Treated as Normal Session

**File:** `installed/src/lib.rs:191-198`

```rust
fn is_semver_upgrade(current_version: &str, installed_version: &str) -> bool {
    // ...
    current > installed  // Downgrade (current < installed) returns false
}
```

If a user downgrades (e.g., from 1.2.0 to 1.1.0):
- No `AppUpdate` event is emitted
- The state file still has the OLD (higher) version
- Subsequent launches will keep thinking they need to "upgrade"

This is either intentional or a bug - needs clarification.

---

## MISSING TESTS

### 8. No Test for Migration from Old State Format

When users upgrade from the old CLI version, their state file contains:
```json
{
  "client_id": "nori-cli",
  "user_id": "sha256:abc123..."
}
```

The new code detects invalid UUIDs and regenerates `client_id`, but there's no test verifying this migration path works correctly.

**Suggested test:**
```rust
#[test]
fn test_migration_from_old_state_format() {
    let temp_home = setup_temp_home();
    let state_path = temp_home.path().join(".nori-install.json");

    // Write old format state
    let old_state_json = r#"{
        "schema_version": 1,
        "client_id": "nori-cli",
        "user_id": "sha256:abc123def456...",
        "first_installed_at": "2025-01-01T00:00:00Z",
        "last_updated_at": "2025-01-01T00:00:00Z",
        "last_launched_at": "2025-01-01T00:00:00Z",
        "installed_version": "0.5.0",
        "install_source": "npm"
    }"#;
    fs::write(&state_path, old_state_json).expect("write failed");

    // Track launch
    let events = track_launch_events(temp_home.path());

    // Verify client_id was regenerated as valid UUID
    let state = read_install_state(temp_home.path()).expect("state should exist");
    assert!(Uuid::parse_str(&state.client_id).is_ok());
    assert_ne!(state.client_id, "nori-cli");
}
```

---

### 9. No Test for Resurrection Detection

There is no test covering the resurrection logic (30+ days since last launch).

**Suggested test:**
```rust
#[test]
fn test_user_resurrection_after_30_days() {
    let temp_home = setup_temp_home();
    let now = Utc::now();

    // Create state with last_launched_at > 30 days ago
    let old_launch = now - Duration::days(31);
    let mut state = InstallState::new_first_install(
        generate_client_id(),
        CLI_VERSION.to_string(),
        InstallSource::Npm,
        old_launch,
    );
    state.last_launched_at = old_launch;

    // Write state and track launch...

    assert_eq!(
        events,
        vec![LaunchEvent::UserResurrected, LaunchEvent::SessionStart]
    );
}
```

---

### 10. No Test for `is_semver_upgrade` Edge Cases

```rust
#[test]
fn test_is_semver_upgrade_edge_cases() {
    assert!(!is_semver_upgrade("1.0.0", "1.0.0"));  // Same version
    assert!(is_semver_upgrade("1.1.0", "1.0.0"));   // Upgrade
    assert!(!is_semver_upgrade("1.0.0", "1.1.0")); // Downgrade
    assert!(is_semver_upgrade("1.0.0", "0.9"));    // Invalid semver fallback
}
```

---

## CODE STYLE ISSUES

### 11. Magic Number: 30-day Resurrection Threshold

**File:** `installed/src/lib.rs:203`

```rust
fn is_resurrected(state: &InstallState, now: chrono::DateTime<Utc>) -> bool {
    let diff = now - state.last_launched_at;
    diff > Duration::days(30)  // Magic number
}
```

**Suggestion:**
```rust
/// Number of days of inactivity before a returning user is considered "resurrected"
const RESURRECTION_THRESHOLD_DAYS: i64 = 30;

fn is_resurrected(state: &InstallState, now: chrono::DateTime<Utc>) -> bool {
    let diff = now - state.last_launched_at;
    diff > Duration::days(RESURRECTION_THRESHOLD_DAYS)
}
```

---

### 12. `Uuid::nil()` Fallback is Questionable

**File:** `installed/src/detection.rs:46-49`

```rust
let uuid = match Uuid::from_slice(&hash[..16]) {
    Ok(value) => value,
    Err(_) => Uuid::nil(),  // Returns 00000000-0000-0000-0000-000000000000
};
```

`Uuid::from_slice` with a 16-byte slice should never fail. However, if it somehow did fail, `Uuid::nil()` would make all affected users appear as the same client.

**Suggestion:** Use `expect()` since failure is impossible:
```rust
Uuid::from_slice(&hash[..16]).expect("SHA256 always produces 32 bytes")
```

---

### 13. `track_launch_sync` Returns Only Last Event

**File:** `installed/src/lib.rs:175-181`

```rust
pub fn track_launch_sync(nori_home: &Path) -> anyhow::Result<LaunchEvent> {
    let events = rt.block_on(track_launch_inner(nori_home))?;
    Ok(events.last().cloned().unwrap_or(LaunchEvent::SessionStart))
}
```

This API only returns `SessionStart` for most cases. The caller loses information about whether this was also a first install, upgrade, or resurrection.

**Suggestion:** Either return `Vec<LaunchEvent>` or document the limitation.

---

### 14. Overly Complex Test Version Logic

**File:** `installed/src/lib.rs:266-285`

The test computes an "older" version from `CLI_VERSION` with complex logic. Simpler approach:

```rust
let old_version = "0.0.1".to_string();  // Just use a version that's definitely older
```

---

## RECOMMENDATIONS SUMMARY

| Priority | Issue | File | Action |
|----------|-------|------|--------|
| **HIGH** | Missing `install_source` in events | `analytics.rs` | Add to `EventProperties` |
| **HIGH** | No test for old state migration | `lib.rs` | Add migration test |
| **MEDIUM** | `previous_version` lost in AppUpdate | `lib.rs` | Include in event properties |
| **MEDIUM** | No resurrection test | `lib.rs` | Add test case |
| **MEDIUM** | `Uuid::nil()` fallback | `detection.rs` | Use `expect()` |
| **LOW** | Magic number 30 days | `lib.rs` | Extract to constant |
| **LOW** | `track_launch_sync` loses info | `lib.rs` | Document or change return type |
| **LOW** | Complex test version logic | `lib.rs` | Simplify to hardcoded old version |

---

*Review generated on 2026-01-20*
