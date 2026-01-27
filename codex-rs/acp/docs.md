# Noridoc: codex-acp

Path: @/codex-rs/acp

### Overview

The ACP crate implements the Agent Client Protocol integration for Nori. It manages spawning ACP-compliant agent subprocesses (like Claude Code, Codex, or Gemini), communicating with them over JSON-RPC, and translating between ACP protocol messages and Codex internal protocol types.

### How it fits into the larger codebase

```
nori-tui
    |
    v
codex-acp <---> ACP Agent subprocess (claude-code-acp, codex-acp, gemini-cli)
    |
    v
codex-protocol (internal event types)
```

The ACP crate serves as a bridge between:
- The TUI layer (`@/codex-rs/tui/`) which displays UI and collects user input
- External ACP agent processes installed via npm (@anthropic-ai/claude-code, @openai/codex, @google/gemini-cli)

Key files:
- `registry.rs` - Agent configuration and npm package detection
- `connection.rs` - Subprocess spawning and JSON-RPC communication
- `translator.rs` - Protocol translation between ACP and Codex types
- `backend.rs` - Implements `ConversationClient` trait from codex-core

### Core Implementation

**Agent Registry** (`registry.rs`): Defines supported agents (`AgentKind::ClaudeCode`, `AgentKind::Codex`, `AgentKind::Gemini`) and their npm package names. Provides detection of installed agents via `npx` availability checks.

**Connection Management** (`connection.rs`): Each ACP session runs in a dedicated single-threaded runtime because the ACP library uses `!Send` futures. Commands flow through channels:

| Command | Purpose |
|---------|---------|
| `CreateSession` | Initialize a new agent session with working directory |
| `Prompt` | Send user content and receive streaming updates |
| `Cancel` | Cancel an in-progress prompt |
| `SetModel` | Switch models (unstable feature) |

**Protocol Translation** (`translator.rs`): Converts between:
- ACP `ContentBlock` <-> Codex `ResponseItem`
- ACP `PermissionRequest` -> Codex approval events (`ExecApprovalRequestEvent`, `ApplyPatchApprovalRequestEvent`)
- User `ReviewDecision` -> ACP `PermissionOption`

**Backend Implementation** (`backend.rs`): Implements `ConversationClient` trait, routing operations through the ACP connection. Handles session lifecycle, message accumulation, and turn tracking.

### Things to Know

- Agent subprocess communication uses stdin/stdout with JSON-RPC 2.0 framing
- The minimum supported ACP protocol version is V1
- The `unstable` feature gates model switching functionality
- Approval requests are translated to use appropriate UI (exec approval for shell commands, patch approval for file edits)
- A `DRAIN_YIELD_COUNT` of 10 yields allows pending notifications to drain before session cleanup
- Config loading uses Nori-specific paths (`~/.nori/cli/config.toml`) when the `nori-config` feature is enabled in the TUI

Created and maintained by Nori.
