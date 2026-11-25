# Auth

This page explains how an agent gets its credentials when the Nori CLI starts
it, and lists every auth command, key, file, and variable. Terms follow the
[glossary](../GLOSSARY.md).

An agent gets its credentials in one of three ways. Nori picks one per agent
at each launch:

| Way | What the agent uses | Set with |
|---|---|---|
| Own login (default) | The agent's own sign-in: a subscription, its own key file, or keys in the environment | The agent itself, for example `claude` then `/login` |
| Saved API key | A key you saved through Nori, passed in the agent's API key variable | `nori auth set-key`, then `nori auth prefer-api-key` |
| Token provider | An endpoint plus an API key that replaces the agent's own sign-in | `[[token_providers]]` and `[agent_token_providers]` in `config.toml` |

Nori never prints a secret. Key input is masked, and logs and errors leave
out key values.

## Guide

### Check how each agent signs in

```sh
nori auth
```

The table shows, per agent, a status, the credentials Nori can see, and where
each one comes from. A `note:` line follows an agent that has a billing or
saved-key notice. The last line lists saved keys and the store.

### Use an API key instead of a subscription

1. Save the key for its model provider:

   ```sh
   nori auth set-key anthropic
   ```

   At a terminal, Nori reads the key hidden. It echoes only a known public
   prefix, such as `sk-ant-api03-`, and draws every other character as `*`.
   From a pipe, Nori reads stdin.
2. Tell the agent to use saved keys:

   ```sh
   nori auth prefer-api-key claude-code
   ```

   When the agent is signed in with a subscription, Nori warns that usage
   moves to the API account and asks you to type `yes`. Nori asks once per
   agent. Pass `--yes` to accept without asking.
3. Start a new session. The choice applies from the agent's next launch.

To go back to the agent's own login:

```sh
nori auth prefer-login claude-code
```

In the TUI, `/api-key` offers the same choice for the current agent.

### Sign in to the agent itself

In the TUI, `/login` signs in to the current agent:

- **Codex:** Nori runs the OpenAI browser sign-in and writes the result to
  `~/.codex/auth.json`, where the Codex agent reads it.
- **Every other agent:** Nori shows the agent's own sign-in steps, for
  example "Run `claude` in another terminal and use /login".

`/logout` tells you to run the agent's own logout command.

### Use a token provider

A token provider sends the agent's model traffic to a gateway, with a key
from an environment variable:

```toml
[[token_providers]]
name = "openrouter"
preset = "openrouter"
protocol = "openai-responses"
base_url = "https://openrouter.ai/api/v1"
env_key = "OPENROUTER_API_KEY"
model = "openai/gpt-5.1-codex"

[agent_token_providers]
codex = "openrouter"
```

Nori stores only the variable name. Set the key in the environment that
starts `nori`. Only `codex` and `pi` can use a token provider.

## Reference

### `nori auth`

| Command | Effect |
|---|---|
| `nori auth [--json]` | Print every agent's sign-in status. `--json` prints the full report. |
| `nori auth set-key <provider> [--json]` | Save an API key for a provider. Warns, and saves anyway, when the key's prefix belongs to another issuer. `--json` prints `{"provider", "type", "store", "agents"}`. |
| `nori auth remove-key <provider>` | Forget the saved key for a provider |
| `nori auth prefer-api-key <agent> [--yes] [--json]` | Pass saved keys to the agent at launch. `--json` prints `{"agent", "prefer_api_key", "api_billing_confirmed"}`, or `{"error": "confirmation_required", "code", "message"}`. |
| `nori auth prefer-login <agent> [--json]` | Let the agent use its own login. `--json` prints the same object as above. |

- `prefer-api-key` without `--yes` fails with `confirmation_required` when
  confirmation is needed and stdin is not a terminal or `--json` is set.
- `prefer-api-key` and `prefer-login` refuse `hermes`, which takes no saved
  key, and a provider name, which is not an agent.
- `set-key` refuses an agent ID and names the provider to use instead.

### Status values

| Status | Meaning |
|---|---|
| `signed in` (`signed_in`) | Nori found at least one credential |
| `signed out` (`signed_out`) | Nori found none, and it can see every place the agent signs in |
| `unknown` | Nori found none, but the agent keeps a sign-in Nori cannot inspect: `gemini`, `grok-build`, `cursor`, `hermes`, `opencode`, `pi`, and `codex` on Linux when its `cli_auth_credentials_store` is `keyring` or `auto` |

Credential kinds: `subscription`, `api_key`, `cloud` (Bedrock, Vertex AI, and
similar), `token`, `login` (a stored sign-in of unknown type).

### Notices

