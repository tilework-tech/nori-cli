# Cloud Session E2E Validation — Handoff Summary

## Initial User Prompt

> Take a look at the work currently in this worktree. We are working on validating that this works as expected. First kill any local broker services. Then, using the corresponding PR / worktree in the nori-sessions repo (~/code/nori/nori-sessions), spin up a local broker instance that is connected to the amol-kapoor flyio backend. Finally, validate that you can do two end to end messages by spinning up the cli in tmux using nori cloud, and then starting an end to end acp session.

After the initial E2E validation succeeded, the user asked:

> Can you write a complete integration test that does the e2e complete with locally deployed broker and session claim verification

## Context

**Worktree:** `/home/amol/code/nori/nori-cli/.worktrees/warm-map-20260521-191426`
**Branch:** `auto/cli-cloud-session-integration-design-20260521-191618`
**Feature:** CLI cloud session integration — `nori cloud` subcommand connects the TUI to a remote sprite VM via a broker WebSocket tunnel.

**Two repos involved:**
- **nori-cli** (this repo): The CLI with the `nori cloud` subcommand. Branch: `auto/cli-cloud-session-integration-design-20260521-191618`
- **nori-sessions** (broker): Worktree at `~/code/nori/nori-sessions/.worktrees/cli-cloud-sessions`, branch `cli-cloud-sessions`, PR #830

**Broker config for local testing:**
- `NORI_ORG=local`
- `NORI_SPRITE_ORG=amol-kapoor`
- `NORI_SPRITE_TOKEN=amol-kapoor/1360907/13cf9c6c65323c544c0a25a51bf3f163/0698cb387dead39bc8595fb3f7309568a72553a98a7c49ee1f57555625eb19a2`
- Broker runs on port 19400
- Broker server dir: `~/code/nori/nori-sessions/.worktrees/cli-cloud-sessions/broker/server`
- Start: `cd broker/server && NORI_ORG=local NORI_SPRITE_ORG=amol-kapoor NORI_SPRITE_TOKEN=... bun run src/main.ts`

## Critical Research Findings

### Architecture of `nori cloud`
1. CLI authenticates via browser OAuth flow → saves JWT to `~/.nori/cli/cloud-auth.json`
2. CLI POSTs to `{broker}/api/sessions/acquire` with `{"source":"cli"}` → gets `{session_id, ws_url}`
3. CLI connects WebSocket to `ws_url` (broker tunnel endpoint)
4. Broker connects a second WebSocket to the sprite's `/acp` endpoint
5. Broker relays messages bidirectionally between CLI and sprite WebSockets
6. CLI does SACP v11 handshake (Initialize → session/new) over the tunnel
7. TUI renders normally — identical to local mode

### Bugs Found and Fixed

**Bug 1 — Nori control frame not filtered in CLI tunnel (BROKER FIX)**
- **File:** `broker/server/src/inbound/ws/cli-tunnel.ts`
- **Root cause:** The sprite bridge sends `__nori_bridge_connection__:{...}` as the first WebSocket message. The existing Slack/Discord ACP path filters these via `isNoriControlFrame()`, but the new CLI tunnel relay forwarded them transparently. The CLI's SACP transport tried to parse it as JSON-RPC and crashed (code 1006, ~150ms after connect).
- **Fix applied:** Added `isNoriControlFrame()` import and filtering in the sprite→client relay handler. This is a real bug that needs to ship with the PR.

**Bug 2 — cwd sent as local path in session/new (CLI DESIGN ISSUE — NOT YET FIXED)**
- **Root cause:** The CLI sends its local working directory (e.g., `/home/amol/code/nori/...`) in `session/new`. The SDK on the sprite spawns Claude Code with that as the working directory. Since the path doesn't exist on the sprite, spawn fails with ENOENT (misleadingly reported as "Claude Code native binary not found").
- **Current workaround:** The E2E test script creates the local cwd directory on the sprite via `sprite_exec "mkdir -p '$NORI_RS_DIR'"`.
- **Proper fix needed:** The CLI cloud path should send a cloud-appropriate cwd (like `/home/sprite/org/workspace`) instead of the local path. This is in `nori-rs/cli/src/main.rs` where the TUI is launched — the `cwd` passed to `create_session()` needs to be overridden for cloud mode.

