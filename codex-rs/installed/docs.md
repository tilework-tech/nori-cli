# Noridoc: installed

Path: @/codex-rs/installed

### Overview

- Tracks CLI lifecycle events (first install, version upgrades, sessions) via a persistent state file
- Sends analytics events to the Tilework backend for usage insights
- Generates privacy-protecting user identifiers based on hashed hostname:username

### How it fits into the larger codebase

- **Called from** `@/codex-rs/tui/src/lib.rs` via `track_launch()` at TUI startup
- **State persistence**: Writes to `$NORI_HOME/.nori-install.json` (where `NORI_HOME` is typically `~/.nori/cli`)
- **Analytics endpoint**: Sends events to `https://demo.tilework.tech/api/analytics/track` (configurable via `NORI_ANALYTICS_URL` env var)
- **Install source detection**: Reads `NORI_MANAGED_BY_BUN` or `NORI_MANAGED_BY_NPM` environment variables set by the nori.js wrapper

```
┌─────────────┐     track_launch()     ┌─────────────────┐
│ TUI startup │ ──────────────────────▶│ nori-installed  │
└─────────────┘                        └────────┬────────┘
                                                │
        ┌───────────────────────────────────────┴───────────────────────────────┐
        ▼                                                                       ▼
┌───────────────────┐                                               ┌───────────────────────┐
│ .nori-install.json│◀── read/write                                 │ Analytics endpoint    │
│ (state file)      │                                               │ (POST, fire-and-forget)│
└───────────────────┘                                               └───────────────────────┘
```

### Core Implementation

**Entry Point (`lib.rs`):**

`track_launch(nori_home: &Path)` spawns a background tokio task that:
1. Reads existing state from `.nori-install.json` (treats missing/corrupt as first install)
2. Determines event type: `FirstInstall`, `Upgrade`, or `Session`
3. Updates state and writes atomically (temp file + rename)
4. Sends analytics event (no-op in debug builds)

**State Structure (`state.rs`):**

| Field | Description |
|-------|-------------|
| `schema_version` | Forward-compatible versioning (currently 1) |
| `client_id` | Always "nori-cli" |
| `user_id` | Privacy hash: `sha256:<hex>` of `hostname:username` |
| `first_installed_at` | Immutable timestamp of first install |
| `last_updated_at` | When version last changed |
| `last_launched_at` | Most recent launch time |
| `installed_version` | Current CLI version |
| `install_source` | `npm`, `bun`, or `unknown` |

**Analytics Events (`analytics.rs`):**

Two event types with standardized `tilework_cli_` prefixed parameters:

| Event | When Sent | Parameters |
|-------|-----------|------------|
| `plugin_install_completed` | First install or upgrade | `tilework_user_id`, `tilework_cli_installed_version`, `tilework_cli_install_source`, `tilework_cli_is_first_install`, `tilework_cli_days_since_install`, `tilework_cli_previous_version` (upgrade only) |
| `nori_session_started` | Every launch (not first/upgrade) | `tilework_user_id`, `tilework_cli_installed_version`, `tilework_cli_install_source`, `tilework_cli_days_since_install` |

**Detection (`detection.rs`):**

- `detect_install_source()`: Checks `NORI_MANAGED_BY_BUN=1` then `NORI_MANAGED_BY_NPM=1`
- `generate_user_id()`: SHA256 hash of `{hostname}:{username}` for privacy

### Things to Know

- **Debug builds skip analytics**: `send_event()` is a no-op when `debug_assertions` is enabled, preventing noise during development and E2E testing
- **Atomic writes**: State file uses temp file + rename to prevent partial writes on crash
- **User ID stability**: Once generated, the `user_id` is persisted in the state file and reused across sessions
- **Version comparison**: Upgrade detection uses simple string equality on `installed_version` vs `CLI_VERSION` constant

Created and maintained by Nori.
