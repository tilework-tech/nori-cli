# Noridoc: installed

Path: @/codex-rs/installed

### Overview

- Tracks CLI lifecycle events (first install, version upgrades, sessions) via a persistent state file
- Sends analytics events to the Nori analytics proxy for usage insights
- Generates privacy-protecting client identifiers derived from a salted hostname:username hash

### How it fits into the larger codebase

- **Called from** `@/codex-rs/tui/src/lib.rs` via `track_launch()` at TUI startup
- **State persistence**: Writes to `$NORI_HOME/.nori-install.json` (where `NORI_HOME` is typically `~/.nori/cli`)
- **Analytics endpoint**: Sends events to `https://noriskillsets.dev/api/analytics/track` (configurable via `NORI_ANALYTICS_URL` env var)
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
2. Determines event type: `app_install`, `app_update`, `user_resurrected`, `session_start`
3. Updates state and writes atomically (temp file + rename)
4. Sends analytics events with a 500ms timeout (fire-and-forget)

**State Structure (`state.rs`):**

| Field | Description |
|-------|-------------|
| `schema_version` | Forward-compatible versioning (currently 1) |
| `client_id` | Deterministic UUID derived from `SHA256("nori_salt:<hostname>:<username>")` |
| `opt_out` | Opt-out flag from config file |
| `first_installed_at` | Immutable timestamp of first install |
| `last_updated_at` | When version last changed |
| `last_launched_at` | Most recent launch time |
| `installed_version` | Current CLI version |
| `install_source` | `npm`, `bun`, or `unknown` |

**Analytics Events (`analytics.rs`):**

Four event types with a flat payload schema:

| Event | When Sent | Parameters |
|-------|-----------|------------|
| `app_install` | First install | `event`, `client_id`, `session_id`, `timestamp`, `properties` |
| `app_update` | Version upgrade | `event`, `client_id`, `session_id`, `timestamp`, `properties` |
| `user_resurrected` | Launch after 30+ days of inactivity | `event`, `client_id`, `session_id`, `timestamp`, `properties` |
| `session_start` | Every launch | `event`, `client_id`, `session_id`, `timestamp`, `properties` |

**Detection (`detection.rs`):**

- `detect_install_source()`: Checks `NORI_MANAGED_BY_BUN=1` then `NORI_MANAGED_BY_NPM=1`
- `generate_client_id()`: Deterministic UUID from `SHA256("nori_salt:<hostname>:<username>")`

### Things to Know

- **Opt-out precedence**: `NORI_NO_ANALYTICS=1` overrides the local `opt_out` flag
- **Atomic writes**: State file uses temp file + rename to prevent partial writes on crash
- **Client ID stability**: Once generated, the `client_id` is persisted in the state file and reused across sessions
- **Version comparison**: Upgrade detection uses semantic versioning comparisons with a string fallback

Created and maintained by Nori.
