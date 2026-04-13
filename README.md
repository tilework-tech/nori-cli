# Nori CLI

[![CI](https://github.com/tilework-tech/nori-cli/actions/workflows/rust-ci.yml/badge.svg?branch=main)](https://github.com/tilework-tech/nori-cli/actions/workflows/rust-ci.yml)
[![npm version](https://img.shields.io/npm/v/nori-ai-cli)](https://www.npmjs.com/package/nori-ai-cli)
[![License](https://img.shields.io/npm/l/nori-ai-cli)](https://github.com/tilework-tech/nori-cli/blob/main/LICENSE)
[![npm downloads](https://img.shields.io/npm/dm/nori-ai-cli)](https://www.npmjs.com/package/nori-ai-cli)

**One CLI, multiple AI providers.** Nori is a local AI coding agent that lets you switch between Claude, Gemini, and Codex. All from the same native CLI.

![Nori TUI Screenshot](https://raw.githubusercontent.com/tilework-tech/nori-cli/refs/heads/main/assets/nori-cli_2026-01-13.png)

## Install

```bash
npm install -g nori-ai-cli
```

Or download binaries from [GitHub Releases](https://github.com/tilework-tech/nori-cli/releases/latest).

## Quick Start

```bash
nori
```

That's it. The agent you choose will rely on existing auth if you have previously been using Claude Code, Codex, or Gemini on this system (and if not, login instructions are below). Nori launches an interactive TUI where you can chat, run commands, and let the AI assist with your codebase.

## Providers

Each provider you plan to use needs to be authenticated separately before use. Then switch between AI providers with the `/agent` command.

Currently each agent relies on an existing authenticated session on your system. If you're coming in from another CLI tool, great!
You should be good to go. If not, first follow the authentication for your desired provider:

| Provider | Authentication                                                                                                                                          |
| -------- | ------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Claude   | Run `npx @anthropic-ai/claude-code` in your terminal, then when the Claude CLI opens, type `/login` there.                                              |
| Gemini   | Run `npx @google/gemini-cli` in your terminal, then when the Gemini CLI opens, type `/auth` there.                                                      |
| OpenAI   | In Nori, use `/agent` to switch to Codex, then run `/login` inside the Nori interface. Nori will prompt you to install OpenAI via npm if needed.        |

## Bring Your Own Agent

Have your own ACP agent? Add it to Nori CLI! Any agent that speaks [Agent Client Protocol](https://github.com/agentclientprotocol/agent-client-protocol) over stdin/stdout can be registered in `~/.nori/cli/config.toml` and used alongside the built-in providers.

Add one or more `[[agents]]` entries to your config:

```toml
[[agents]]
name = "Mistral Vibe"
slug = "vibe-acp"

[agents.distribution.local]
command = "vibe-acp"

[[agents]]
name = "ElizACP"
slug = "elizacp"

[agents.distribution.local]
command = "elizacp"
args = ["acp"]
```

Then switch to your agent with `/agent` inside Nori.

**Example agents to try:**

| Agent | Install | Notes |
|-------|---------|-------|
| [Mistral Vibe](https://docs.mistral.ai/mistral-vibe/introduction/install) | `curl -LsSf https://mistral.ai/vibe/install.sh \| bash` | Installs both `vibe` and `vibe-acp`. Run `vibe --setup` to configure your API key. |
| [ElizACP](https://github.com/agentclientprotocol/symposium-acp) | `cargo install --locked elizacp` | Minimal Eliza chatbot, useful for testing. |
| [Kimi](https://github.com/nicepkg/kimi-cli) | No install needed — uses `uvx` | First-time auth: run `uvx --python 3.13 kimi-cli`, then `/login`. |

Want your AI agent to configure this automatically? Point it at the raw skill file: https://github.com/tilework-tech/nori-cli/blob/main/.claude/skills/registering-custom-acp-agent/SKILL.md

## Features

- **Multi-provider**: Anthropic's Claude Code, Google DeepMind's Gemini, and OpenAI's Codex
- **Improved terminal interface**: Fast incremental renders in Ratatui, double buffered scrollback history, and built in Rust for performance
- **Coming Soon!**
  - **Sandboxed execution**: Commands run in OS-level security sandboxes
  - **MCP integration**: Connect to Model Context Protocol servers for extended tools
  - **Session persistence**: Save and resume conversations with `nori resume`
  - **Multi-agent orchestration**: Alternate between multiple agent sessions

## Attribution

Nori CLI is built on the great work within [OpenAI Codex CLI](https://github.com/openai/codex).

Nori CLI is working with the great protocol led by [Zed Industries](https://github.com/agentclientprotocol/agent-client-protocol) for orchestrating agents.

## License

[Apache-2.0](LICENSE)
