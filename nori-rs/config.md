# Nori configuration (`config.toml`)

Nori reads one TOML file at `$NORI_HOME/config.toml` (default
`~/.nori/cli/config.toml`). Launch flags and dotted overrides are layered on top
at startup. The resolution architecture and the full set of runtime-policy types
live in [`nori-config/docs.md`](nori-config/docs.md); this file documents the
agent and model configuration surface, which is the most commonly hand-edited
part of the file, plus tool call display and the opt-in multi-session feature.

## Choosing an agent

```toml
# The ACP agent to launch (built-in slug or a custom [[agents]] slug).
agent = "claude-code"
```

Built-in IDs are `claude-code`, `codex`, `gemini` (Antigravity), `grok-build`, and `pi`. An explicitly configured agent with the requested slug takes precedence over a built-in or alias.

## Default models: `[default_models]`

Maps an agent slug to the model id Nori applies when it starts a session with
that agent:

```toml
[default_models]
claude-code = "claude-opus-4-6"
codex = "gpt-5.1-codex"
```

Nori saves model choices under the agent’s canonical ID. For example, selecting a model after launching `--agent agy` saves it under `gemini`. If the canonical entry is missing, Nori reads an existing alias entry, choosing the first key in alphabetical order if several match. A custom agent with an alias as its slug keeps its own model preference.

For a single launch, `nori --agent agy -c 'default_models.antigravity="gemini-3.6-flash"'` overrides the saved Gemini model. If several dotted `-c` assignments name the same agent through different aliases, the last one wins. Assigning the entire `default_models` table replaces earlier assignments.

For Antigravity, Grok Build, and Pi, choose a model advertised in `/model`. Nori requests it through the agent's ACP model option, and the agent retains its current selection if it rejects the value.

Claude Code, Codex, and custom agents with a configured injection channel can also accept model IDs at process startup. For these agents, `/model` offers a custom ID and any curated additional choices. If the agent rejects a custom choice over ACP, Nori can save it here and restart the session to apply it. A provider may still reject an unknown model at the first prompt.

## Token providers: `[token_providers]` and `[agent_token_providers]`

A token provider is an API endpoint that serves models to an agent with a key
you keep in an environment variable. Only Pi can use one today, and the only
preset is `subconscious`. Choosing a provider for Pi in `/agent` writes these
tables for you.

```toml
[token_providers.subconscious]
preset = "subconscious"                 # catalog preset
api_key_env = "SUBCONSCIOUS_API_KEY"    # name of the variable, never the key
# base_url = "https://proxy.example/v1" # optional; replaces the preset endpoint

[agent_token_providers]
pi = "subconscious"                     # agent slug -> provider id

[default_models]
pi = "subconscious/subconscious/glm-5.2" # <provider id>/<model id>
```

- The table key under `[token_providers]` is the provider id that agents bind
  to. The saved model uses Pi's ACP form, the provider id followed by the
  preset's model id. Without a saved model Nori uses the preset's default.
- Nori never writes the key. Before launch it checks that the named variable
  is set and non-empty in the environment that started Nori, and refuses to
  start the agent if it is not. The generated Pi settings refer to the
  variable by name, so Pi reads the key from the environment it inherits.
- A binding Nori cannot honor stops the launch with an error: an unknown
  provider id or preset, an agent that cannot use the preset, or a model
  outside the preset's catalog. Nori never falls back to the agent's own
  settings.
- An agent without an `[agent_token_providers]` entry uses its own sign-in and
  settings; for Pi that is `~/.pi/agent`. Choosing "Pi settings" in `/agent`
  removes the entry and drops a saved model that belonged to the provider.
- For a bound Pi launch, Nori writes a private Pi agent directory under
  `$NORI_HOME/token-providers/` and points `PI_CODING_AGENT_DIR` at it.
  Pi then does not load `~/.pi/agent`: its extensions, skills, settings and
  session history stay with unbound launches, so resuming a Pi session works
  only under the same binding.
- While bound, `/model` can still list other Pi providers whose own key
  variables are set. Saving one of those makes the next launch stop with a
  "not offered by" error until you choose a model again in `/agent`.

This is an interim, CLI-local representation. A shared token-provider
configuration is expected to replace it.

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

Set exactly one. If both are present, `env` wins. This field configures custom agents. The built-in Claude Code uses `ANTHROPIC_MODEL` and Codex uses the `model` key of `CODEX_CONFIG`. For env-based channels, Nori's
injected value takes precedence over the agent's own configured model (for
example `~/.claude/settings.json`) for Nori-spawned sessions — Nori's
`[default_models]` is authoritative there by design.

## Tool call display: `tool_output`

```toml
[tui]
tool_output = "compact"   # or "full" (the default)

[projects."/home/me/work/app"]
trust_level = "trusted"
tool_output = "compact"   # this project only
```

