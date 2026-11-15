# Noridoc: Background session host

Path: @/nori-rs/cli/src/host

### Overview

- Implements `nori host`, its administrative commands, and `nori attach`
  discovery so agent sessions can outlive every terminal.
- Owns Unix process lifetime and private host files; protocol framing remains
  in [acp-host](../../../acp-host/docs.md), and session policy remains in the
  [harness](../../../harness/docs.md).

### How it fits into the larger codebase

- [CLI dispatch](../main.rs) resolves ordinary Nori configuration for startup
  and attach. Administrative list and stop only need host discovery, so they
  remain available if the configuration file becomes invalid. Attach requires
  `multi_session` and invokes the dedicated
  [terminal frontend](../../../tui/src/attach/docs.md), bypassing the
  local process-owning TUI application.
- [HostService](service.rs) implements `ManagedAgent`, delegates envelope
  operations to `ManagedRemoteHost`, and adds lifecycle commands over the same
  `SessionManager`. Agent initialization and transcript recording follow the
  ordinary harness path.
- The host starts with an empty manager and retains it until explicit stop or
  process termination. Ending the last session never ends the host.

```text
host command -> detached host -> authenticated /sessions -> HostService
                                   ^                         |
attach TUI ------------------------+                         v
host ls / host stop ---------------+                   SessionManager
                                                             |
                                                       agent processes
```

### Core Implementation

- [Startup](mod.rs) launches the same executable with a private foreground
  argument, starts a Unix process session, disconnects stdin, and redirects
  output to the host log. The launching command waits for an authenticated
  readiness check before reporting success.
- [HostFiles](state.rs) creates the host directory beneath `NORI_HOME` with
  owner-only permissions. It validates ownership, rejects file symlinks and
  nonregular files, and holds an OS file lock for the entire host lifetime.
  Discovery state contains the endpoint and PID; the lock decides liveness.
- Each start generates a token and binds loopback before publishing discovery
  state. Administrative clients initialize with `observe: true`, so list and
  stop do not claim the controller slot. [List formatting](table.rs) sanitizes
  control characters and colors status only on a terminal. Terminal rows fit
  the available display cells by truncating titles with an ellipsis at grapheme
  boundaries; if metadata alone exceeds the width, that prefix is clipped.
  Piped output retains full titles without color or a width limit.
- Lifecycle requests prepare agents using the host's configuration. Resume
  uses the stored agent and working directory while retaining the conversation
  identity. Stop closes the server, shuts down the manager, removes discovery
  state, then releases the lock.

### Things to Know

- Tokens are mandatory even for loopback. Local attach reads the private token
  automatically and receives a callback that rereads it with the same private
  file validation after a reconnect authentication failure. Explicit endpoints
  require `--host` together with `--token-file` and receive no local credential
  rediscovery callback. Token contents never appear in startup output.
- The authenticated listener exposes only `/sessions`; it does not change the
  legacy interactive remote-control listener. See the
  [wire contract](../../../../docs/remote-control.md#11-background-host-and-attached-terminals).
- The host has no persistent manager snapshot. Transcripts survive normally,
  but in-memory registry state and reconnect replay belong to the current host
  process. The token rotates when that process starts again.
- [Lifecycle tests](../../tests/host_lifecycle.rs) use isolated homes and
  bounded process waits to cover detached startup, privacy, lock ownership,
  occupied ports, and explicit stop.

Created and maintained by Nori.