### Issues That Were NOT Bugs (DEV testing environment)
The broker's `cli-cloud-sessions` branch has intentional DEV modifications in `manager.ts` and `sessions.ts` that disable the provisioning pipeline (credential distribution, GC sweep, agent restart on acquire). These caused:
- Sprites stuck as `claimed` (DEV mod marks all as claimed instead of ready) — **fixed locally** by changing `discoverExisting()` else branch to `lifecycle: 'ready'`
- Missing Claude credentials on sprite after checkpoint restore — **workaround:** E2E script copies local `~/.claude/.credentials.json` to the sprite
- Bridge needing restart after checkpoint restore — **workaround:** E2E script restarts via sprites REST API

These are NOT bugs in the feature code — they're consequences of the DEV testing setup. In production, the broker's provisioning pipeline handles all of this automatically.

## Current Progress

### Completed
1. **E2E validation passed** — Two messages sent and received via `nori cloud` through the local broker to a remote sprite VM
2. **E2E test script written and passing** — `scripts/cloud-e2e-test.sh` automates the full flow:
   - Starts broker, validates auth, acquires session
   - Configures sprite (checkpoint restore, cwd creation, credential push, bridge restart)
   - Launches `nori cloud` in tmux, sends 2 messages, asserts responses
   - Cleans up on exit
   - Last run: ALL TESTS PASSED (~60 seconds)

### Files Changed
**nori-cli repo (this worktree):**
- `scripts/cloud-e2e-test.sh` — NEW: E2E test script (not yet committed)
- `APPLICATION_SPEC.md` — modified (pre-existing change, not ours)

**nori-sessions repo (`cli-cloud-sessions` worktree) — unstaged changes:**
- `broker/server/src/inbound/ws/cli-tunnel.ts` — Control frame filtering fix + temporary debug relay logging
- `broker/server/src/features/lifecycle/manager.ts` — DEV mod changed: `discoverExisting()` now marks sprites as `ready` instead of `claimed`
- `broker/server/src/fleet/base-version.ts` — Pre-existing DEV mod (not ours)
- `broker/server/src/inbound/http/endpoints/sessions.ts` — Pre-existing DEV mod (not ours)

## What Remains To Be Done

### 1. Remove temporary debug logging from `cli-tunnel.ts`
The relay handler in `cli-tunnel.ts` has `[RELAY]` log lines added for debugging. These should be removed before the broker PR is finalized. Keep only the `isNoriControlFrame()` filtering fix.

### 2. Fix the `cwd` design issue in the CLI cloud path
The CLI sends its local cwd in `session/new`, which breaks on the sprite. Needs a proper fix — NOT the current hack of creating the directory on the sprite. The fix should be in the CLI code where `create_session(cwd, mcp_servers)` is called for cloud mode. Key files:
- `nori-rs/cli/src/main.rs` (lines ~552-608) — cloud command handler, where the TUI is launched
- `nori-rs/acp/src/connection/sacp_connection.rs` (line 596) — `create_session` sends `NewSessionRequest::new(cwd)`
- The cwd needs to be overridden to a sprite-appropriate path (e.g., `/home/sprite/org/workspace`) when in cloud mode

### 3. Commit the E2E test script
`scripts/cloud-e2e-test.sh` needs to be committed. Consider whether it should be in `scripts/` or somewhere else.

### 4. Update documentation
Per the workflow, update docs in `docs/` folder if applicable. The cloud session feature likely needs documentation updates.

### 5. Run finishing-a-development-branch skill
Final checks per the Nori workflow.

## Questions for User

1. **cwd fix scope:** Should the CLI hardcode `/home/sprite/org/workspace` for cloud mode, or should the broker's acquire response include a recommended cwd? The broker knows the sprite's workspace layout.
2. **Broker changes:** The control frame fix is in the broker repo, not this one. Should it be committed to the `cli-cloud-sessions` branch there, or does the user want to handle the broker PR separately?
3. **E2E test location:** Is `scripts/cloud-e2e-test.sh` the right place, or should it live elsewhere?
