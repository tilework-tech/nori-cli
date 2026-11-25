# nori-host-runtime

Path: `nori-rs/host-runtime`

## Purpose

`nori-host-runtime` is the service behind every `nori host`: the background
host, the stdio host (`nori host --stdio`, `nori exec --acp`), the managed host
(`--stdio --launch`), the in-process host of one-shot `nori exec`, and the
private host that plain `nori` owns for its TUI. It wraps one session-runtime
`SessionManager` in `HostService`, implements the `ManagedAgent` trait that the
`nori-acp-host` transport serves, and owns the host's standard ACP v1 face,
its files under `$NORI_HOME/host/`, and managed-binding recovery. The wire it
serves is owned by the host contract
(`nori-monorepo: docs/specs/nori-cli/host-contract.md`); this page covers the
code. Terms follow [`GLOSSARY.md`](../../GLOSSARY.md).

## Modules

| Module | Main items | Role |
|---|---|---|
| `host/service.rs` | `HostService` | One manager, config and overrides, launches, settings, deleted set; implements `ManagedAgent` |
| `host/acp/` | `AcpFace` | Standard ACP v1 face: `handshake.rs` (`initialize`), `list.rs`, `attach.rs` (`session/new`, `load`, `resume`), `session.rs`, `config.rs` (host `agent` and `mode` options, settings kept across resume), `adapter_modes.rs`, `find_paths.rs`, `history.rs` (`_nori/host/history/*` over the host's own `history.jsonl`; recording is the controller's, reads are open to watchers) |
| `host/transcripts.rs` | | List, replay, and resume sessions that an earlier host or local run recorded under `$NORI_HOME/transcripts` |
| `host/state.rs` | `HostFiles`, `HostState`, `read_token`, `private_open` | `lock`, `token`, `host.json`; owner-only files |
| `host/tui_owned.rs` | `OwnedHost`, `StopHost`, `launch` | The private host that plain `nori` owns |
| `host/recovery_control.rs`, `host/recovery_history.rs` | `RecoveryControl` | Binding checkpoints and history for a managed host |

```text
   nori-cli (host, attach, exec)          nori-tui (plain nori)
          |   HostService::new                 |  launch() -> OwnedHost
          v                                    v
      RemoteAcpServer (nori-acp-host)    in-process pipe (controller)
          |  ManagedAgent                      |       + manager handle
          v                                    v
   +-------------------- HostService ------------------------+
   |  acp::AcpFace   settings   transcripts   recovery        |
   |                  SessionManager (nori-harness)            |
   +-----------------------------------------------------------+
                               |
                          agent servers
```

`OwnedHost` gives the TUI two handles to the same sessions: an ACP client pipe
that is always the controller, and the manager for foreground features the
wire has no method for yet. `StopHost` stops the pipe, then shuts down every
session.

## Dependency direction

```text
nori-cli, nori-tui --> nori-host-runtime --> nori-harness --> nori-acp-host
                                     \-----> nori-config, nori-protocol
```

`nori-host-runtime` must not depend on `nori-tui` or `nori-cli`. It reaches
the transport types through `nori_harness::remote_agent`, not through
`nori-acp-host` directly.

## Invariants

- One `SessionManager` per host. Every connection to the host sees the same
  sessions. The TUI never builds a manager of its own.
- Only the foreground launcher builds an `OwnedHost`. A terminal attached to
  another host never holds the in-process half.
- The owned host is private to its terminal: `[host] managed` does not apply,
  it publishes no discovery file, and only the TUI stops it.
- Each session loads config for its own folder (`config_in`), so that folder's
  project table and trust apply. The host's command-line overrides are
  reapplied to each folder.
- The ACP `sessionId` the face shows is the conversation ID.
- Restarting, replacing, and deleting sessions are serialized (`restarts`
  mutex), so two clients cannot start one session twice or revive a deleted
  one. Permission-mode changes are serialized (`mode_changes`), so the
  adapter's mode and the reported mode change together.
- The host directory is created with mode `0700` and must not be a symlink.
  Files in it open with mode `0600` and `O_NOFOLLOW`. The `lock` file is never
  removed. `host.json` is written to a temporary file and renamed.
- Background hosts need Unix owner-only permissions. `HostFiles::new` fails on
  other platforms.
- Permission modes are `ask`, `agent`, and `approve-all`
  (`nori_harness::permission_mode`). See
  [`docs/permissions.md`](../../docs/permissions.md).
- Wire changes (methods, `_meta` keys, error codes, `HOST_CONTRACT`) start in
  the host contract and `nori-monorepo: docs/acp-extensions.md`.

## Tests

| What | Where |
|---|---|
| Owned host | `src/host/tui_owned_tests.rs` |
| Face, config, recovery, state | `#[cfg(test)]` modules in `src/host/` |
| Host end to end through the `nori` binary | `nori-rs/cli/tests/host_lifecycle.rs`, `nori-rs/cli/tests/host_lifecycle/` |

Run:

```sh
cd nori-rs
cargo test -p nori-host-runtime
cargo build -p mock-acp-agent && cargo test -p nori-cli --test host_lifecycle
```
