# Noridoc: nori-acp-host

Path: @/nori-rs/acp-host

### Overview

- Client-side ACP hosting machinery, split out of the session harness (now `nori-harness`, then named `nori-acp`) during the crate-layering cleanup (`@/docs/specs/crate-layering.md`). It owns spawning an ACP agent subprocess and speaking JSON-RPC over its stdio (`connection/`, see `@/nori-rs/acp-host/src/connection/docs.md`), the agent registry and distribution resolution (`registry.rs`), the ACP-wire to internal-event bridge (`translator.rs`), file-change/diff helpers (`patch.rs`), and ACP error categorization (`error_category.rs`).
- This is a Layer-0 leaf inside the Nori workspace: it must stay independent of the session harness (`nori-harness`) and any terminal UI. It is agent-agnostic in that sense, but it is not yet a registry-neutral or package-ready generic ACP host crate because it still depends on Nori config types, Codex/Nori control-plane vocabulary, and the inherited MCP OAuth store.
- Deliberately harness-free: no session runtime, transcripts, hooks, or goal state. New agent-facing wire behavior belongs here; anything that needs backend session state belongs in `@/nori-rs/harness/`.

### How it fits into the larger codebase

```
nori-tui
    |
    v
nori-harness (session harness, re-exports this crate)
    |
    v
nori-acp-host <---> ACP Agent subprocess (JSON-RPC over stdio)
```

- `nori-harness` (`@/nori-rs/harness/`) is the primary consumer and re-exports every module (`pub use nori_acp_host::connection;` and friends in `@/nori-rs/harness/src/lib.rs`), so downstream consumers such as `@/nori-rs/tui/` import through `nori_harness` paths.
- Wire/schema types come from the official `agent-client-protocol` SDK; the schema's own `unstable` feature is enabled unconditionally for the Model-category config option.
- Depends on `codex-protocol` (`@/nori-rs/protocol/`) for the internal event vocabulary, `nori-config` (`@/nori-rs/nori-config/`) for agent/MCP/wire-proxy configuration types, and `codex-rmcp-client` (`@/nori-rs/rmcp-client/`) for OAuth token loading in `connection/mcp.rs`.
- Publishing this crate as an independently reusable ACP host is intentionally deferred until those boundary types are either adopted into Nori-owned public APIs or injected by callers.
- Error classification lives here (`AcpErrorCategory`, `categorize_acp_error`); the harness-side user-facing message composition (`enhanced_error_message`) stays in `@/nori-rs/harness/src/backend/`.

### Core Implementation

- `connection/` — `AcpConnection::spawn()` launches the agent as a child subprocess, owns its full lifecycle (exit watching, stderr tail, graceful stdin-EOF shutdown), forwards configured MCP servers (`mcp.rs`), optionally wraps the transport in an append-only wire logger (`wire_log.rs`), and delivers everything through one ordered `ConnectionEvent` inbox. See `@/nori-rs/acp-host/src/connection/docs.md`.
- `registry.rs` — data-driven agent registry merging built-in agents (Claude Code, Codex, Gemini) with custom `[[agents]]` config entries; resolves an agent slug to a spawnable `AcpAgentConfig` across npx/bunx/pipx/uvx/local distributions. The registry is process-global state (`AGENT_REGISTRY`, a `RwLock`) initialized once via `initialize_registry()` at startup, with a built-in-defaults fallback when uninitialized.
- `translator.rs` — converts user input into ACP `ContentBlock`s (text plus base64 image blocks) and provides local parsing/display helpers.
- `patch.rs` — diff/patch construction (`create_patch_with_context`) used to normalize file mutations for rendering and transcripts.
- `error_category.rs` — priority-chained substring matching (Auth > Quota > ExecutableNotFound > Initialization > PromptTooLong > ApiServerError > Unknown) over the Debug-formatted error chain; `is_retryable()` marks only server errors and quota limits as transient.

### Things to Know

- Detailed behavior documentation for the registry, error categorization, and their harness coupling still lives in `@/nori-rs/harness/docs.md`, which documents the full `nori-harness` public API (this crate's modules are part of that API via re-export).
- The connection layer's child-lifecycle invariants (ordered inbox, stdin-EOF-then-grace shutdown, exit-watcher ownership of the `Child`) are load-bearing for `nori cloud` session release; see `@/nori-rs/acp-host/src/connection/docs.md` before changing teardown behavior.
- `to_acp_mcp_servers()` in `connection/mcp.rs` is not a pure transformation: it eagerly resolves environment variables and loads stored OAuth tokens from the keyring/filesystem at conversion time.
- The dependency direction is `nori-harness -> nori-acp-host`, never the reverse. If a change here needs harness state (session runtime, transcript, goals), thread it in as a parameter or move the logic up to `@/nori-rs/harness/`.
- Treat new dependencies on `nori-config`, `codex-protocol`, or `codex-rmcp-client` as boundary debt unless the code is explicitly Nori-only.

Created and maintained by Nori.
