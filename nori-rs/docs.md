# Noridoc: nori-rs

Path: @/nori-rs

### Overview

This Cargo workspace contains the Nori CLI, its headless ACP session harness,
the Ratatui frontend, and focused support crates for configuration, protocol
types, agent hosting, sandboxing, authentication, and testing.

### How it fits into the larger codebase

The production path is layered downward:

```text
nori-cli / nori-tui
        -> nori-installed -> authenticated analytics ingress
        -> nori-harness
        -> nori-acp-host
        -> ACP agent subprocess
```

`nori-config` supplies resolved runtime policy to the frontend, harness, host,
and sandbox where needed. `nori-protocol` is the common public type boundary:
it re-exports the ACP schema and defines the small Nori-owned session event
branch. The mock agent and PTY suite exercise the same wire and harness paths
used in production.

### Core Implementation

- `cli/` dispatches the `nori` binary and sandbox debug commands. Its
  [host module](cli/src/host/docs.md) owns the detached process, private token,
  and session manager used by `nori host`, whose sessions each work in
  their own folder. One-shot `nori exec "prompt"` is a thin ACP client of
  that host: it uses the home's running host when it can and an in-process
  host otherwise.
- `tui/` renders events and forwards user intent through typed harness methods.
  Its [attach frontend](tui/src/attach/docs.md) instead consumes host-owned
  envelopes and sends managed commands; closing that terminal never ends the
  host's sessions.
- `harness/` owns session orchestration, private reduction, transcripts, goals,
  undo, hooks, worktrees, history, and the embeddable runtime API. Its public
  event stream is an ordered fan-out: one primary frontend receiver plus
  bounded subscribers. Its `session_manager` module
  keeps a registry of live sessions (one agent process each) keyed by
  conversation ID; the TUI starts all of its sessions through it.
- `acp-host/` owns the client-side ACP SDK, subprocess lifecycle, JSON-RPC
  connection, agent registry, delegated requests, and host-handled filesystem
  operations. It also owns the remote ACP transport the background host
  (`nori host`) serves: an authenticated `/sessions` WebSocket with
  controller/watcher roles, plus stdio and pipe variants, behind a
  `ManagedAgent` trait that `harness/` implements over its `SessionManager`,
  preserving the `nori-harness -> nori-acp-host` dependency direction (see
  [the wire contract](../docs/remote-control.md)). The interactive TUI opens
  no listener; the earlier unauthenticated `/acp` surface (`--remote`,
  `/remote-control`) is retired.
- `nori-protocol/` owns no behavior. It exports `nori_protocol::acp`,
  `SessionEvent::{Acp, Nori}`, and the session manager's `SessionEnvelope` and
  `SessionStatus` types.
- `nori-config/` owns CLI configuration and the approval, sandbox, MCP, trust,
  and shell-policy types consumed at runtime.
- `installed/` keeps local installation/launch state and owns the authenticated
  `nori_agent_session_started` reporter. Frontends attach it to a harness
  handle so one logical session is counted only after its first prompt reaches
  ACP; identity and organization expansion remain server-owned.
- `sandbox/` and the platform crates implement local sandbox debug execution;
  agent tool execution itself occurs in the external ACP agent.

The protocol hard cut deleted the inherited `protocol/` and
`app-server-protocol/` crates. Client-side schema imports now go through
`nori_protocol::acp`; only `nori-protocol` directly depends on the schema crate,
and only `nori-acp-host` directly depends on the ACP SDK among client-side
product crates.

### Things to Know

- The public harness boundary is the stable embedding surface. Private
  reduction in `harness` and private presentation projection in `tui` are not
  protocol APIs.
- Transcript schema v3 records user input and the exact public `SessionEvent`
  stream. Private loader compatibility still reads v2 normalized records; the
  versioned storage enum is not part of the public Harness API.
- ACP `session/list`, `session/resume`, and `session/close` power cloud and
  agent-sourced session lifecycle behavior; Nori does not mirror those
  responses into another protocol.
- ACP capabilities describe an agent facade's operations, not whether the
  deployment is local or cloud. The top-level `nori cloud` launch carries that
  identity explicitly into the TUI.
- Product analytics distinguishes `interactive`, `cloud`, and `exec` session
  modes; the `acp` mode value still exists but nothing sends it since
  `nori exec --acp` became the headless host. Resume is not a mode or event; a resumed interactive session
  reports only if a new prompt reaches the transport.
- The outward ACP surfaces are the background host's standard ACP face and
  the interactive remote WebSocket projection. The host serves its face on an
  authenticated `/sessions` WebSocket and, for one client, on stdio
  (`nori host --stdio`, aliased by `nori exec --acp`); its lifetime is
  independent of connected terminals.
- Rust 2024 and strict workspace lints apply. Add only the derive traits a
  public boundary type actually needs.

Created and maintained by Nori.
