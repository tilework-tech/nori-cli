# Nori configuration (`config.toml`)

Nori reads one TOML file at `$NORI_HOME/config.toml` (default
`~/.nori/cli/config.toml`). Launch flags and dotted overrides are layered on top
at startup. The resolution architecture and the full set of runtime-policy types
live in [`nori-config/docs.md`](nori-config/docs.md); this file documents the
agent and model configuration surface, which is the most commonly hand-edited
part of the file.

## Choosing an agent

```toml
# The ACP agent to launch (built-in slug or a custom [[agents]] slug).
agent = "claude-code"
```

Built-in agents are `claude-code`, `codex`, and `gemini`.

## Default models: `[default_models]`

Maps an agent slug to the model id Nori applies when it starts a session with
that agent:

```toml
[default_models]
claude-code = "claude-opus-4-6"
codex = "gpt-5.1-codex"
```

The value is **not** validated against the agent's advertised model catalog. ACP
adapters advertise only a subset of the models they can actually run and reject
anything outside that list over the live `session/set_config_option` RPC. To make
an out-of-catalog model usable, Nori forces it through the agent's own
out-of-band channel at spawn time (see model injection below), so it becomes the
session's current model. An invalid id is only rejected at the first prompt;
recovery is manual (pick another model via `/model`).

When you pick a custom model with `/model` and the agent rejects it, Nori writes
the value here and restarts the session so the model is injected on the next
spawn. The `/model` picker also surfaces a curated per-agent **Other** section of
known-good models the adapter does not advertise, so you can select one instead
of typing its id; choosing one follows the same reject → persist-here → restart
path.

## Custom agents: `[[agents]]`

Each `[[agents]]` block defines an additional ACP agent (bring-your-own).

```toml
[[agents]]
name = "My Agent"            # display name in the picker
slug = "my-agent"            # machine id used as a slug / cmdline value
context_window_size = 200000 # optional token budget override
auth_hint = "run: my-agent login"  # optional message shown on auth failure
transcript_base_dir = ".my-agent"  # optional transcript dir, relative to home

# Exactly one distribution variant must be set.
[agents.distribution.local]
command = "/usr/bin/my-agent"
args = ["acp"]
env = { MY_AGENT_KEY = "value" }   # extra env for the spawned subprocess

# Optional: how to force a chosen model on this agent at spawn.
model_override = { env = "MY_AGENT_MODEL" }   # or { arg = "--model" }
```

Distribution variants (choose one): `local` (`command`/`args`/`env`), or one of
the package runners `npx`, `bunx`, `pipx`, `uvx` (each takes `package`/`args`).

### `model_override`

Custom agents advertise their own (possibly small) model catalog. `model_override`
tells Nori which out-of-band channel carries a forced model id so a
`[default_models]` entry the picker would reject can still be applied:

| Field | Meaning |
|-------|---------|
| `env` | Environment variable set to the model id on the spawned subprocess |
| `arg` | CLI flag appended, followed by the model id |

Set exactly one; `env` wins if both are present. Built-in agents know their own
channel (Claude → `ANTHROPIC_MODEL`, Gemini → `GEMINI_MODEL`, Codex → the `model`
key of `CODEX_CONFIG`) and ignore this field. For env-based channels, Nori's
injected value takes precedence over the agent's own configured model (for
example `~/.claude/settings.json`) for Nori-spawned sessions — Nori's
`[default_models]` is authoritative there by design.
