# Noridoc: mock-acp-agent

Path: @/codex-rs/mock-acp-agent

### Overview

The mock-acp-agent crate provides a mock ACP agent binary for testing the Nori TUI. It simulates an AI agent's behavior including streaming responses, tool calls, and permission requests.

### How it fits into the larger codebase

Used by `@/codex-rs/tui-pty-e2e/` for end-to-end integration testing. The mock agent is spawned as a subprocess and communicates over stdin/stdout using the ACP protocol.

### Core Implementation

**MockAgent**: Implements the ACP `Agent` trait, handling:
- Session creation/destruction
- Prompt processing with simulated responses
- Tool call execution (shell, apply_patch, etc.)
- Permission request/response flow
- Cancellation

**Mock Behaviors**: The agent recognizes special prompts to trigger specific behaviors:
- Text streaming with configurable delays
- Tool calls that request permissions
- File read/write operations
- Error simulation

**Client Requests**: Outbound requests to the client:
- `ReadFile` - Request file contents
- `WriteFile` - Request file write
- `RequestPermission` - Ask user for tool approval

### Things to Know

- The mock is a binary crate (no lib.rs) intended only for testing
- Uses the same ACP protocol as real agents for realistic testing
- Simulates streaming with configurable chunk delays
- Supports permission options (accept, deny, skip)
- Session state is tracked per-session ID

Created and maintained by Nori.
