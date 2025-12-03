# Noridoc: protocol

Path: @/codex-rs/protocol

### Overview

The `codex-protocol` crate defines the core message types, data structures, and protocol definitions shared across all Codex components. It serves as the canonical source for events, operations, model types, configuration enums, and user input structures.

### How it fits into the larger codebase

Protocol is a foundational dependency used by nearly every crate:

- **Core** re-exports protocol types as `codex_core::protocol`
- **TUI/Exec** use `Event`, `Op`, `EventMsg` for conversation communication
- **App Server** references protocol types for JSON-RPC messages
- **All crates** use `ConversationId`, content types, and config enums

This separation ensures consistent type definitions without circular dependencies.

### Core Implementation

**Key Modules:**

| Module | Contents |
|--------|----------|
| `protocol` | `Event`, `Op`, `EventMsg`, `AskForApproval`, session types, turn lifecycle |
| `models` | `ResponseItem`, `ContentItem`, `LocalShellAction`, etc. |
| `config_types` | `SandboxMode`, `TrustLevel`, model settings |
| `user_input` | `UserInput` variants (text, image, file) |
| `items` | Item types for conversation history |
| `account` | Account-related types |
| `approvals` | Approval request/response structures |
| `custom_prompts` | Custom system prompt definitions |
| `plan_tool` | Plan tool specific types |

**Core Types:**

```rust
// Operation sent to conversation
pub enum Op {
    UserTurn { items, cwd, approval_policy, ... },
    Interrupt,
    Shutdown,
    // ...
}

// Event received from conversation
pub struct Event {
    pub id: String,
    pub msg: EventMsg,
}

pub enum EventMsg {
    SessionConfigured { ... },
    TurnStart { ... },
    Delta { ... },
    TurnComplete { ... },
    Error { ... },
    ShutdownComplete,
    // ...
}
```

**Content Types:**

`ContentItem` represents message content:
- Text
- Image (base64 or URL)
- Tool calls and results

`ResponseItem` wraps model response items with metadata.

### Things to Know

**ConversationId:**

The `ConversationId` type (in `conversation_id.rs`) is a wrapper around UUID used to identify sessions. It provides string conversion and validation.

**Approval Policy:**

`AskForApproval` enum controls when user confirmation is required:
- `Always`: Every action
- `OnRequest`: User decides per-request
- `Never`: Fully autonomous (for automation)

**Sandbox Modes:**

`SandboxMode` in `config_types`:
- `ReadOnly`: No writes allowed
- `WorkspaceWrite`: Writes to cwd only
- `DangerFullAccess`: No restrictions

**Number Formatting:**

`num_format.rs` provides locale-aware number formatting for token counts and statistics display.

**Parse Command:**

`parse_command.rs` contains utilities for parsing shell command strings.

Created and maintained by Nori.
