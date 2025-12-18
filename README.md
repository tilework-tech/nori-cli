# Nori CLI

[![CI](https://github.com/tilework-tech/nori-cli/actions/workflows/rust-ci.yml/badge.svg?branch=dev)](https://github.com/tilework-tech/nori-cli/actions/workflows/rust-ci.yml)
[![npm version](https://img.shields.io/npm/v/nori-ai-cli)](https://www.npmjs.com/package/nori-ai-cli)
[![License](https://img.shields.io/npm/l/nori-ai-cli)](https://github.com/tilework-tech/nori-cli/blob/dev/LICENSE)

**One CLI, multiple AI providers.** Nori is a local AI coding agent that lets you switch between Claude, Gemini, and Codex. All from the same native CLI.

<!-- TODO: Add TUI screenshot here -->
<!-- ![Nori TUI Screenshot](assets/screenshot.png) -->

## Install

```bash
npm install -g nori-ai-cli
```

Or download binaries from [GitHub Releases](https://github.com/tilework-tech/nori-cli/releases/latest).

## Quick Start

```bash
nori
```

That's it. Nori launches an interactive TUI where you can chat, run commands, and let the AI assist with your codebase.

## Providers

Switch between AI providers with the `/agent` command:

| Provider | Command |
|----------|---------|
| Claude | `npx @zed-industries/claude-code-acp` (default) |
| Gemini | `npx @google/gemini-cli --experimental-acp` |
| OpenAI | `npx @zed-industries/codex-acp` |

## Features

- **Multi-provider**: Anthropic's Claude Code, Google DeepMind's Gemini, and OpenAI's Codex
- **Sandboxed execution**: Commands run in OS-level security sandboxes
- **Coming Soon!**
    - **MCP integration**: Connect to Model Context Protocol servers for extended tools
    - **Session persistence**: Save and resume conversations with `nori resume`
    - **Multi-agent orchestration**: Alternate between multiple agent sessions

## Attribution

Nori CLI is built on the great work within [OpenAI Codex CLI](https://github.com/openai/codex).

## License

[Apache-2.0](LICENSE)
