# Nori CLI

[![CI](https://github.com/tilework-tech/nori-cli/actions/workflows/rust-ci.yml/badge.svg?branch=main)](https://github.com/tilework-tech/nori-cli/actions/workflows/rust-ci.yml)
[![npm version](https://img.shields.io/npm/v/nori-ai-cli)](https://www.npmjs.com/package/nori-ai-cli)
[![License](https://img.shields.io/npm/l/nori-ai-cli)](https://github.com/tilework-tech/nori-cli/blob/main/LICENSE)
[![npm downloads](https://img.shields.io/npm/dm/nori-ai-cli)](https://www.npmjs.com/package/nori-ai-cli)

Nori brings Claude Code, Codex, Antigravity, and Grok Build into one terminal. You can select OpenCode, Cursor Agent, or Hermes Agent. You can also bring your own ACP agent.

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

Use `/agent` to switch agents, or select one at launch with `nori --agent grok-build`. To start a session on a running Nori host, use `nori host new --agent grok-build`. Aliases use the same provider ID in the session list and share its saved model. An exact `[[agents]]` slug keeps its own settings.

## Providers

Claude Code and Codex use `npx` or `bunx` to resolve the upstream `@latest` release. Grok Build uses `npx` so its native executable is installed by the upstream npm package. The adapters use their upstream dependencies and authentication settings. For Antigravity, install Google's CLI and make `agy` available on the host's `PATH`. [Antigravity CLI installation](https://antigravity.google/cli)

| Agent | ID for `--agent` or `config.toml` | ACP launch | Sign in |
| --- | --- | --- | --- |
| Claude Code | `claude-code` | `@agentclientprotocol/claude-agent-acp@latest` | Use your Claude Code login or set `ANTHROPIC_API_KEY`. |
| Codex | `codex` | `@agentclientprotocol/codex-acp@latest` | Use `/login` for ChatGPT or set `OPENAI_API_KEY`. |
| Antigravity | `gemini` (also `antigravity` or `agy`) | `agy acp --disable-telemetry` | Open `agy` for Google sign-in or set `GEMINI_API_KEY`. |
| Grok Build | `grok-build` (also `grok`) | `@xai-official/grok@latest agent stdio` | Use `/login` for the Grok sign-in command or set `XAI_API_KEY`. |
| Cursor Agent | `cursor` (also `cursor-agent`) | `cursor-agent acp` if available, otherwise `agent acp` | Use your existing `agent login` session or export a Cursor-issued user API key as `CURSOR_API_KEY`. |
| Hermes Agent | `hermes` (also `hermes-agent`) | `hermes acp` | Run `hermes model` in another terminal to choose a provider and configure credentials. |

Install Cursor CLI on the machine that runs Nori. To use an API key with a local host, export `CURSOR_API_KEY` before starting `nori host`. Then use `nori host new --agent cursor`. With multi-session enabled, use `/delegate cursor <task>` to open a Cursor child from another agent. [Cursor CLI installation](https://cursor.com/docs/cli/installation) · [Cursor CLI authentication](https://cursor.com/docs/cli/reference/authentication)

OpenCode is also built in. Install OpenCode 2 on the host's `PATH` and use `nori --agent opencode` or `nori host new --agent opencode`. With `[features] multi_session = true`, try `/delegate opencode Summarize the README` from another agent. All three launch `opencode acp`. OpenCode reads its own provider configuration and credentials. To connect an account, run `opencode auth login` in a terminal. [Model selection and ChatGPT-plan restrictions](nori-rs/config.md#default-models-default_models) · [OpenCode ACP guide](https://opencode.ai/docs/cli/acp/)

The `gemini` ID keeps existing configs and Nori history associated with the Google provider. Each agent supplies its own model choices through `/model`.

### Hermes Agent

Install Hermes and its ACP extra on the machine running Nori. Make `hermes` available on that machine’s `PATH` and run `hermes acp --check`. [Nous Research’s ACP installation guide for v0.23.3](https://github.com/NousResearch/hermes-agent/blob/v0.23.3/website/docs/user-guide/features/acp.md#installation)

Start it with `nori --agent hermes` or create a session on a running host with `nori host new --agent hermes`. With `features.multi_session = true` in your Nori configuration, enter `/delegate hermes Read README.md and summarize the setup steps` from another agent’s conversation. Approve the child when prompted. Its result returns to the parent conversation.

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

| Agent                                                                     | Install                                                 | Notes                                                                              |
| ------------------------------------------------------------------------- | ------------------------------------------------------- | ---------------------------------------------------------------------------------- |
| [Mistral Vibe](https://docs.mistral.ai/mistral-vibe/introduction/install) | `curl -LsSf https://mistral.ai/vibe/install.sh \| bash` | Installs both `vibe` and `vibe-acp`. Run `vibe --setup` to configure your API key. |
| [ElizACP](https://github.com/agentclientprotocol/symposium-acp)           | `cargo install --locked elizacp`                        | Minimal Eliza chatbot, useful for testing.                                         |
| [Kimi](https://github.com/nicepkg/kimi-cli)                               | No install needed — uses `uvx`                          | First-time auth: run `uvx --python 3.13 kimi-cli`, then `/login`.                  |

Want your AI agent to configure this automatically? Point it at the raw skill file: https://github.com/tilework-tech/nori-cli/blob/main/.claude/skills/registering-custom-acp-agent/SKILL.md

## Features

- **Multi-provider**: Switch between built-in and custom ACP agents from the same CLI.
- **Fast native interface**: Incremental Ratatui rendering, double-buffered scrollback history, and a Rust core keep long sessions responsive.
- **Solid ACP client support**: Nori supports ACP session listing, loading, resume, context and usage metadata.
- **Agent Integrations**: Your agents keep their configurations, whether that's slash commands, skills, and MCP server forwarding.
- **Session continuity**: Pick up prior work with resume, and rewind agent changes with escape-to-undo.
- **Project context**: Reference files with `@`, flip between an editor or a file picker, and keep useful context visible without leaving the terminal.

See the [ACP roadmap](ROADMAP.md) for what is done, what is in progress, and what is planned next.

## Attribution

Nori CLI is built on the great work within [OpenAI Codex CLI](https://github.com/openai/codex).

Nori CLI relies on the great contributions within the [ACP project](https://github.com/agentclientprotocol/agent-client-protocol), from Zed Editor, JetBrains, and others.

## License

[Apache-2.0](LICENSE)
