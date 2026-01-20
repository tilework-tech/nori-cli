# Noridoc: installed

Path: @/codex-rs/installed

### Overview

- Tracks CLI lifecycle events (first install, version upgrades, sessions, user resurrection) via a persistent state file
- Sends analytics events to the Tilework backend using standardized flat schema
- Generates deterministic client IDs and privacy-protecting user identifiers based on hashed hostname:username
- Supports analytics opt-out via environment variable or state file
- Tracks ephemeral session IDs (per-process, not persisted)

### How it fits into the larger codebase

- **Called from** `@/codex-rs/tui/src/lib.rs` via `track_launch()` at TUI startup
- **State persistence**: Writes to `$NORI_HOME/.nori-install.json` (where `NORI_HOME` is typically `~/.nori/cli`)
- **Analytics endpoint**: Sends events to `https://noriskillsets.dev/api/analytics/track` (configurable via `NORI_ANALYTICS_URL` env var)
- **Install source detection**: Reads `NORI_MANAGED_BY_BUN` or `NORI_MANAGED_BY_NPM` environment variables set by the nori.js wrapper
- **Opt-out controls**: Respects `NORI_NO_ANALYTICS=1` environment variable (highest priority) and `opt_out` field in state file

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
| `schema_version` | Forward-compatible versioning (currently 2) |
| `client_id` | Deterministic UUID formatted as `8-4-4-4-12` from SHA256("nori_salt:hostname:username") |
| `user_id` | Privacy hash: `sha256:<hex>` of `hostname:username` |
| `opt_out` | Boolean flag for analytics opt-out (defaults to false) |
| `first_installed_at` | Immutable timestamp of first install |
| `last_updated_at` | When version last changed |
| `last_launched_at` | Most recent launch time |
| `installed_version` | Current CLI version |
| `install_source` | `npm`, `bun`, or `unknown` |

**Analytics Events (`analytics.rs`):**

Uses flat event schema with top-level fields. Four event types:

| Event | When Sent | Top-Level Fields | Properties |
|-------|-----------|------------------|------------|
| `app_install` | First install | event, client_id, session_id, timestamp | version, os, arch, is_ci |
| `app_update` | Version upgrade | event, client_id, session_id, timestamp | version, os, arch, is_ci |
| `session_start` | Normal launch | event, client_id, session_id, timestamp | version, os, arch, is_ci |
| `user_resurrected` | >30 days since last launch | event, client_id, session_id, timestamp | version, os, arch, is_ci |

**Legacy nested schema** (still supported): `plugin_install_completed` and `nori_session_started` events with `tilework_cli_` prefixed parameters.

**Detection (`detection.rs`):**

- `detect_install_source()`: Checks `NORI_MANAGED_BY_BUN=1` then `NORI_MANAGED_BY_NPM=1`
- `generate_user_id()`: SHA256 hash of `{hostname}:{username}` for privacy

### Things to Know

- **Debug builds skip analytics**: `send_flat_event()` and `send_event()` are no-ops when `debug_assertions` is enabled, preventing noise during development and E2E testing
- **Atomic writes**: State file uses temp file + rename to prevent partial writes on crash
- **Client ID stability**: Deterministic UUID generated from `SHA256("nori_salt:hostname:username")`, formatted as `8-4-4-4-12`, persisted in state file
- **User ID stability**: Privacy hash `sha256:<hex>` of `hostname:username`, persisted in state file and reused across sessions
- **Session ID ephemeral**: Generated fresh per process using `uuid::Uuid::new_v4()`, never persisted to disk
- **Opt-out priority**: `NORI_NO_ANALYTICS=1` environment variable takes precedence over state file `opt_out` field
- **State updates even when opted out**: File timestamps and version are updated regardless of opt-out status
- **User resurrection**: Triggered when `>30 days` since `last_launched_at`, sends `user_resurrected` event before main event
- **Version comparison**: Upgrade detection uses simple string equality on `installed_version` vs `CLI_VERSION` constant
- **Network timeout**: Analytics requests timeout after 5 seconds (fire-and-forget, failures logged at debug level)

### Schema Migration (v1 → v2)

**Backward Compatibility:**
- V1 state files (`schema_version: 1, client_id: "nori-cli"`) deserialize correctly
- Missing `opt_out` field defaults to `false` via `#[serde(default)]`
- On first write after reading v1, state upgrades to v2 with new client_id and opt_out field

**Breaking Changes:**
- `client_id` changes from static `"nori-cli"` to deterministic UUID
- New required parameter `client_id` in `InstallState::new_first_install()`
- Legacy analytics functions still work but new code should use flat schema

Created and maintained by Nori.
