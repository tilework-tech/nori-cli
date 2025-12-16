# Nori CLI

[![CI](https://github.com/tilework-tech/nori-cli/actions/workflows/rust-ci.yml/badge.svg?branch=dev)](https://github.com/tilework-tech/nori-cli/actions/workflows/rust-ci.yml)
[![npm version](https://img.shields.io/npm/v/nori-ai-cli)](https://www.npmjs.com/package/nori-ai-cli)
[![npm downloads](https://img.shields.io/npm/dm/nori-ai-cli)](https://www.npmjs.com/package/nori-ai-cli)
[![License](https://img.shields.io/npm/l/nori-ai-cli)](https://github.com/tilework-tech/nori-cli/blob/dev/LICENSE)
[![GitHub Release](https://img.shields.io/github/v/release/tilework-tech/nori-cli)](https://github.com/tilework-tech/nori-cli/releases/latest)

A multi-provider AI coding agent that runs locally on your computer.

```bash
npm install -g nori-ai-cli
```

---

## Overview

Nori CLI is a fork of [OpenAI Codex CLI](https://github.com/openai/codex) with support for multiple AI providers. Switch between Claude, Gemini, and OpenAI seamlessly.

## Quickstart

### Installation

Install globally via npm:

```bash
npm install -g nori-ai-cli
```

For pre-release versions:

```bash
npm install -g nori-ai-cli@next
```

You can also download platform-specific binaries from the [GitHub Releases](https://github.com/tilework-tech/nori-cli/releases/latest) page:

- **macOS**: `nori-*-darwin-arm64.tar.gz` (Apple Silicon) or `nori-*-darwin-x86_64.tar.gz` (Intel)
- **Linux**: `nori-*-linux-arm64.tar.gz` (ARM64) or `nori-*-linux-x86_64.tar.gz` (x86_64)

### Running Nori

Simply run `nori` to get started:

```bash
nori
```

## Supported Providers

Nori supports multiple AI providers via the Agent Context Protocol:

| Provider | Model            | Setup                                           |
| -------- | ---------------- | ----------------------------------------------- |
| Claude   | Anthropic Claude | `npx @zed-industries/claude-code-acp` (default) |
| Gemini   | Google Gemini    | `npx @google/gemini-cli --experimental-acp`     |
| Codex    | OpenAI           | `npx @zed-industries/codex-acp`                 |

Switch providers during a session with the `/agent` command.

## Key Features

- **Multi-Provider Support**: Switch between Claude, Gemini, and OpenAI via ACP
- **MCP Integration**: Connect to Model Context Protocol servers for extended capabilities
- **Sandboxed Execution**: Commands run in a security sandbox (Seatbelt on macOS, Landlock on Linux)
- **Session Management**: Save and resume conversations

## Configuration

Configuration is stored in `~/.codex/`:

- `config.toml` - Main configuration file
- `auth.json` - Authentication tokens
- `sessions/` - Saved conversations

## Attribution

Nori CLI is a fork of [OpenAI Codex CLI](https://github.com/openai/codex), extended for multi-provider AI assistance.

## License

This repository is licensed under the [Apache-2.0 License](LICENSE).
