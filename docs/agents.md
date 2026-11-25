# Agents

This reference lists the agents the Nori CLI can run, how it starts each
one, and how you change the launch command, the adapter version, and the
model. Terms follow the [glossary](../GLOSSARY.md). The mapping between CLI
agent IDs and Sessions harness IDs is in the monorepo glossary
(`nori-monorepo: GLOSSARY.md`, section "Agent IDs"). This page does not
repeat it.

An **agent** is the ACP agent server that Nori starts for a session. It is a
harness with native ACP, or a harness behind an ACP adapter. Nori talks ACP v1
to it over the child's stdin and stdout.

## Choose an agent

| Where | How | Default |
|---|---|---|
| `config.toml` | `agent = "<id>"` | `claude-code` |
| Command line | `--agent <id>` on `nori`, `nori exec`, `nori host`, and `nori host new`. `-a` is the short form on `nori` and `nori host`. | the `agent` key |
| TUI | `/agent` | the `agent` key |
| `nori host` client | `_meta["nori/agent"]` on `session/new` | the host's agent |

- `--agent` changes the agent for this run only. It does not change the
  `agent` key.
- IDs are case-insensitive. An alias resolves to its agent ID.
- A configured `[[agents]]` slug wins over a built-in alias with the same
  spelling.
- An unknown ID fails the session start. On `nori host`, `session/new` gets
  `-32602`.

For every flag, see [commands](commands.md). For the host wire rules, see
`nori-monorepo: docs/specs/nori-cli/host-contract.md`.

## Built-in agents

| ID | Name | Aliases | Agent server | ACP | Adapter package |
|---|---|---|---|---|---|
| `claude-code` | Claude Code | `claude`, `claude-acp`, `claude-4.5` | `<runner> @agentclientprotocol/claude-agent-acp@latest` | Adapter | `@agentclientprotocol/claude-agent-acp` |
| `codex` | Codex | `codex-acp` | `<runner> @agentclientprotocol/codex-acp@latest` | Adapter | `@agentclientprotocol/codex-acp` |
| `gemini` | Antigravity | `gemini-acp`, `gemini-2.5-flash`, `antigravity`, `agy` | `agy acp --disable-telemetry` | Native | none |
| `grok-build` | Grok Build | `grok` | `npx --yes @xai-official/grok@latest agent stdio` | Native | `@xai-official/grok` |
| `opencode` | OpenCode | none | `opencode acp` | Native | none |
| `cursor` | Cursor Agent | `cursor-agent` | `cursor-agent acp`, or `agent acp` when `cursor-agent` is not on `PATH` | Native | none |
| `hermes` | Hermes Agent | `hermes-agent` | `hermes acp` | Native | none |
| `pi` | Pi | `pi-acp` | `npx --yes --package=pi-acp@latest --package=@earendil-works/pi-coding-agent@latest pi-acp` | Adapter | `pi-acp` |

- **Pinned version.** A local launch pins no adapter version. Every npm
  package runs at `@latest` unless `[acp_adapters.<id>] version` is set. A
  managed host runs the adapter that its launch spec pins (see
  `nori-monorepo: docs/specs/nori-cli/host-contract.md`, "Launch").
- **Package runner.** `<runner>` is `bunx` or `npx --yes`:
  1. `NORI_MANAGED_BY_BUN` is set: `bunx`.
  2. `NORI_MANAGED_BY_NPM` is set: `npx`.
  3. `bunx` is on `PATH`: `bunx`.
  4. Otherwise: `npx`.
- Grok Build and Pi always use `npx`. Grok needs npm's postinstall step,
  which `bunx` skips.
- With a version pin, `npx` also gets `--prefer-offline`.
- Pi's pin applies to `pi-acp` only. `@earendil-works/pi-coding-agent` always
  runs at `@latest`.
- When the runner has not cached the package, the TUI shows
  "Downloading `<package>`..." before the start.
- Agents with no npm package must be on `PATH`. The start error names the
  install step.
- Debug builds also offer the test agents `mock-model` and `mock-model-alt`.
  They run the binary in `MOCK_ACP_AGENT_BIN`, or `mock_acp_agent` next to the
  `nori` binary.

### Per-agent details

