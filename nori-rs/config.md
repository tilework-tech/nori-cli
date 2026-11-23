# Nori configuration (`config.toml`)

Nori reads one TOML file at `$NORI_HOME/config.toml` (default
`~/.nori/cli/config.toml`). Launch flags and dotted overrides are layered on top
at startup. The resolution architecture and the full set of runtime-policy types
live in [`nori-config/docs.md`](nori-config/docs.md); this file documents the
agent and model configuration surface, which is the most commonly hand-edited
part of the file, plus tool call display and running several sessions in one TUI.

## Choosing an agent

```toml
# The ACP agent to launch (built-in slug or a custom [[agents]] slug).
agent = "claude-code"
```

Built-in IDs are `claude-code`, `codex`, `gemini` (Antigravity), `grok-build`, `opencode`, `cursor` (Cursor Agent), `hermes` (Hermes Agent), and `pi`. An explicitly configured agent with the requested slug takes precedence over a built-in or alias.

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

For Antigravity, Grok Build, OpenCode, Hermes Agent, and Pi, choose a model advertised in `/model`. Nori requests it through the agent's ACP model option, and the agent retains its current selection if it rejects the value.

OpenCode advertises enabled models for the session's working directory, using IDs such as `nori-gateway/<model>` for a gateway configured under that provider name. OpenCode reads its provider, endpoint, and default model from its native configuration, including a file selected with `OPENCODE_CONFIG`. It uses the credentials for that provider from its saved accounts, supported environment variables, or the provider configuration. [OpenCode providers](https://opencode.ai/docs/cli/providers/) · [OpenCode models](https://opencode.ai/docs/models/)

