# Noridoc: Nori CLI

Path: @/

### Overview

Nori CLI is a Rust terminal frontend and headless session harness for ACP
agents. The shipped `nori` binary composes configuration, a reusable harness,
and the Ratatui frontend; the npm package in `nori-cli/` is a thin launcher.

### How it fits into the larger codebase

- `nori-rs/` contains the Cargo workspace and production binary.
- `nori-cli/` packages that binary for npm distribution.
- `docs/specs/` records durable architecture decisions; `docs/plans/` records
  their execution.
- ACP owns agent-to-client messages, plans, tools, permissions, capabilities,
  configuration options, usage, and responses. Nori adds only lifecycle and
  product behavior ACP does not define.

### Core Implementation

```text
nori-cli / nori-tui
         |
         v
   nori-harness  <-------- nori-config
         |
         v
  nori-acp-host  <-------- codex-rmcp-client (MCP credentials)
         |
         v
    ACP agent process

Public boundary used by every client-side layer:
    nori-protocol
      ├── nori_protocol::acp   (re-exported ACP schema)
      └── SessionEvent::{Acp, Nori}
```

`nori-protocol` is the sole direct ACP schema dependency and re-exports it as
`nori_protocol::acp`. `nori-acp-host` alone uses the higher-level ACP SDK on the
client side. The harness exposes typed control methods and one ordered
`SessionEvent` stream. Frontends match the source branch first: raw ACP
envelopes remain ACP-shaped, while the Nori branch carries lifecycle, queue,
replay, goals, undo, user shell, hooks, notices, and failures for which no ACP
response exists.

Configuration is resolved from `$NORI_HOME/config.toml` (default
`~/.nori/cli/config.toml`) and injected into session launches. Transcripts live
under the same Nori home and can be resumed through `nori resume`.

### Things to Know

- The former `codex-protocol` and `codex-app-server-protocol` crates were
  deleted in the ACP-canonical hard cut. There is no deprecated facade or
  second public normalized ACP vocabulary.
- `nori-config` owns approval, sandbox, MCP, trust, and shell policy.
  `codex-rmcp-client` owns computed MCP authentication status.
- Filesystem requests from an ACP agent are currently handled by the host.
  Permission requests that require a consumer decision are delegated with
  their raw `RequestId`; `AskForApproval::Never` resolves them internally.
- Cross-platform sandboxing uses Landlock on Linux, Seatbelt on macOS, and
  restricted tokens on Windows.
- The crate-layering decision and the exact protocol contract are documented
  in `@/docs/specs/crate-layering.md` and
  `@/docs/specs/protocol-unification.md`.

Created and maintained by Nori.