| ID | Context window | Model injection at spawn | Extra models in `/model` | Own transcripts read from |
|---|---|---|---|---|
| `claude-code` | 1,000,000 | env `ANTHROPIC_MODEL` | `claude-opus-4-8`, `claude-opus-4-7`, `claude-opus-4-6`, `claude-sonnet-4-6` | `~/.claude/projects` |
| `codex` | 258,000 | `model` key in env `CODEX_CONFIG` (JSON) | `gpt-5.3-codex` | `~/.codex/sessions` |
| `gemini` | 1,000,000 | none | none | none |
| `grok-build` | unknown | none | none | none |
| `opencode` | unknown | none | none | none |
| `cursor` | unknown | `--model <id>` argument | none | none |
| `hermes` | unknown | none | none | none |
| `pi` | unknown | none | none | none |

- **Codex goals.** Nori always sets `features.goals = false` in
  `CODEX_CONFIG`. Nori owns goals for Codex through the `nori-client` MCP
  server (see [MCP](mcp.md)). Nori merges these keys into any
  `CODEX_CONFIG` already in the environment. An invalid `CODEX_CONFIG` fails
  the start.
- **`CODEX_HOME`.** Nori removes `CODEX_HOME` from the environment of every
  agent it starts, so Codex reads `~/.codex`.
- **Own transcripts** feed the token count in the TUI only.
- **Installed check.** The agent picker marks an agent as installed when this
  binary is on `PATH`: `claude-agent-acp`, `codex-acp`, `agy`, `grok`,
  `opencode`, `cursor-agent` or `agent`, `hermes`, `pi-acp`.

## Steering support

Steering hands a message to the agent while its turn runs. The turn rules
are in [turns](turns.md). The host steers only when all of these hold:

1. The agent answers `initialize` with `protocolVersion: 1`.
2. It advertises `_meta.steering.supported: true`.
3. Its `agentInfo.name` is an adapter in the table below, and its
   `agentInfo.version` is at or above that adapter's minimum.

| Agent | `agentInfo.name` | Minimum version |
|---|---|---|
| `claude-code` | `@agentclientprotocol/claude-agent-acp` | 0.102.0 |
| `codex` | `@agentclientprotocol/codex-acp` | 2.7.0 |
| All others | none | Steering not supported |

When a check fails, a steer request goes to the prompt queue, and the
client gets the status "unsupported, queued".

## `[acp_adapters.<id>]`

Changes how Nori starts one agent.

| Key | Type | Default | Effect |
|---|---|---|---|
| `command` | string | unset | Replaces the whole launch command. Nori splits it like a shell. It must have at least one word and balanced quotes. |
| `version` | string | unset | Replaces `latest` on the agent's npm package. Use an npm version, range, or dist-tag: letters, digits, and `. - + ^ ~ < > = * _`. No spaces. |

- The table key is an agent ID or alias. When several keys resolve to the
  same agent, the exact ID wins, then the alphabetically first alias.
- `version` on an agent without an npm package, or on a custom agent, fails
  the start. Set `command` instead.
- `command` disables the download notice and the version pin.
- Unknown keys in the table are errors.
- An invalid entry stops the TUI and `nori host` at startup.

## `NORI_ACP_COMMAND_<ID>`

An environment variable that replaces one agent's launch command. `<ID>` is
the canonical agent ID in upper case, with `-` replaced by `_`. Examples:
`NORI_ACP_COMMAND_CLAUDE_CODE`, `NORI_ACP_COMMAND_GROK_BUILD`.

- Nori splits the value like a shell.
- The value must be UTF-8, with at least one word and balanced quotes.
- An alias does not work as `<ID>`.

### Launch precedence

For one agent, the first match wins:

1. `NORI_ACP_COMMAND_<ID>`
2. `[acp_adapters.<id>] command`
3. `[acp_adapters.<id>] version`, applied to the built-in package
4. The built-in command, with `@latest`

## Custom agents: `[[agents]]`

Each `[[agents]]` entry adds an agent, or replaces the built-in agent with
the same slug.