| Code | When |
|---|---|
| `api_key_overrides_subscription` | The agent will use an API key or token ahead of a subscription sign-in, so usage is billed to the API account. `prefer-api-key` and `/api-key` show it as the billing warning. |
| `saved_key_not_used` | A key is saved for a provider the agent uses, but the agent prefers its own login |

OpenCode and Pi keep their own stored login for a provider even when they get
a key. They never get the billing warning.

### Providers and key routes

Saved keys are per model provider. A saved key reaches an agent only in the
variable below, and only when the agent prefers API keys.

| Provider | `<provider>` | Agents and the variable each receives |
|---|---|---|
| Anthropic | `anthropic` | `claude-code` `ANTHROPIC_API_KEY`; `opencode` `ANTHROPIC_API_KEY`; `pi` `ANTHROPIC_API_KEY` |
| OpenAI | `openai` | `codex` `OPENAI_API_KEY`; `opencode` `OPENAI_API_KEY`; `pi` `OPENAI_API_KEY` |
| Google Gemini | `google` | `gemini` `GEMINI_API_KEY`; `opencode` `GOOGLE_GENERATIVE_AI_API_KEY`; `pi` `GEMINI_API_KEY` |
| xAI | `xai` | `grok-build` `XAI_API_KEY`; `opencode` `XAI_API_KEY`; `pi` `XAI_API_KEY` |
| Cursor | `cursor` | `cursor` `CURSOR_API_KEY` |

`hermes` and custom agents take no saved key.

### When a saved key reaches the agent

Nori passes a saved key at launch only when all of these hold:

- `[auth.<agent>] prefer_api_key = true`.
- The agent is built in and has a key route.
- No token provider is bound to the agent.
- A key is saved for the provider.

Nori skips one provider's route when any of these hold:

