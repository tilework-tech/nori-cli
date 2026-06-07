# CLI Cloud Session Refactoring — Current Progress

## Completed

### CLI Side (nori-cli repo) — All V1 work complete

1. **Cloud subcommand** — `nori cloud` connects the TUI to a remote sprite VM via broker WebSocket tunnel
2. **Broker client** — `broker/mod.rs` implements `BrokerClient` with acquire/release/auth (30 tests passing)
3. **Cloud connection** — `sacp_connection.rs` supports `connect_remote()` for cloud WebSocket connections
4. **cwd fix** — Removed hardcoded `/home/sprite/org/workspace` from `spawn_and_relay.rs` (commit `e5827749`). The CLI now uses its local cwd (`config.cwd`) for cloud sessions. The broker's SACP proxy handles sprite-side cwd via AcpTunnelManager.
5. **`source: "cli"` in acquire request** — `broker/mod.rs:115` sends `{"source": "cli"}` (pre-existing)
6. **Cloud disconnect handling** — `run_connection_event_relay` emits "Cloud session disconnected" error event when the transport closes unexpectedly (3 tests in `part6.rs`)
7. **E2E test script** — `scripts/cloud-e2e-test.sh` validates full flow through local broker to remote sprite (2 messages sent and received)
8. **No further CLI changes needed for V1** — The SACP protocol is unchanged; the broker's refactoring from dumb relay to SACP proxy via AcpTunnelManager is invisible to the CLI

### Broker Side (nori-sessions repo, branch `cli-cloud-sessions`)

1. **ConnectorConfig expansion** (commit `a0d4a5a7`)
   - Added 4 optional hooks to `ConnectorConfig`: `fetchThreadContext`, `onSessionCreated`, `onSessionDestroyed`, `getDiagnostics`
   - Guarded all 5 Slack-specific call sites in AcpTunnelManager
   - Made `SlackUserResolver` nullable (Slack path only)
   - Tests: 3 new tests in `manager.test.ts` for optional hooks

2. **SACP proxy replaces dumb relay** (commit `2cf075a1`)
   - Created `cli-session.ts` — SACP proxy with state machine (awaiting_initialize -> awaiting_session_new -> ready -> prompting -> errored)
   - Created `cliLifecycle/index.ts` — CLI lifecycle module following Discord's pattern
   - Wired into `server.ts` and `main.ts`
   - Deleted `cli-tunnel.ts` and its tests
   - Tests: 8 new tests in `cli-session.test.ts` for SACP proxy behavior

3. **Documentation** (commit `5ca919bd`)
   - Updated all relevant noridocs files
   - Created new `cliLifecycle/docs.md`

### Cloud Session Resume (2026-06-07)

1. **`resume_session()` cloud branching** — `session.rs` now mirrors the cloud-vs-local pattern from `spawn_and_relay.rs`: if `config.cloud_connection` is Some, uses `SacpConnection::connect_remote()` and sets `agent_config = None` with cloud-specific error messages. Previously it unconditionally called `get_agent_config()` and `SacpConnection::spawn()`, making cloud session resume impossible.

2. **TUI resume path threading** — `spawn_acp_agent_resume()` in `agent.rs` now accepts and passes through `cloud_connection`. `ChatWidget::new_resumed_acp()` in `constructors.rs` captures `cloud_connection` from `ChatWidgetInit` instead of discarding it.

3. **Tests** — 2 new tests in `part6.rs`: `cloud_resume_session_connects_and_produces_session_configured` and `cloud_resume_session_fails_with_unreachable_url`. All 577 ACP tests pass, all 1333 TUI tests pass.

4. **Documentation** — Updated `acp/docs.md`, `acp/src/broker/docs.md`, `acp/src/connection/docs.md`, and `tui/docs.md` to reflect cloud session resume support.

## Not Yet Done
2. **Broker PR** — The broker changes on `cli-cloud-sessions` branch (PR #830) need to be pushed and reviewed.
3. **`session/set_config_option` and `session/set_model` forwarding** — Currently stubbed with empty responses in the broker. Need pass-through methods on AcpTunnelManager to forward to underlying AcpClient. No CLI changes needed.
4. **Reconnection (V2)** — The broker-side plumbing enables reconnection (AcpTunnelManager holds session across CLI disconnects), but the CLI session picker UI is deferred.

## Verification (2026-06-05)

- All 598 ACP tests pass (including 3 cloud tests in `part6.rs` and 30 broker client tests)
- All 1329 TUI tests pass
- All 27 CLI tests pass
- `just fmt` and `just fix` clean — no warnings or errors
- E2E test script (`scripts/cloud-e2e-test.sh`) does not need updates — the patches target broker startup behavior, not session routing
