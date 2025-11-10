# Nori CLI - Agent Router TUI

A terminal user interface (TUI) for routing prompts to different AI coding agents: Claude Code and GPT Codex.

## Features

- **Agent Selection**: Choose between Claude Code and GPT Codex
- **Interactive Prompts**: Multi-line text input for complex prompts
- **Streaming Responses**: Real-time display of agent responses as they stream
- **Event Visibility**: See file changes, command executions, and agent messages
- **Async Architecture**: Built with Tokio for responsive UI and concurrent subprocess management

## Prerequisites

Before using nori-cli, you must have the following installed:

### Claude Code CLI

Install from [code.claude.com](https://code.claude.com/)

Verify installation:
```bash
claude --version
```

### GPT Codex CLI

Install from [developers.openai.com/codex/cli](https://developers.openai.com/codex/cli/)

Verify installation:
```bash
codex --version
```

### Authentication

- **Claude Code**: Set up your API key following [Claude Code authentication docs](https://code.claude.com/docs/en/setup)
- **Codex**: Login with `codex login` or set `CODEX_API_KEY` environment variable

## Installation

```bash
cargo build --release
```

The binary will be at `target/release/nori-cli`

## Usage

Run the TUI:
```bash
cargo run
# or
./target/release/nori-cli
```

### Navigation

**Selection Mode** (choosing an agent):
- `↑`/`↓` or `k`/`j`: Navigate agent list
- `Enter`: Select agent
- `q`: Quit

**Input Mode** (entering prompt):
- Type your prompt (supports multi-line)
- `Ctrl+Enter`: Submit prompt to agent
- `Esc`: Go back to selection

**Streaming Mode** (viewing response):
- Watch the streaming response
- `Esc`: Return to selection (interrupts current stream)

### Response Format

The TUI displays different event types from the agents:

- `[agent_message]`: Text responses from the agent
- `[file_change]`: File modifications made by the agent
- `[command]`: Shell commands executed by the agent

## Architecture

### Tech Stack

- **ratatui 0.29**: Terminal UI framework
- **tokio**: Async runtime for subprocess management
- **tui-textarea**: Multi-line text input widget
- **crossterm**: Terminal manipulation
- **serde_json**: JSON parsing for agent events

### Design Pattern

Uses The Elm Architecture (TEA):
- `Model`: Application state
- `Message`: Events/actions
- `update()`: State transitions
- `render()`: UI rendering

### Subprocess Integration

Agents run as child processes:
- **Claude Code**: `claude --print --output-format stream-json`
- **Codex**: `codex exec --json`

Events are parsed from newline-delimited JSON (JSONL) output and streamed to the TUI in real-time.

## Current Limitations

- Session history is not persisted across runs
- No conversation context between prompts (each prompt is independent)
- Session resumption is prepared but not yet wired up
- Error messages from subprocess failures need better formatting
- No configuration file support (uses hardcoded defaults)

## Future Enhancements

- Persistent session resumption across TUI restarts
- Multi-turn conversations with context
- Configuration file for agent settings
- Better error handling and recovery
- Process cancellation (kill subprocess on Esc)
- Scrollable response view for long outputs

## Development

Run tests:
```bash
cargo test
```

The test suite uses mock backends (subprocess with `printf`) to avoid requiring actual CLI installations during testing.

## Troubleshooting

**"No such file or directory" when selecting an agent:**
- Ensure `claude` or `codex` is in your PATH
- Verify CLI is installed with `which claude` or `which codex`

**Empty response / no streaming:**
- Check authentication (API keys)
- Run the CLI directly to verify it works: `claude --print "test"` or `codex exec "test"`

**TUI freezes:**
- Press `Esc` to return to selection
- If unresponsive, `Ctrl+C` to force quit

## License

MIT