- A credential variable the agent reads for that provider is already set, in
  the environment or in the agent's launch `env`. The variables are listed
  under [Detection](#detection).
- The provider's base URL variable is set and not blank:
  `ANTHROPIC_BASE_URL`, `OPENAI_BASE_URL`, or `GOOGLE_GEMINI_BASE_URL`.
- The route's variable is in the agent's `env_remove`.

A store that cannot be read never stops the launch. Nori logs a warning
without values and starts the agent without saved keys.

### `[auth.<agent>]`

Keyed by the canonical agent ID. Unknown keys are errors.

| Key | Type | Default | Effect |
|---|---|---|---|
| `prefer_api_key` | bool | `false` | Pass saved keys to the agent at launch |
| `api_billing_confirmed` | bool | `false` | You accepted the billing change. Nori does not ask again. |

### Credential stores

`NORI_CREDENTIALS_STORE` picks where saved keys live.

| Value | Store |
|---|---|
| unset, empty, or `file` | `$NORI_HOME/credentials.json`, mode 0600 |
| `keyring` | One OS keyring item: service `Nori Credentials`, account `credentials` |
| anything else | Error |

- The two stores hold the same JSON document:
  `{"<provider>": {"type": "api_key", "key": "..."}}`. Records of unknown
  types or providers survive rewrites.
- Nori never falls back from one store to the other.
- A write goes to a 0600 temporary file in `$NORI_HOME`, is synced, then
  renamed over the old file. Writers hold an advisory lock on the
  `$NORI_HOME` folder.
- An empty keyring document deletes the keyring item.
- When `$NORI_HOME/saved-keys.json` exists, every read fails. The error says
  which files to delete before you save each key again.

### Detection

`nori auth`, `/api-key`, and `prefer-api-key` look in these places. Nori only
reads them.

| Agent | Environment (in the agent's order) | Files and keychain |
|---|---|---|
| `claude-code` | `CLAUDE_CODE_USE_BEDROCK`, `CLAUDE_CODE_USE_VERTEX`, `CLAUDE_CODE_USE_FOUNDRY` (`1` or `true`), `ANTHROPIC_AUTH_TOKEN`, `ANTHROPIC_API_KEY`, `CLAUDE_CODE_OAUTH_TOKEN` | `$CLAUDE_CONFIG_DIR` or `~/.claude`: `.credentials.json`; `$CLAUDE_CONFIG_DIR/.claude.json` or `~/.claude.json`; macOS keychain `Claude Code-credentials` (with a hash suffix when `CLAUDE_CONFIG_DIR` is set) |
| `codex` | `CODEX_API_KEY`, `OPENAI_API_KEY` | `~/.codex/auth.json`; `~/.codex/config.toml` (`cli_auth_credentials_store`); macOS keychain `Codex Auth` |
| `gemini` | `GOOGLE_GENAI_USE_VERTEXAI`, `AGY_ADC_AUTH` (`1` or `true`), `GEMINI_API_KEY`, `GOOGLE_API_KEY` | none |
| `grok-build` | `XAI_API_KEY` | none |
| `cursor` | `CURSOR_API_KEY` | none |
| `opencode` | `ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, `GOOGLE_GENERATIVE_AI_API_KEY`, `GOOGLE_API_KEY`, `GEMINI_API_KEY`, `XAI_API_KEY` | `${XDG_DATA_HOME:-~/.local/share}/opencode/auth.json` |
| `pi` | `ANTHROPIC_AUTH_TOKEN`, `ANTHROPIC_OAUTH_TOKEN`, `ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, `GEMINI_API_KEY`, `XAI_API_KEY` | `~/.pi/agent/auth.json` |
| `hermes` | none | none |

For OpenCode and Pi, the agent's own file wins over the environment. For the
other agents, the environment wins.

### Token providers

`[[token_providers]]` defines a connection. Unknown keys are errors.

| Key | Values | Rule |
|---|---|---|
| `name` | `[A-Za-z0-9_-]+` | Unique across entries |
| `preset` | `openrouter`, `subconscious`, `fireworks`, `cloudflare`, `custom` | |
| `protocol` | `openai-completions`, `openai-responses`, `anthropic-messages` | Must be one the preset allows |
| `base_url` | HTTP or HTTPS URL | No credentials, query, or fragment. A named preset must use its gateway URL. `cloudflare` must be `https://api.cloudflare.com/client/v4/accounts/<32 hex>/ai/v1`. |
| `env_key` | Variable name | Holds the key. Nori stores only the name. |
| `model` | Model ID | Not empty, no whitespace |

`[agent_token_providers]` binds an agent to a connection by name:

- The key must be the canonical agent ID; an alias is an error.
- The name must refer to a defined connection.
- The connection must be compatible with the agent under the harness-compat
  contract. Nori uses that contract's protocol for the agent.
- The connection's `model` becomes the agent's model. See
  [agents](agents.md#models).

A legacy table `[token_providers.<name>]` with `preset`, `api_key_env`, and
`base_url` is rewritten into the list form when Nori loads or edits the file.
Its default model is `subconscious/glm-5.3-marathon`.

The harness-compat contract is owned by Sessions:
`nori-sessions: broker/server/src/core-services/agents/docs.md`. Nori embeds
a copy, pinned to one Sessions commit, at build time.

### How a token provider reaches the agent

At launch, Nori reads the key from `env_key` in its own environment. A
missing, blank, or multi-line key fails the launch.

| Agent | What Nori does |
|---|---|
| `codex` | Requires `openai-responses`. Sets `CODEX_CONFIG` to a provider `nori_binding` with the `base_url` and model, and `MODEL_PROVIDER=nori_binding`. Passes the key as `NORI_PROVIDER_API_KEY`. Removes `OPENAI_API_KEY`, `CODEX_API_KEY`, `DEFAULT_AUTH_REQUEST`, and `env_key` from the agent's environment. |
| `pi` | Writes a private profile (`models.json`, `settings.json`) to a fresh folder under `$NORI_HOME/.pi-launches/` and sets `PI_CODING_AGENT_DIR` to it. Pi keeps its history in `$NORI_HOME/token-providers/<name>-pi-acp/sessions`. Passes the key in `env_key`. The model is `<name>/<model>`. |
| Any other agent | The launch fails: "this agent has no named token provider launch adapter". |

When ACP wire recording is on, Nori redacts the key from the recording.

### On a managed host

A managed host does not use saved keys or `[agent_token_providers]`. Its
launch spec carries the environment and credential references for each
binding. See `nori-monorepo: docs/specs/nori-cli/host-contract.md`
("Managed wire"), and for how Sessions builds them,
`nori-sessions: broker/server/src/core-services/agents/docs.md`.

### Files

| Path | Written by | Content |
|---|---|---|
| `$NORI_HOME/credentials.json` | `nori auth set-key`, `remove-key` | Saved keys (file store) |
| `$NORI_HOME/config.toml` | `prefer-api-key`, `prefer-login`, `/api-key` | `[auth.<agent>]` |
| `$NORI_HOME/.pi-launches/` | Pi launches with a token provider | Private Pi profiles, one per launch |
| `$NORI_HOME/token-providers/<name>-pi-acp/` | Pi launches with a token provider | Pi history |
| `~/.codex/auth.json` | `/login` for Codex | The Codex agent's sign-in, mode 0600 |

## Related

- [Agents](agents.md): agent IDs and launch.
- [Configuration](configuration.md): every `config.toml` key.
- [Storage](storage.md): every file under `NORI_HOME`.
- [MCP](mcp.md): MCP OAuth tokens.
