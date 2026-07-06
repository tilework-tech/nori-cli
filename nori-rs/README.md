# nori-rs

The Rust workspace behind the `nori` binary — a multi-provider terminal AI
coding assistant. Nori is an ACP (Agent Client Protocol) host: it spawns
agents such as Claude Code, Codex, and Gemini as subprocesses and drives them
over JSON-RPC/stdio, presenting one terminal interface for all of them.

The workspace began as a fork of the OpenAI Codex CLI. The codex agent engine
has since been removed; every agent — including Codex — runs as an external
ACP subprocess. Remaining `codex-*` crates are inherited utility code being
progressively adopted or removed (see `docs/specs/crate-layering.md`).

## Key crates

| Crate | Purpose |
|-------|---------|
| `cli/` (`nori-cli`) | The shipped `nori` binary: dispatch and subcommands |
| `tui/` (`nori-tui`) | Ratatui interactive terminal interface |
| `harness/` (`nori-harness`) | Headless ACP session harness: session runtime, transcripts, hooks |
| `acp-host/` (`nori-acp-host`) | Agent-agnostic ACP hosting: subprocess spawn, wire client, registry |
| `nori-config/` | Nori config layer (`~/.nori/cli/config.toml`) |
| `nori-protocol/` | Session-runtime types over the ACP schema |
| `sandbox/` (`codex-sandbox`) | Sandboxed exec engine: Seatbelt, Landlock/seccomp, Windows restricted tokens |
| `installed/` (`nori-installed`) | Install detection and analytics |
| `mock-acp-agent/` | Mock ACP agent used by tests |
| `tui-pty-e2e/` | End-to-end PTY tests driving the real binary |

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