When signed in with a ChatGPT plan, OpenCode 2.2.6 and 2.3.2 offer GPT chat models with a numeric version above 5.5, excluding the exact ID `gpt-5.6` and models that require pro reasoning mode. For example, `gpt-5.10` passes the version filter. API-key and gateway connections have their own catalogs. [2.2.6 provider filter](https://github.com/anomalyco/opencode/blob/v2.2.6/packages/core/src/plugin/provider/openai.ts#L283-L305) · [2.3.2 provider filter](https://github.com/anomalyco/opencode/blob/v2.3.2/packages/core/src/plugin/provider/openai.ts#L285-L307)

Claude Code, Codex, and custom agents with a configured injection channel can also accept model IDs at process startup. For these agents, `/model` offers a custom ID and any curated additional choices. If the agent rejects a custom choice over ACP, Nori can save it here and restart the session to apply it. A provider may still reject an unknown model at the first prompt.

## Named API token providers

To run built-in Codex against a Responses endpoint, define a connection in
`$NORI_HOME/config.toml` and select it for the canonical agent slug:

```toml
[agent_token_providers]
codex = "lab"

[[token_providers]]
name = "lab"
preset = "custom"
protocol = "openai-responses"
base_url = "https://gateway.example.invalid/v1"
env_key = "LAB_MODEL_KEY"
model = "lab/model-a"
```

Replace the example endpoint and model with the values supplied by your gateway.
Make the API key available as `LAB_MODEL_KEY` in the environment that starts
Nori (for `nori host`, the host process). Save only its variable name in
`env_key`. Give each connection a unique name. A base URL accepts HTTP(S)
and excludes credentials, query parameters and fragments.

Run `nori --agent codex`, `nori --agent codex exec "hello"`, or
`nori host new --agent codex` with a locally configured host. A binding's
`model` supplies the exact API model ID and takes precedence over
`[default_models]` for that agent. A built-in launch alias such as `codex-acp` uses the saved `codex` binding.
Use the canonical slug as the key in `[agent_token_providers]`. A missing or
blank key stops launch,
including when native credentials are available. Remove the agent's binding
to resume its usual model and authentication settings. ChatGPT subscriptions
use Codex's native sign-in. Codex ACP can also generate a thread title using
its own auxiliary model on the selected connection.

**Protocol compatibility.** The resolver understands `openai-completions`,
`openai-responses`, and `anthropic-messages`. A `custom` endpoint exposes
exactly its declared protocol. `openrouter`, `fireworks`, and `subconscious`
try the declared protocol first, then other eligible protocols on the same
connection, keeping its endpoint, model and key reference. `cloudflare` accepts only `openai-completions`.
Subconscious uses Completions for Pi and OpenCode, Responses for Codex, and
Messages for Claude Code. Use the published gateway base URL for a named
preset or the account-scoped Workers AI URL for Cloudflare.

A bound launch requires an eligible pairing, a concrete wire protocol and an
adapter for the selected executable. A custom command that shadows a built-in
keeps its own launch definition. Broker-managed Sessions supply an admitted
launch specification. [Compatibility contract](nori-config/docs.md#api-token-provider-resolution)

### Pi connections

Selecting `pi` uses Pi's native profile by default. To use a named connection,
set `pi = "lab-pi"` under `[agent_token_providers]` and define the connection.

```toml
[[token_providers]]
name = "lab-pi"
preset = "custom"
protocol = "openai-completions"
base_url = "https://gateway.example.invalid/v1"
env_key = "LAB_PI_KEY"
model = "demo-model"
```

Make the key available to the process starting Nori. The selected endpoint,
protocol, model and key reference determine the private Pi profile. The model
ID on ACP is `lab-pi/demo-model`. Remove the `pi` binding to return to Pi's
native settings. Each launch owns its generated profile until the child exits. The next local launch removes abandoned profiles. Pi session history remains in the connection’s existing sessions directory. Nori also reads earlier Pi provider tables, preserving their
selected route, endpoint and key variable.

## Custom agents: `[[agents]]`

Each `[[agents]]` block defines an additional ACP agent (bring-your-own).

```toml
[[agents]]
name = "My Agent"            # display name in the picker
slug = "my-agent"            # machine id used as a slug / cmdline value
context_window_size = 200000 # optional token budget override
auth_hint = "run: my-agent login"  # optional message shown on auth failure
transcript_base_dir = ".my-agent"  # optional transcript dir, relative to home
model_override = { env = "MY_AGENT_MODEL" }   # optional, or { arg = "--model" }

# Exactly one distribution variant must be set.
[agents.distribution.local]
command = "/usr/bin/my-agent"
args = ["acp"]
env = { MY_AGENT_REGION = "lab" }   # extra env for the spawned subprocess
```

Choose one distribution. Use `local` for an installed executable or a package
runner (`npx`, `bunx`, `pipx`, `uvx`) with `package` and `args`.

### ACP server launch

In a local distribution, `command` and `args` select the ACP server. Add
`env` for settings the child should receive. Use `env_remove` to drop inherited
variables for this launch. A name can appear in only one of these fields.
A server that delegates to a second executable can read its path from an environment variable the server
supports. To run a wrapper first, supply its executable and each prefix
argument in a `wrapper` array.

```toml
[[agents]]
name = "Review"
slug = "review"

[agents.distribution.local]
command = "/opt/example/bin/example-acp"
args = ["--profile", "Review team"]
wrapper = ["/opt/example/bin/runtime-wrapper", "--"]
runtime_dir = "/opt/example/prepared/review"
env = { EXAMPLE_ENGINE = "/opt/example/bin/engine" }
env_remove = ["EXAMPLE_PROFILE"]
```

The wrapper receives `--`, the server path, `--profile`, and `Review team` as
four separate arguments. Nori performs no shell expansion. The wrapper must
pass ACP stdin and stdout through to the server and direct diagnostics to
stderr. When using `runtime_dir`, supply an existing absolute directory. The
child receives its path in `NORI_AGENT_RUNTIME_DIR` and keeps the session
workspace as its working directory. When supplying this path, leave
`NORI_AGENT_RUNTIME_DIR` out of both environment fields. Omit the wrapper, runtime or environment
independently when the server runs without them.

Picker selections, `nori exec`, local host sessions and delegated launches
use the registered agent slug. On a managed host the machine supplies the
pinned definition and stages any outer wrapper inside that binding’s prepared
runtime directory. An adapter that supports selecting its own inner executable
through an environment variable reads that setting from its child environment.
[Managed launch admission](cli/src/host/docs.md#managed-launch-control-v1)

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

## Background host: `[host]`

```toml
[host]
max_clients = 16
```

`[host] max_clients` (default 16) is the most clients a background `nori host`
serves at once over its WebSocket endpoints, `/acp` and its alias `/sessions`,
counting `nori attach` terminals, `nori host --stdio` clients that joined the
host, and remote ACP clients alike. One more connection gets HTTP 503 with a
message naming the limit; a missing or wrong bearer token gets 401 first.
Values below 1 count as 1. A full host still reports itself running to
`nori host`, and `nori host stop` stops it with SIGTERM when its connection
is refused. A managed host (`[host] managed =
true`) is not limited. Sessions are bounded separately by `[tui]
max_sessions`. See [the ACP remote transport](../docs/remote-control.md#3-acp-remote-transport).

## Several sessions in one TUI

```toml
[tui]
max_sessions = 8

[tui.hotkeys]
previous_session = "alt+,"
next_session = "alt+."
next_waiting_session = "alt+down"
open_session_list = "alt+s"
parent_session = "alt+up"
```

Several sessions per TUI is always on; there is no feature flag. The former
`[features] multi_session` key is no longer read, and a leftover one in
`config.toml` is ignored. `/new`, `/resume`, switching agents with `/agent`, and forking to an earlier message keep the
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
the Hotkeys view. Moving between sessions skips
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
| `/close` | Closes the session on screen (local sessions); in Nori Cloud it releases the session and the picker that replaces it confirms "Session closed (released)." |

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