`full` (the default) shows every command in full with syntax highlighting, all
of its output, every exploring step, and edit diffs. `compact` shows each tool
call as one scannable line: a status dot (green ok, red failed, animated while
running), the command summarized to its command names with arguments shown as
`…`, and the last line of output underneath. Read-only commands and file
reads/searches collapse into one `Explored N reads · M searches` line, and
edits show only the file and line counts.

The setting is read in this order, first match wins:

1. `-c tui.tool_output=...` (or a `-c tui={...}` table that sets
   `tool_output`) on the command line. It applies to that run only and is
   never saved.
2. `tool_output` in the active project's `[projects."<path>"]` entry. The
   active project is the entry for the exact working directory, otherwise the
   entry for its git repository root; linked worktrees use the main
   repository's entry.
3. `[tui] tool_output`.
4. `full`.

The transcript view (Ctrl+T or Ctrl+O) always shows the full form. `nori
attach` does not read project settings and always shows full output.

After the first-run directory-trust question, Nori asks "Use compact tool
output in this project?" with Full preselected. Choosing Compact or Full
writes `tool_output = "compact"` or `"full"` to the same project entry the
trust answer was saved under (the git repository root, or the working directory
outside a repository). Pressing Esc writes nothing, so the project keeps
following `[tui] tool_output` (including a value added later) or the full
default. The question is skipped only when a saved `[tui] tool_output =
"compact"` already applies; a one-off `-c` override does not skip it.
Directories that are already trusted never see the trust
screen, so they keep full output unless you set the key yourself.

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

The multi-session features in the TUI are interactive conveniences for one
terminal. For long-lived sessions under remote or programmatic control, use
`nori host`, which keeps sessions running with no terminal attached and
accepts scripts (for example `nori host new --cwd DIR --prompt TEXT`); attach
with `nori attach`. Nori deliberately does not build a full terminal
multiplexer into the TUI's hot path.

`[tui] max_sessions` (default 8; values below 1 count as 1) is the most
sessions that may run at once. At the cap, `/new` and `/agent` refuse to start
another session and say how to close one.

| Way in | What it does |
| --- | --- |
| `previous_session` / `next_session` | Move through sessions in the order they were opened, wrapping at the ends |
| `next_waiting_session` | Jump to the next session waiting on you; a draft stays in its own session's prompt. With nothing waiting, the key goes to a non-empty prompt |
| Left on an empty prompt, `open_session_list`, or `/sessions` | Open the session list |
| `parent_session` | From a delegated child, return to the session that delegated it; elsewhere it does nothing |

The `[tui.hotkeys]` values above are the defaults, and the actions appear in
the Hotkeys view only with the feature on. Moving between sessions skips
closed and done ones. In the session list, Tab shows only sessions waiting on you (Tab
again shows all), arrows or `j`/`k` move, `/` (or `f`, Ctrl+F) starts a search
that filters by title or agent, Enter jumps, Esc leaves the search and then
closes, and Ctrl+R restarts a failed session from its
transcript in the same place, keeping its title. A session is titled by the
agent's summary, else its first message, else the task of a `/delegate` sent
first; a delegation result never titles a session. Rows show the agent slug
that `/delegate` takes.

Closing a session:

| Way | What it does |
| --- | --- |
| Ctrl+D in the session list | Closes the selected session and stops its agent. A working, starting, or waiting session needs a second Ctrl+D; any other key cancels |
| Ctrl+D on a `closed` or `done` row | Removes it from the list |
| `/close` | Closes the session on screen (local sessions); in Nori Cloud it releases the session |

A closed session stays in the list as `closed` until you remove it; a
delegated child whose task finished stays as `done`. Closing
the session on screen shows the next open one. The last open session cannot be
closed this way; use `/quit`.

A status line above the prompt counts open sessions by status (starting,
working, waiting on you, ready, failed) once two or more are open, a separate
line names a background session that is waiting on you (for a delegated child,
as `<parent> › <child>`), and a temporary "Preparing <agent>…" line shows while
`/agent` starts a new agent. Inside a delegated child, a breadcrumb line shows
the path from the top-level session down to it and the `parent_session` key.
These lines fit the terminal width, shortening names before dropping them. In
the session list, status words are colored: waiting on you amber, working
cyan, ready green, failed red; starting, closed, and done are muted. A blank session that never started is not counted or listed.

`/delegate <agent> <task>` (or the agent's own `delegate` tool) starts a
delegated child session, named after the first line of its task. In the
parent it reads as one line (`↳ you delegated to <agent> · <name> · working`);
the full brief and result are in the transcript view (Ctrl+T). When the child
ends, a short result block names it and shows up to six result lines, and the
result goes back to the parent agent on its own. A child on a different agent
asks first ("Yes, start it" / "No, decline"; Esc declines).

Remote control (`--remote`, `/remote-control`) serves the most recently started
session, which is not necessarily the one on screen.
