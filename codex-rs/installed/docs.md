# Noridoc: installed

Path: @/codex-rs/installed

### Overview

- Tracks CLI lifecycle events (first install, version upgrades, sessions, resurrection) via a persistent state file
- Sends analytics events to the shared Nori analytics proxy for usage insights
- Generates deterministic client IDs from hashed hostname + username for churn tracking

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
2. Determines lifecycle events: `app_install`, `app_update`, `user_resurrected`, and `session_start`
3. Updates state and writes atomically (temp file + rename)
4. Sends analytics events unless opted out (`NORI_NO_ANALYTICS=1` or `opt_out: true`)

**State Structure (`state.rs`):**

| Field | Description |
|-------|-------------|
| `schema_version` | Forward-compatible versioning (currently 1) |
| `client_id` | Deterministic UUID from `SHA256("nori_salt:<hostname>:<username>")` |
| `opt_out` | Boolean flag to disable analytics |
| `first_installed_at` | Immutable timestamp of first install |
| `last_updated_at` | When version last changed |
| `last_launched_at` | Most recent launch time |
| `installed_version` | Current CLI version |
| `install_source` | `npm`, `bun`, or `unknown` |

**Analytics Events (`analytics.rs`):**

Flat JSON payloads with `event`, `client_id`, `session_id`, `timestamp`, and a shared `properties` object:

| Event | When Sent | Notes |
|-------|-----------|-------|
| `app_install` | First install | Created when state file is missing |
| `app_update` | Version upgrade | Semver comparison against `installed_version` |
| `user_resurrected` | Returning after 30+ days | Emitted before `session_start` |
| `session_start` | Every launch | Session-scoped UUID |

**Detection (`detection.rs`):**

- `detect_install_source()`: Checks `NORI_MANAGED_BY_BUN=1` then `NORI_MANAGED_BY_NPM=1`
- `generate_client_id()`: SHA256 hash of `nori_salt:{hostname}:{username}` formatted as UUID

### Things to Know

- **Opt-out honored**: `NORI_NO_ANALYTICS=1` or `opt_out: true` disables network requests but keeps state updated
- **Atomic writes**: State file uses temp file + rename to prevent partial writes on crash
- **Client ID stability**: Once generated, the `client_id` is persisted in the state file and reused across sessions
- **Version comparison**: Upgrade detection uses semver ordering on `installed_version` vs `CLI_VERSION`

Created and maintained by Nori.
