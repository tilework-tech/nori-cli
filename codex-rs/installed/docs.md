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
2. Generates a session ID from current Unix timestamp in seconds
3. Determines event type: `noricli_install_detected`, `noricli_user_resurrected`, `noricli_session_started`
4. Updates state and writes atomically (temp file + rename)
5. Sends analytics events with a 5-second timeout (fire-and-forget, release builds only)

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

Three event types sent via `TrackEventRequest` (snake_case JSON fields: `client_id`, `user_id`, `event_name`, `event_params`):

| Event | When Sent |
|-------|-----------|
| `noricli_install_detected` | First install or upgrade/downgrade |
| `noricli_user_resurrected` | Launch after 30+ days of inactivity |
| `noricli_session_started` | Every launch |

**`event_params` structure:**

| Field | Description |
|-------|-------------|
| `tilework_source` | Always `"nori-cli"` (identifies client application) |
| `tilework_session_id` | Unix timestamp in seconds when session started |
| `tilework_timestamp` | ISO 8601 timestamp with millisecond precision |
| `tilework_cli_*` | CLI-specific fields (version, install source, days since install, platform, executable name) |

For `noricli_install_detected` events, additional fields: `tilework_cli_is_first_install` (boolean), `tilework_cli_previous_version` (for upgrades/downgrades).

**Detection (`detection.rs`):**

- `detect_install_source()`: Checks `NORI_MANAGED_BY_BUN=1` then `NORI_MANAGED_BY_NPM=1`
- `generate_client_id()`: Deterministic UUID from `SHA256("nori_salt:<hostname>:<username>")`

### Things to Know

- **Opt-out precedence**: `NORI_NO_ANALYTICS=1` overrides the local `opt_out` flag; CI environments (`CI=true`) also skip analytics
- **Debug builds**: Analytics sending is a no-op in debug builds to avoid noise during development and testing
- **Atomic writes**: State file uses temp file + rename to prevent partial writes on crash
- **Client ID stability**: Once generated, the `client_id` is persisted in the state file and reused across sessions
- **Version changes**: Both upgrades and downgrades emit `noricli_install_detected` events; simple string inequality comparison is used

Created and maintained by Nori.
