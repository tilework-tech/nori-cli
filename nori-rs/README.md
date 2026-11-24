# nori-rs

The Rust workspace behind the `nori` binary — a multi-provider terminal AI
coding assistant. Nori is an ACP (Agent Client Protocol) host: it spawns
Claude Code, Codex, Antigravity, Grok Build, OpenCode, Cursor Agent, Hermes Agent, Pi, or a configured agent and drives it
over JSON-RPC/stdio, presenting one terminal interface for all of them.

The workspace began as a fork of the OpenAI Codex CLI. The codex agent engine
has since been removed; every agent — including Codex — runs as an external
ACP subprocess. Remaining `codex-*` crates are inherited utility code being
progressively adopted or removed (see the crate layering rules in `../docs.md`).

## Key crates

| Crate | Purpose |
|-------|---------|
| `cli/` (`nori-cli`) | The shipped `nori` binary: dispatch and subcommands |
| `tui/` (`nori-tui`) | Ratatui interactive terminal interface |
| `harness/` (`nori-harness`) | Headless ACP session harness: session runtime, transcripts, hooks |
| `acp-host/` (`nori-acp-host`) | Agent-agnostic ACP hosting: subprocess spawn, wire client, registry |
| `nori-config/` | Nori config layer (`~/.nori/cli/config.toml`) |
| `auth-providers/` (`nori-auth-providers`) | Per-agent sign-in detection and API keys saved per model provider through Nori |
| `nori-protocol/` | Session-runtime types over the ACP schema |
| `sandbox/` (`codex-sandbox`) | Sandboxed exec engine: Seatbelt, Landlock/seccomp, Windows restricted tokens |
| `installed/` (`nori-installed`) | Local install state and prompt-bound authenticated analytics |
| `mock-acp-agent/` | Mock ACP agent used by tests |
| `tui-pty-e2e/` | End-to-end PTY tests driving the real binary |

## Crate documentation