| Key | Type | Required | Effect |
|---|---|---|---|
| `name` | string | yes | Name in the agent picker |
| `slug` | string | yes | Agent ID. Use lower case: Nori lowercases the requested ID before it compares. |
| `distribution` | table | yes | How to start the agent. Set exactly one form (below). |
| `context_window_size` | integer | no | Context window in tokens, for the TUI |
| `auth_hint` | string | no | Text shown when the agent fails to authenticate |
| `transcript_base_dir` | string | no | The agent's own transcript folder, relative to the home folder |
| `model_override` | table | no | How Nori forces a model at spawn: `env = "<VAR>"` sets the variable to the model ID; `arg = "<flag>"` appends the flag and the model ID. When both are set, `env` wins. |

### `distribution`

| Form | Keys | Command Nori runs |
|---|---|---|
| `local` | `command`, `args` (default `[]`), `env` (default `{}`), `wrapper`, `env_remove` (default `[]`), `runtime_dir` | `[wrapper...] command args...` |
| `npx` | `package`, `args` (default `[]`) | `npx <package> args...` |
| `bunx` | `package`, `args` | `bunx <package> args...` |
| `pipx` | `package`, `args` | `pipx run <package> args...` |
| `uvx` | `package`, `args` | `uvx <package> args...` |

`local` options:

- `wrapper` is a list: a non-empty program, then its arguments. Nori puts it
  in front of `command`.
- `env_remove` removes variables from the inherited environment. A variable
  cannot be in both `env` and `env_remove`.
- `runtime_dir` must be an absolute path to an existing folder. The agent
  receives it as `NORI_AGENT_RUNTIME_DIR`. Do not also set that variable in
  `env` or `env_remove`.

Rules:

- Two entries with the same slug are an error.
- A custom agent gets no saved API keys and no token provider.
- A custom agent always shows as installed in the picker.
- An invalid `[[agents]]` list stops `nori host`. The TUI logs a warning and
  runs with the built-in agents only, unless a token provider is configured;
  then it stops.

Example:

```toml
[[agents]]
name = "Kimi"
slug = "kimi"
distribution = { uvx = { package = "kimi-cli", args = ["--acp"] } }
model_override = { arg = "--model" }
```

## Models

### Default model per agent

`[default_models]` maps an agent ID to a model ID:

```toml
[default_models]
claude-code = "claude-opus-4-6"
codex = "gpt-5.3-codex"
```

Nori picks the model for a session in this order:

1. The model of the token provider bound to the agent. For `pi` it is
   `<provider name>/<model>`. See [auth](auth.md).
2. The last `-c default_models.<key>=<model>` on the command line whose key
   resolves to the agent. A whole-table `-c default_models=...` cancels the
   dotted overrides before it.
3. The `[default_models]` entry under the agent's ID. Without one, the entry
   under the alphabetically first alias of the agent.
4. No model: the agent uses its own default.

### How Nori applies the model

1. **At spawn.** When the agent has a model injection channel (see
   [Per-agent details](#per-agent-details), or `model_override` for a custom
   agent), Nori puts the model into it. A failure is logged and does not
   stop the start.
2. **After the session starts.** When the agent advertises a session config
   option with category `model`, and its current value differs, Nori sends
   `session/set_config_option` with the model.
   - For a built-in agent, Nori sends it only when the option lists the
     model.
   - For a custom agent, Nori always sends it.
   - A failure is logged and does not stop the session.

### Choosing a model in the TUI

- `/model` lists the model option the agent advertises.
- For a built-in agent with an injection channel, the picker also lists the
  extra models in the table above, under "Other".
- Choosing a value of a `model` option saves it as
  `[default_models].<agent ID>`.
- When the agent rejects a model typed into the custom-model input, and the
  agent has an injection channel, Nori saves the model and starts a new
  session, so the model goes in at spawn.

## Related

- [Auth](auth.md): how credentials reach an agent.
- [MCP](mcp.md): MCP servers Nori passes to an agent.
- [Configuration](configuration.md): every `config.toml` key.
- `nori-sessions: broker/server/src/core-services/agents/docs.md`: agents
  on Sessions machines.

## Where `main` differs

- Steering uses an exact version allowlist, not a minimum:
  `claude-agent-acp` 0.102.0, 0.108.1, 0.109.0 and `codex-acp` 2.7.0, 2.8.0.
  Any other version, newer ones included, falls back to the queue.
