# Debugging the Nori CLI

This runbook covers where Nori logs, how to turn up logging, how to record the
ACP wire, how to troubleshoot `nori host`, and how to run the PTY end-to-end
tests. All paths are under `NORI_HOME`, which defaults to `~/.nori/cli`. The
full file list is in [../storage.md](../storage.md).

## Logs

| File | Written by | Contents |
|---|---|---|
| `log/nori-acp.YYYY-MM-DD.log` | Every `nori` process: TUI, `nori exec`, `nori host`, `nori attach` | The main log. A new file each day. |
| `log/nori-tui.log` | The TUI | TUI-only log, mode `0600`. Stays empty when the TUI runs through `nori`, because `nori` already set up `nori-acp.*.log`. |
| `log/session-<UTC time>.jsonl` | The TUI, when `CODEX_TUI_RECORD_SESSION` is set | A record of UI events, mode `0600`. |
| `host/host.log` | A background host started by `nori host` | The host process's stdout and stderr. Appended on every start. |
| `acp-wire/*.jsonl` | Each agent process, when wire recording is on | Raw ACP messages. See [Record the ACP wire](#record-the-acp-wire). |

What to look for in `nori-acp.*.log`:

- `ACP agent stderr: ...` lines carry the agent's own stderr, at `WARN`. A
  failed agent start usually explains itself here. Nori also shows the last
  20 stderr lines to the user when an agent dies.
- A credential that Nori passed to the agent is redacted from stderr lines.

### Log levels and `RUST_LOG`

`RUST_LOG` sets the filter for `nori-acp.*.log`. It uses the
`tracing-subscriber` `EnvFilter` syntax. Setting it replaces the default, so
keep a base level.

| Build | Default filter |
|---|---|
| Release | `warn,nori_tui=info,acp=info` |
| Debug (`cargo run`) | `debug` |

Useful targets:

| Target | What it logs | Level to set |
|---|---|---|
| `nori_tui` | The TUI | `info`, `debug` |
| `nori_harness` | The session runtime: sessions, turns, the queue, delegation | `debug` |
| `nori_acp_host` | Agent launch, ACP connections, the host wire | `debug` |
| `nori_host_runtime` | `nori host` and the private host | `debug` |
| `codex_rmcp_client` | MCP client and MCP OAuth | `info`, `debug` |
| `acp_event_flow` | Every ACP event as the session runtime reduces it | `debug` |
| `nori_resume` | How resume finds and loads transcripts | `info` |
| `nori_context_profile` | The opt-in context profiler | `debug` |

Examples:

```bash
RUST_LOG=warn,nori_harness=debug,nori_acp_host=debug nori
RUST_LOG=warn,acp_event_flow=debug nori exec "hello"
RUST_LOG=warn,nori_resume=info nori resume --last
tail -f ~/.nori/cli/log/nori-acp.$(date +%F).log
```

A background host keeps the environment of the `nori host` command that
started it. To change its filter, stop it and start it again with the new
`RUST_LOG`.

### UI event log

```bash
CODEX_TUI_RECORD_SESSION=1 nori
CODEX_TUI_RECORD_SESSION=1 CODEX_TUI_SESSION_LOG_PATH=/tmp/ui.jsonl nori
```

`CODEX_TUI_RECORD_SESSION` accepts `1`, `true`, `TRUE`, `yes`, or `YES`.
Without `CODEX_TUI_SESSION_LOG_PATH`, the file is
`$NORI_HOME/log/session-<YYYYMMDDTHHMMSSZ>.jsonl`. The first record holds the
folder and the agent.

### Context profiler

`NORI_CONTEXT_PROFILE_DIR=<dir>` measures instruction discovery and token
counts and writes JSONL to that folder and to the terminal.
`NORI_CONTEXT_ENFORCE_BUDGET` makes the profiler enforce its budget.

## Record the ACP wire

Wire recording saves every JSON-RPC message between Nori and each agent
process.

Turn it on in one of two ways:

- In the TUI, open `/agent` and press Shift+Tab. The picker footer shows
  "rec on" or "rec off". Nori saves the choice.
- In `config.toml`:

  ```toml
  [acp_proxy]
  enabled = true
  ```

Rules:

- Recording applies to agent processes started after it is turned on. An agent
  that is already running is not recorded. Start a new session.
- A background host reads `config.toml` when it starts. Restart it to change
  recording.
- Each agent process gets one file:
  `$NORI_HOME/acp-wire/<unix-ms>-<child-pid>-<agent>.jsonl`.
- Each line is one message:
  `{"ts_ms", "direction", "agent", "child_pid", "message"}`. `direction` is
  `client_to_agent` or `agent_to_client`. A line that is not JSON is stored as
  `raw_line` with a `parse_error`.
- A message that contains the credential Nori passed to the agent is stored as
  `"[redacted]"`.
- The files hold prompts, file contents, and tool output. Turn recording off
  when you are done, and treat the files as private.

## Host

`nori host` runs a background host. Its files are in `$NORI_HOME/host/`. The
discovery files and their contract are owned by
`nori-monorepo: docs/specs/nori-cli/host-contract.md`.

| File | Purpose |
|---|---|
| `lock` | OS lock. Held while a host runs. Never deleted. |
| `host.json` | Discovery record: endpoint, token, PID, instance ID, version, contract, who started it. |
| `state.json` | Endpoint and PID. |
| `token` | Bearer token. New on every start. |
| `host.log` | The host's stdout and stderr. |

Plain `nori` never uses these files. Its private host lives in the `nori`
process.

### Check the state

```bash
nori host                # starts a host, or prints "Host already running (pid N)  <endpoint>"
nori host ls             # lists sessions; fails when no host answers
cat ~/.nori/cli/host/host.json
tail -n 100 ~/.nori/cli/host/host.log
```

### Problems and fixes

| Symptom | Cause | Fix |
|---|---|---|
| `host startup failed; check .../host.log: host exited before publishing readiness` | The host process failed before it reported ready. | Read `host.log`. A common cause is a port in use (default `7420`; `--port 0` picks a free one). |
| `host startup failed; check .../host.log: host startup timed out` | The host did not report ready within 15 s. `nori` killed it. | Read `host.log`. |
| `timed out waiting for another host operation` | Another `nori` held `start.lock` for 15 s while it started or checked the host. | Run the command again. If it repeats, read `host.log`. |
| `a host owns this home but did not confirm readiness` | A process holds `lock` but does not answer the lifecycle probe. | Read `host.log`. Find the process with the PID from `state.json`. |
| `another host is already starting or running` | Another process holds `lock`. | Use the running host, or stop it with `nori host stop`. |
| `the running host (pid N, nori V, contract C) does not serve this nori's contract C2; ...` | The running host comes from a different `nori` version with an incompatible host contract. | `nori host stop`, then `nori host`. Or run a second host in another home: `NORI_HOME=<dir> nori host`. |
| `no host running; start one with \`nori host\`` | No process holds `lock`. | Start a host. Leftover `host.json` or `state.json` from a crash does not matter; the next start replaces them. |
| `host did not answer` | The host did not reply to `initialize` within 3 s. | Check `host.log`. Stop and start the host. |
| `host path must be owned by this user with owner-only permissions: ...` | A file or `host/` has group or other permissions, or another owner. | `chmod 700 ~/.nori/cli/host && chmod 600 ~/.nori/cli/host/*`, or fix the owner. |
| `host directory must not be a symlink` | `host/` is a symlink. | Replace it with a real folder. |
| `background hosts require Unix owner-only file permissions on this platform` | Not a Unix system. | Use plain `nori`. |
| HTTP `401`: `A valid host bearer token is required.` | Missing or old token. The token changes on every host start. | Local: run the command again; it rereads `token`. Remote (`--host`): copy the new token into the `--token-file`. |
| HTTP `426` | A plain HTTP request, not a WebSocket upgrade. | Connect with a WebSocket client. |
| HTTP `503`: `This nori host already serves N clients, its limit. ...` | `[host] max_clients` (default 16) clients are connected. | Close a client, or raise `[host] max_clients` and restart the host. |
| Attach shows `Host restarted; sessions were lost. Re-run nori attach.` | The host instance ID changed. | Run `nori attach` again. |
| Attach shows `Authentication failed; host may have restarted. Re-run nori attach.` | The token was rejected and could not be refreshed. | Run `nori attach` again. |
| Attach shows `Host identity unavailable; cannot safely resume. Re-run nori attach.` | The host sent no instance ID. | Run `nori attach` again. |
| Attach shows `Host stopped responding` | No message for 10 s. Attach reconnects every 750 ms. | Check that the host process runs; read `host.log`. |
| Attach shows `Read-only: take control with alt+t ...` | This client is a watcher. | Press `alt+t`. |

### How `nori host stop` works

1. If no process holds `lock`, it prints `No host running.`
2. It connects on the host's reserved lifecycle connection, which works at
   the client limit. It checks that the host is the instance recorded in
   `host.json` and `state.json`, and fails with `retry stop` when it is not.
3. It sends `_nori/host/stop`.
4. It waits up to 30 s for the host to release `lock`, then prints
   `Host stopped.` Otherwise it fails with
   `host is still shutting down; check .../host.log`.

Stopping the host ends every session it owns. If `stop` timed out, check
`host.log`, then find the process with the PID from `state.json`.

## PTY end-to-end tests

The `tui-pty-e2e` crate runs the real `nori` binary in a pseudo-terminal
against `mock-acp-agent`, a scripted ACP agent. It checks the screen with
snapshots.

Run every test from the repo root:

```bash
just test e2e
```

This builds `nori`, `nori-tui`, and `mock_acp_agent`, then runs
`cargo test -p tui-pty-e2e`. Arguments after `e2e` go to `cargo test`:

```bash
just test e2e --test host_attach
just test e2e --test multi_session -- some_test_name
```

Environment:

| Variable | Effect |
|---|---|
| `MOCK_ACP_AGENT_BIN` | Path to `mock_acp_agent`. Without it, Nori looks next to its own binary in `target/`. CI sets it. |
| `DEBUG_TUI_PTY` | Any value prints the harness's debug output. |
| `NO_COLOR` | Turns off color in that debug output. |

Each test runs with a temporary `NORI_HOME`, `TERM=xterm-256color`,
`NORI_SYNC_SYSTEM_INFO=1`, and
`RUST_LOG=codex_core=info,nori_tui=info,codex_rmcp_client=info,nori_harness=debug,nori_acp_host=debug`.

Terminals do not render OSC 7501 status reports. `program_status_reports(raw)`
in `tui-pty-e2e` returns the body of each report in `raw_output()` in order
(for example `state=working:app=nori`).

Tests that need real agents and credentials are marked `#[ignore]`. Run them
with:

```bash
cd nori-rs && cargo test -p tui-pty-e2e -- --ignored
```

Snapshots live in `nori-rs/tui-pty-e2e/tests/snapshots/`. Review changes with
`cargo insta pending-snapshots -p tui-pty-e2e` and accept them with
`cargo insta accept -p tui-pty-e2e`.

CI (`.github/workflows/rust-ci.yml`) builds `mock-acp-agent` to
`target/mock-acp-out/`, sets `MOCK_ACP_AGENT_BIN`, and runs the suite with
`--profile ci-test` on Linux for pull requests, and on Linux and macOS for
pushes to `main`.

## Where `main` differs

- The TUI's own default filter (`nori_tui=info,codex_rmcp_client=info`) never
  applies when the TUI runs through `nori`; `nori-tui.log` stays empty.
- The UI event log uses the `CODEX_TUI_RECORD_SESSION` and
  `CODEX_TUI_SESSION_LOG_PATH` names, and the PTY harness still sets
  `CODEX_HOME`.
