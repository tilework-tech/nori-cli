# CURRENT PROGRESS

## 2026-06-05

- Started continuing the `nori-client` MCP objective on branch `feat/nori-client-mcp-cleanup`.
- Decision: created this root-level append-only log because this branch did not have an active `CURRENT-PROGRESS.md`; only older spec/worktree copies existed.
- Decision: next slice is to prove and implement the MCP resource/prompt discovery surface from `docs/followups/nori-client-mcp.md`, starting at the real streamable-HTTP MCP client boundary.
- Red/green result: added real rmcp streamable-HTTP client tests for listing/reading Nori context resources and listing/getting workflow prompts; tests failed with empty lists before implementation and pass after adding the surface.
- Decision: extracted the fixed resource/prompt catalog into `nori_client_context.rs` so `nori_client_mcp.rs` remains focused on transport, goal tools, and session capability wiring.
- Open question: the exact prose in the curated resources/prompts may need product review; I kept it compact and aligned to the behavior spec rather than trying to encode every workflow detail.
- Verification: `cargo test -p nori-acp`, focused `cargo test -p nori-acp nori_client_mcp -- --nocapture`, `just fmt`, `env -u RUSTC_WRAPPER just fix -p nori-acp`, `cargo build --bin nori`, `cargo test -p tui-pty-e2e`, and an isolated tmux TUI run with ElizACP all passed.
- Tooling note: `just fix -p nori-acp` failed through `sccache` even with escalation; rerunning with `RUSTC_WRAPPER` unset completed successfully.
