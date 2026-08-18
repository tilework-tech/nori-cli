# Noridoc: nori-acp-host

Path: @/nori-rs/acp-host

### Overview

- Agent-agnostic, client-side ACP hosting machinery, split out of the session harness (now `nori-harness`, then named `nori-acp`) during the crate-layering cleanup (`@/docs/specs/crate-layering.md`). It owns spawning an ACP agent subprocess and speaking JSON-RPC over its stdio (`connection/`, see `@/nori-rs/acp-host/src/connection/docs.md`), the agent registry and distribution resolution (`registry.rs`), the ACP-wire to internal-event bridge (`translator.rs`), file-change/diff helpers (`patch.rs`), and ACP error categorization (`error_category.rs`).
- One deliberate exception to agent-agnosticism: `claude_models/` (see `@/nori-rs/acp-host/src/claude_models/docs.md`) is Claude-specific spawn-time machinery that widens the model list the Claude ACP adapter advertises. It is quarantined to its own module and hooked in from a single branch in `connection/acp_connection.rs`.
- This is a Layer-0 leaf of the crate layering: it must stay independent of the session harness (`nori-harness`) and any terminal UI so it remains usable and testable by other ACP-ecosystem projects.
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

- `nori-harness` (`@/nori-rs/harness/`) is the primary consumer and re-exports every public module (`pub use nori_acp_host::connection;` and friends in `@/nori-rs/harness/src/lib.rs`), so downstream consumers such as `@/nori-rs/tui/` import through `nori_harness` paths. Crate-private modules like `claude_models/` are invisible above this layer -- their effect reaches the TUI only through what the agent ends up advertising on the wire.
- Wire/schema types come from the official `agent-client-protocol` SDK. The schema's `unstable` umbrella feature is enabled unconditionally, but **not** for the Model-category config option: `SessionConfigOptionCategory` is ungated and its `Model` variant is always available (only the sibling `ModelConfig` variant sits behind `unstable_model_config_category`). What actually requires the feature is `SessionConfigOptionValue`, the payload type `set_config_option()` constructs, which is gated behind `unstable_boolean_config`.
- Depends on `codex-protocol` (`@/nori-rs/protocol/`) for the internal event vocabulary, `nori-config` (`@/nori-rs/nori-config/`) for agent/MCP/wire-proxy configuration types, and `codex-rmcp-client` (`@/nori-rs/rmcp-client/`) for OAuth token loading in `connection/mcp.rs`.
- `reqwest` is the crate's only outbound HTTP client and exists solely for `claude_models/`; everything else here talks to the agent subprocess over stdio.
- Error classification lives here (`AcpErrorCategory`, `categorize_acp_error`); the harness-side user-facing message composition (`enhanced_error_message`) stays in `@/nori-rs/harness/src/backend/`.

### Core Implementation

- `connection/` — `AcpConnection::spawn()` launches the agent as a child subprocess, owns its full lifecycle (exit watching, stderr tail, graceful stdin-EOF shutdown), forwards configured MCP servers (`mcp.rs`), optionally wraps the transport in an append-only wire logger (`wire_log.rs`), and delivers everything through one ordered `ConnectionEvent` inbox. See `@/nori-rs/acp-host/src/connection/docs.md`.
- `registry.rs` — data-driven agent registry merging built-in agents (Claude Code, Codex, Gemini) with custom `[[agents]]` config entries; resolves an agent slug to a spawnable `AcpAgentConfig` across npx/bunx/pipx/uvx/local distributions. The registry is process-global state (`AGENT_REGISTRY`, a `RwLock`) initialized once via `initialize_registry()` at startup, with a built-in-defaults fallback when uninitialized.
- `translator.rs` — converts user input into ACP `ContentBlock`s (text plus base64 image blocks) and provides local parsing/display helpers.
- `patch.rs` — diff/patch construction (`create_patch_with_context`) used to normalize file mutations for rendering and transcripts.
- `error_category.rs` — priority-chained substring matching (Auth > Quota > ExecutableNotFound > Initialization > PromptTooLong > ApiServerError > Unknown) over the Debug-formatted error chain; `is_retryable()` marks only server errors and quota limits as transient.
- `claude_models/` — crate-private, Claude-only, unix-only. At spawn it fetches Anthropic's published model id list, filters out retired and duplicate-dated ids, and writes a `claude` wrapper into `$NORI_HOME/cache` that injects the widened list via `--settings`. Every failure path — including running on a non-unix platform, where no safe wrapper strategy exists — resolves to "no injection", leaving the adapter's own list intact.

### Things to Know

- Detailed behavior documentation for the registry, error categorization, and their harness coupling still lives in `@/nori-rs/harness/docs.md`, which documents the full `nori-harness` public API (this crate's modules are part of that API via re-export).
- The connection layer's child-lifecycle invariants (ordered inbox, stdin-EOF-then-grace shutdown, exit-watcher ownership of the `Child`) are load-bearing for `nori cloud` session release; see `@/nori-rs/acp-host/src/connection/docs.md` before changing teardown behavior.
- `to_acp_mcp_servers()` in `connection/mcp.rs` is not a pure transformation: it eagerly resolves environment variables and loads stored OAuth tokens from the keyring/filesystem at conversion time.
- The dependency direction is `nori-harness -> nori-acp-host`, never the reverse. If a change here needs harness state (session runtime, transcript, goals), thread it in as a parameter or move the logic up to `@/nori-rs/harness/`.
- Agent-specific behavior is a smell here but occasionally unavoidable when an adapter's own defaults are too narrow (`claude_models/`). The containment rule is: keep it in a dedicated module, gate it on `AgentKind`, and make every failure degrade to the agent's unmodified behavior rather than a nori-specific one.
- `$NORI_HOME/cache` is this crate's only on-disk state. It holds derived, regenerable artifacts (the fetched model catalog and the generated Claude wrapper plus its settings file); nothing here is user data and deleting it only costs one refetch.
- Those paths are **not per-session** — every concurrent nori process writes the same files. The generated wrapper and its settings file are therefore installed by atomic rename and skipped when the content already matches, because a truncating write over a file another session is executing fails that session's agent spawn with `ETXTBSY`. Any future cache artifact added here inherits that constraint.

Created and maintained by Nori.
