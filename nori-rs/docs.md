# Noridoc: nori-rs

Path: @/nori-rs

### Overview

This Cargo workspace contains the Nori CLI, its headless ACP session harness,
the Ratatui frontend, and focused support crates for configuration, protocol
types, agent hosting, sandboxing, authentication, and testing.

### How it fits into the larger codebase

The production path is layered downward:

```text
nori-cli / nori-tui / nori-exec
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

- `cli/` dispatches the `nori` binary and sandbox debug commands.
- `exec/` projects the harness into a final-answer plaintext command or a
  bounded standard ACP-over-stdio agent facade.
- `tui/` renders events and forwards user intent through typed harness methods.
- `harness/` owns session orchestration, private reduction, transcripts, goals,
  undo, hooks, worktrees, history, and the embeddable runtime API. Its public
  event stream is an ordered fan-out: one primary frontend receiver plus
  bounded subscribers such as the remote ACP host.
- `acp-host/` owns the client-side ACP SDK, subprocess lifecycle, JSON-RPC
  connection, agent registry, delegated requests, and host-handled filesystem
  operations. It also owns the optional remote ACP transport: a WebSocket
  server that serves the running interactive session outward as an ACP Agent
  through a `HostedAgent` trait that `harness/` implements, preserving the
  `nori-harness -> nori-acp-host` dependency direction. The TUI owns startup
  `--remote` and runtime `/remote-control` listener policy around that server
  (see `@/docs/specs/remote-acp-transport.md`).
- `nori-protocol/` owns no behavior. It exports `nori_protocol::acp` and
  `SessionEvent::{Acp, Nori}`.
- `nori-config/` owns CLI configuration and the approval, sandbox, MCP, trust,
  and shell-policy types consumed at runtime.
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
- There are two outward ACP agent surfaces: `nori exec --acp` (a bounded,
  one-session stdio facade) and the remote WebSocket transport (the long-lived
  interactive session, enabled at startup or runtime, loopback by default,
  stable Nori conversation ids as outward session ids). They are deliberately
  separate; neither replaces the other.
- Rust 2024 and strict workspace lints apply. Add only the derive traits a
  public boundary type actually needs.

Created and maintained by Nori.
