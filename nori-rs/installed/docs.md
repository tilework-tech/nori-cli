# Noridoc: nori-installed

Path: @/nori-rs/installed

### Overview

The installed crate tracks CLI lifecycle events for analytics purposes. It maintains a state file (`~/.nori/cli/.nori-install.json`) that records installation date, version history, and session information.

### How it fits into the larger codebase

Called early in `@/nori-rs/tui/` startup via `track_launch()`. This is a non-blocking, fire-and-forget operation that never delays CLI startup.

### Core Implementation

**State Management** (`state.rs`): The `InstallState` struct persists:
- `client_id` - Anonymous UUID for analytics
- `installed_version` - Current CLI version
- `first_installed_at` - Original installation timestamp
- `last_launched_at` - Most recent session timestamp
- `opt_out` - Analytics opt-out flag

**Launch Detection** (`lib.rs`): `track_launch_inner()` determines:
- `AppInstall` - First time installation
- `AppUpdate` - Version change (upgrade or downgrade)
- `SessionStart` - Normal session
- `UserResurrected` - User returned after 30+ day absence

**Analytics** (`analytics.rs`): Sends events to analytics endpoint:
- `InstallDetected`
- `SessionStart`
- `UserResurrected`

**Install Source Detection** (`detection.rs`): Detects installation method (npm, binary, etc.) for analytics segmentation.

### Things to Know

- Analytics can be disabled via `NORI_ANALYTICS_OPT_OUT=1` environment variable
- Analytics are automatically skipped in CI environments
- The client ID is a UUID (legacy IDs like "nori-cli" are migrated)
- State file uses JSON format with schema versioning
- All errors are silently logged at debug level to avoid impacting CLI startup

Created and maintained by Nori.