CLI docs across nori-cli, nori-sessions and nori-monorepo, with status: [cross-repo index](https://github.com/tilework-tech/nori-monorepo/blob/main/docs/cli-docs-index.md).

Cross-cutting docs: [workspace noridoc](docs.md), [configuration reference](config.md), [topic docs](../docs/README.md), [roadmap](../ROADMAP.md).

### Host and runtime

| Docs                                                                 | Purpose                                                              |
| -------------------------------------------------------------------- | -------------------------------------------------------------------- |
| [cli](cli/docs.md)                                                   | The `nori` binary: argument parsing and subcommand routing           |
| [cli/src/host](cli/src/host/docs.md)                                 | `nori host` background host and `nori attach` discovery              |
| [acp-host](acp-host/docs.md)                                         | Agent-agnostic ACP host: SDK connection, subprocess, registry        |
| [acp-host/src/connection](acp-host/src/connection/docs.md)           | Host side of one ACP stdio connection and its child lifecycle        |
| [harness](harness/docs.md)                                           | Headless runtime for ACP sessions: lifecycle, reduction, transcripts |
| [harness/src/session_manager](harness/src/session_manager/docs.md)   | Supervises agent processes and the session registry                  |
| [harness/src/context_timeline](harness/src/context_timeline/docs.md) | Prompt-boundary checkpoints and context growth for `/context`        |
| [nori-protocol](nori-protocol/docs.md)                               | Types-only public boundary for embedding a Nori ACP session          |
| [nori-config](nori-config/docs.md)                                   | Configuration and runtime-policy boundary                            |

### TUI

| Docs                                                                                                                                                             | Purpose                                                 |
| ---------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------- |
| [tui](tui/docs.md)                                                                                                                                               | Ratatui frontend for the harness                        |
| [tui/src/attach](tui/src/attach/docs.md)                                                                                                                         | Terminal attached to a background host                  |
| [tui/src/host_client](tui/src/host_client/docs.md)                                                                                                               | Connection and request correlation for a `nori host`    |
| [tui/src/chatwidget/EXEC_CELL_LIFECYCLE.md](tui/src/chatwidget/EXEC_CELL_LIFECYCLE.md)                                                                           | How ACP tool calls become chat history cells and render |
| [tui/styles.md](tui/styles.md)                                                                                                                                   | TUI text and color style rules                          |
| [tui-components](tui-components/docs.md) ([README](tui-components/README.md), [DESIGN](tui-components/DESIGN.md), [examples](tui-components/examples/README.md)) | Reusable, domain-free Ratatui components                |

### Providers and auth

| Docs                                                           | Purpose                                                    |
| -------------------------------------------------------------- | ---------------------------------------------------------- |
| [auth-providers](auth-providers/docs.md)                       | Agent sign-in status and saved API keys per model provider |
| [login](login/docs.md)                                         | OAuth device-code and browser login flows                  |
| [skillsets](skillsets/docs.md)                                 | Client for the skillsets v2 CLI                            |
| [installed](installed/docs.md)                                 | Local install and launch state, analytics identity         |
| [keyring-store](keyring-store/docs.md)                         | OS keyring abstraction for credentials                     |
| [rmcp-client](rmcp-client/docs.md)                             | MCP client connections, OAuth, and discovery               |
| [mcp-types](mcp-types/docs.md) ([README](mcp-types/README.md)) | Generated MCP protocol types                               |

### Sandboxing

| Docs                                                                                   | Purpose                                                      |
| -------------------------------------------------------------------------------------- | ------------------------------------------------------------ |
| [sandbox](sandbox/docs.md)                                                             | Sandboxed exec engine and platform sandbox selection         |
| [linux-sandbox](linux-sandbox/docs.md) ([README](linux-sandbox/README.md))             | Landlock and seccomp helper for Linux                        |
| [windows-sandbox-rs](windows-sandbox-rs/docs.md)                                       | Restricted-token sandbox for Windows                         |
| [execpolicy](execpolicy/docs.md) ([README](execpolicy/README.md))                      | Starlark prefix-rule policies behind `nori execpolicy check` |
| [process-hardening](process-hardening/docs.md) ([README](process-hardening/README.md)) | Early process hardening at startup                           |

### Utilities

| Docs                                                                    | Purpose                                                                |
| ----------------------------------------------------------------------- | ---------------------------------------------------------------------- |
| [arg0](arg0/docs.md)                                                    | argv[0] dispatch for helper tools inside `nori`                        |
| [apply-patch](apply-patch/docs.md)                                      | Patch format parser and applier                                        |
| [ansi-escape](ansi-escape/docs.md) ([README](ansi-escape/README.md))    | ANSI escapes to Ratatui text                                           |
| [async-utils](async-utils/docs.md)                                      | Tokio helpers such as cancellable futures                              |
| [common](common/docs.md) ([README](common/README.md))                   | Shared CLI argument types and small helpers                            |
| [core](core/docs.md) ([README](core/README.md))                         | Inherited auth and compatibility code                                  |
| [file-search](file-search/docs.md) ([README](file-search/README.md))    | Fuzzy file finder                                                      |
| [stdio-to-uds](stdio-to-uds/docs.md) ([README](stdio-to-uds/README.md)) | Relay between stdio and a Unix domain socket                           |
| [utils](utils/docs.md) ([git README](utils/git/README.md))              | Small crates: git, cache, image, JSON to TOML, PTY, readiness, strings |

### Testing

| Docs                                     | Purpose                                                    |
| ---------------------------------------- | ---------------------------------------------------------- |
| [mock-acp-agent](mock-acp-agent/docs.md) | Deterministic ACP agent for integration tests              |
| [tui-pty-e2e](tui-pty-e2e/docs.md)       | End-to-end tests of the `nori` binary in a pseudo-terminal |

## Working in this workspace

- Build the shipped binary: `cargo build --bin nori`
- Test a crate: `cargo test -p <crate>`; e2e: `cargo test -p tui-pty-e2e`
  (requires `cargo build --bin nori` first)
- Format and lint: `just fmt` and `just fix -p <crate>` from this directory
- Contributor conventions live in the repo-root `AGENTS.md`; architecture
  docs in `docs.md` files beside the code

## Distribution

The `nori` binary is packaged by the npm launcher in `../nori-cli/` and
published as `nori-ai-cli`.
