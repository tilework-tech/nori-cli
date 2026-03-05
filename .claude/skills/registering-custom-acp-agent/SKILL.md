---
name: Registering Custom ACP Agents
description: Use when the user wants to register a custom ACP agent in Nori, or try one of the example agents (elizacp, kimi-cli, opencode)
---

<required>
*CRITICAL* Add the following steps to your Todo list using TodoWrite:

1. Ask the user: custom agent setup or example demo?
2. If custom agent: gather agent details and write config
3. If example: let user choose (elizacp, kimi-cli, opencode) and walk through setup
4. Write the `[[agents]]` entry to `~/.nori/cli/config.toml`
5. Verify the agent appears in Nori's agent picker
</required>

# Overview

Nori supports registering custom ACP (Agent Client Protocol) agents via `~/.nori/cli/config.toml`. Once registered, agents appear in the agent picker and can be used like any built-in agent. All ACP agents communicate over JSON-RPC 2.0 via stdin/stdout -- they are spawned as subprocesses.

# Step 1: Determine What the User Wants

Ask the user:

> Would you like to:
> 1. **Register your own custom agent** -- I'll walk you through the config fields
> 2. **Try an example agent** -- Choose from elizacp, kimi-cli, or opencode

# Step 2a: Custom Agent Setup

Gather the following from the user:

| Field | Required | Description |
|-------|----------|-------------|
| `name` | Yes | Display name in the agent picker (e.g. "My Agent") |
| `slug` | Yes | Machine identifier, used as CLI arg (e.g. "my-agent") |
| `distribution` | Yes | How to invoke the agent (see variants below) |
| `context_window_size` | No | Context window size in tokens |
| `auth_hint` | No | Message shown on auth failures (e.g. "Set MY_API_KEY") |
| `transcript_base_dir` | No | Transcript directory relative to home |

## Distribution Variants

Exactly one must be specified:

### Local Binary

```toml
[agents.distribution.local]
command = "/path/to/agent"   # or just "agent" if in PATH
args = ["--acp"]             # optional arguments
env = { "KEY" = "value" }    # optional environment variables
```

### Package Manager Distributions

```toml
# npx (npm)
[agents.distribution.npx]
package = "@scope/agent-pkg"
args = ["acp"]

# bunx (bun)
[agents.distribution.bunx]
package = "@scope/agent-pkg"
args = ["acp"]

# pipx (Python)
[agents.distribution.pipx]
package = "agent-pkg"
args = ["acp"]

# uvx (uv/Python)
[agents.distribution.uvx]
package = "agent-pkg"
args = ["acp"]
```

# Step 2b: Example Agent Walkthroughs

Ask the user which example they want to try:

## Option A: elizacp (Rust/Cargo)

elizacp is a minimal Eliza chatbot that implements ACP. Great for testing.

**Install:**
```bash
cargo install --git https://github.com/agentclientprotocol/symposium-acp elizacp
```

> Note: There is no `cargo` distribution variant. Use `local` for cargo-installed binaries since they end up in your PATH.

**Config to add to `~/.nori/cli/config.toml`:**
```toml
[[agents]]
name = "ElizACP"
slug = "elizacp"

[agents.distribution.local]
command = "elizacp"
```

## Option B: kimi-cli (Python/uv)

Moonshot AI's CLI coding agent with native ACP support.

**First-time setup:** Before using kimi-cli through Nori, run `uvx --python 3.13 kimi-cli` in a terminal and use `/login` to authenticate, then `/setup` to initialize.

The `uvx` distribution runs the package on-the-fly, so no separate install step is needed.

**Config to add to `~/.nori/cli/config.toml`:**
```toml
[[agents]]
name = "Kimi"
slug = "kimi"
context_window_size = 128000
auth_hint = "Run 'uvx --python 3.13 kimi-cli' and use /login to authenticate"

[agents.distribution.uvx]
package = "kimi-cli"
args = ["acp"]
```

## Option C: opencode (Local installer)

An open-source AI coding agent with full ACP support.

**Install (pick one):**
```bash
# Quick install
curl -fsSL https://opencode.ai/install | bash

# Or via npm
npm install -g opencode-ai

# Or via Homebrew
brew install anomalyco/tap/opencode
```

**Config to add to `~/.nori/cli/config.toml`:**
```toml
[[agents]]
name = "OpenCode"
slug = "opencode"

[agents.distribution.local]
command = "opencode"
args = ["acp"]
```

# Step 3: Write the Config

Use the Read tool to check if `~/.nori/cli/config.toml` exists and read its contents.

- If the file exists, use the Edit tool to append the `[[agents]]` block.
- If the file does not exist, use the Write tool to create it with the `[[agents]]` block.

**Important:** Do not overwrite existing content. Append the new agent entry.

# Step 4: Verify

Tell the user to launch Nori and check the agent picker via the `/agent` command. The new agent should appear in the list.

Custom agents always appear as "installed" in the picker (no pre-check is done). If the binary is actually missing, the error occurs when Nori tries to spawn the subprocess, not at selection time. The error message includes an install hint derived from the distribution type.

# Notes

- Custom agents override built-in agents if they share the same slug.
- Duplicate slugs among custom agents are rejected.
- All ACP agents communicate via JSON-RPC 2.0 over stdin/stdout (no ports to configure).
