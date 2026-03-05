---
name: registering-custom-acp-agent
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

Nori supports registering custom ACP agents via `~/.nori/cli/config.toml`. All ACP agents communicate over JSON-RPC 2.0 via stdin/stdout (spawned as subprocesses).

# Custom Agent Setup

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

```toml
# Local binary (or anything in PATH)
[agents.distribution.local]
command = "/path/to/agent"
args = ["--acp"]             # optional
env = { "KEY" = "value" }    # optional

# Package managers: npx, bunx, pipx, uvx (same shape)
[agents.distribution.uvx]   # or .npx / .bunx / .pipx
package = "agent-pkg"
args = ["acp"]
```

# Example Agents

## elizacp (Rust/Cargo)

Minimal Eliza chatbot. Install: `cargo install --git https://github.com/agentclientprotocol/symposium-acp elizacp`

No `cargo` distribution variant exists -- use `local` since cargo puts binaries in PATH.

```toml
[[agents]]
name = "ElizACP"
slug = "elizacp"

[agents.distribution.local]
command = "elizacp"
```

## kimi-cli (Python/uv)

Moonshot AI's coding agent. No install needed -- `uvx` runs on-the-fly. First-time auth: run `uvx --python 3.13 kimi-cli`, then `/login` and `/setup`.

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

## opencode

Install: `curl -fsSL https://opencode.ai/install | bash` (or `npm install -g opencode-ai` / `brew install anomalyco/tap/opencode`)

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
