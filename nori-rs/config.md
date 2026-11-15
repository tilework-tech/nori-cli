# Nori configuration (`config.toml`)

Nori reads one TOML file at `$NORI_HOME/config.toml` (default
`~/.nori/cli/config.toml`). Launch flags and dotted overrides are layered on top
at startup. The resolution architecture and the full set of runtime-policy types
live in [`nori-config/docs.md`](nori-config/docs.md); this file documents the
agent and model configuration surface, which is the most commonly hand-edited
part of the file, plus the opt-in multi-session feature.

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

## Several sessions in one TUI: `[features] multi_session`

```toml
[features]
multi_session = true

[tui]
max_sessions = 8

[tui.hotkeys]
previous_session = "alt+,"
next_session = "alt+."
next_waiting_session = "alt+down"
open_session_list = "alt+s"
parent_session = "alt+up"
```

Off by default; restart Nori after changing it. With it on, `/new`, `/resume`,
switching agents with `/agent`, and forking to an earlier message keep the
previous session running in the background instead of ending it. `/new`,
`/agent`, and `/resume` also work while the session on screen is busy: that
session keeps working in the background and your next prompt goes to the new
one. One session is on screen at a time, and the terminal scrollback always
shows only that session's history.

`[tui] max_sessions` (default 8; values below 1 count as 1) is the most
sessions that may run at once. At the cap, `/new` and `/agent` refuse to start
another session and say how to close one.

| Way in | What it does |
| --- | --- |
| `previous_session` / `next_session` | Move through sessions in the order they were opened, wrapping at the ends |
| `next_waiting_session` | With an empty prompt, jump to the next session waiting on you |
| Left on an empty prompt, `open_session_list`, or `/sessions` | Open the session list |
| `parent_session` | From a delegated child, return to the session that delegated it |

The `[tui.hotkeys]` values above are the defaults, and the actions appear in
the Hotkeys view only with the feature on. Moving between sessions skips
closed ones. In the session list, Tab shows only sessions waiting on you (Tab
again shows all), typing filters by title or agent, Enter jumps, Esc clears
the filter and then closes, and Ctrl+R restarts a failed session from its
transcript in the same place, keeping its title. A session is titled by the
agent's summary, else its first message.

Closing a session:

| Way | What it does |
| --- | --- |
| Ctrl+D in the session list | Closes the selected session and stops its agent. A working, starting, or waiting session needs a second Ctrl+D; any other key cancels |
| Ctrl+D on a `closed` row | Removes it from the list |
| `/close` | Closes the session on screen (local sessions); in Nori Cloud it releases the session |

A closed session stays in the list as `closed` until you remove it. Closing
the session on screen shows the next open one. The last open session cannot be
closed this way; use `/quit`.

A status line above the prompt counts open sessions by status (starting,
working, waiting on you, ready, failed) once two or more are open, a separate
line names a background session that is waiting on you (for a delegated child,
as `<parent> › <child>`), and a temporary "Preparing <agent>…" line shows while
`/agent` starts a new agent. Inside a delegated child, a breadcrumb line names
the parent and child and the `parent_session` key. In the session list,
status words are colored: waiting on you amber, working cyan, ready green,
failed red. A blank session that never started is not counted or listed.

`/delegate <agent> <task>` (or the agent's own `delegate` tool) starts a
delegated child session, named after the first line of its task. In the
parent it reads as one line (`↳ you delegated to <agent> · <name> · working`);
the full brief and result are in the transcript view (Ctrl+T). When the child
ends, a short result block names it and shows up to six result lines, and the
result goes back to the parent agent on its own. A child on a different agent
asks first ("Yes, start it" / "No, decline"; Esc declines).

Remote control (`--remote`, `/remote-control`) serves the most recently started
session, which is not necessarily the one on screen.
