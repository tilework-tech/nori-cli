# Configuration reference

This page lists every key in the Nori CLI configuration file, with its type,
default, and effect, and the order in which settings combine. Terms follow
`GLOSSARY.md`.

Facts owned elsewhere:

- Agents, adapters, `[[agents]]`, `[acp_adapters]`, `default_models`:
  [agents.md](agents.md)
- `[auth.<agent>]`, `[[token_providers]]`, `agent_token_providers`:
  [auth.md](auth.md)
- What `approval_policy` and the permission modes do:
  [permissions.md](permissions.md)
- How `[mcp_servers]` reach agents, and MCP OAuth: [mcp.md](mcp.md)
- `[host]` keys on the managed wire:
  `nori-monorepo: docs/specs/nori-cli/host-contract.md`
- Command-line flags and environment variables: [commands.md](commands.md)

## The file

- Path: `$NORI_HOME/config.toml`. `NORI_HOME` defaults to `~/.nori/cli`.
- Format: TOML.
- A missing or empty file means "all defaults".
- A file that does not parse stops every command that loads configuration.
- Unknown keys are ignored, except inside tables that reject unknown fields:
  `[host]`, `[host.dedicated_roots]`, `[acp_adapters.<slug>]`,
  `[auth.<agent>]`, and `[[token_providers]]`.
- These keys are errors:

| Key | Error |
|---|---|
| `profile`, `profiles` | Codex configuration profiles are not supported. Use Nori skillsets. |
| `model` | Use `agent`. |
| `[mcp_servers.nori-client]` | The name `nori-client` is reserved for Nori's own MCP server. |

Nori edits this file itself (see [Keys Nori writes](#keys-nori-writes)). An
edit keeps comments and formatting, holds a lock on
`$NORI_HOME/.config.toml.lock`, and replaces the file atomically. A new file
gets mode `0600`; an existing file keeps its mode.

## Precedence

From lowest to highest:

1. Built-in defaults.
2. `config.toml`.
3. `-c key=value` overrides, in command-line order. Overrides before the
   subcommand come first, so overrides after it win.
4. Folder settings from `[projects."<path>"]`, for the keys this table
   supports (see [projects](#projectspath)). A `-c tui.<key>` override beats
   the folder setting for the same key.
5. Command-line flags: `--agent`, `--yolo`, `--cd`, `--add-dir`. These beat
   the file and `-c`.

`NORI_ACP_COMMAND_<SLUG>` beats `[acp_adapters.<slug>]` (see
[agents.md](agents.md)). There are no configuration profiles.

A running background host uses the configuration it started with. A client
that brings its own `-c`, `--add-dir`, `--agent`, or `--yolo` gets a private
host instead (see [commands.md](commands.md#nori-exec)).

### Command-line overrides

`-c key=value` (or `--config key=value`) sets one key for this run.

- `key` is a dotted path, such as `tui.animations` or
  `mcp_servers.docs.enabled`. Missing tables are created. A value that is not
  a table is replaced by one when the path goes through it.
- The text is split at the first `=`. Both sides are trimmed.
- `value` is parsed as TOML. When that fails, it is a string, with
  surrounding quotes removed. So `-c agent=codex` works without quotes.
- A later override of a whole table (`-c tui={...}`) replaces earlier
  overrides inside it.
- Overrides are not saved.

### Folder trust

`[projects."<path>"]` is chosen for the session folder like this:

1. The entry whose key equals the canonical session folder.
2. Otherwise, the entry for the folder's Git project root.
3. Otherwise, no entry.

When no `approval_policy` is set, `trust_level = "untrusted"` makes the
default `untrusted`; `trusted` or no entry makes it `on-request`.

The TUI shows the trust prompt when the folder has no `trust_level` and none
of `approval_policy`, `sandbox_mode`, or `[sandbox_workspace_write]` is set
(in the file, by `-c`, or by `--yolo`). On native Windows it shows the prompt
only when the experimental Windows sandbox is on and available. The answer is
saved for the Git project root, or for the folder outside Git.

## Top-level keys

| Key | Type | Default | Effect |
|---|---|---|---|
| `agent` | string | `"claude-code"` | Agent for new sessions. The TUI saves the agent you pick here. `--agent` overrides it for one run. |
| `approval_policy` | `"untrusted"`, `"on-failure"`, `"on-request"`, `"never"` | from [folder trust](#folder-trust) | Who answers permission requests. `untrusted` is the `ask` mode, `on-failure` and `on-request` are `agent`, `never` is `approve-all`. See [permissions.md](permissions.md). |
| `sandbox_mode` | `"read-only"`, `"workspace-write"`, `"danger-full-access"` | `"workspace-write"` | **Not enforced yet.** Agent tools do not run in a Nori sandbox. Today it affects only the TUI: what `/approvals` and the status show, the trust prompt, and `--add-dir` (rejected under `read-only`). `nori sandbox` sets its own mode. Per-agent sandboxing is being built (nori-monorepo PR #263). |
| `sandbox_workspace_write` | table | see below | **Not enforced yet.** Settings for `workspace-write`. Used by `nori sandbox`. |
| `shell_environment_policy` | table | see below | Environment for commands run by `nori sandbox`. No effect on agents. |
| `history_persistence` | `"save-all"`, `"none"` | `"save-all"` | `none` stops writes to `$NORI_HOME/history.jsonl`. |
| `browser_profile` | `"throwaway"`, `"persistent"`, `"system"` | `"throwaway"` | Chrome profile for `/browser`: a temporary profile deleted on exit; `$NORI_HOME/browser-profile`; or your own Chrome profile with its logins. |
| `notify` | array of strings | unset | Notification command. Nori runs it with the notification JSON as the last argument and does not wait. An empty array turns notifications off. Unset: desktop notifications when `[tui] os_notifications` is enabled. |
| `check_for_update_on_startup` | boolean | `true` | Check GitHub for a newer release at most every 20 hours. |
| `disable_paste_burst` | boolean | `false` | Turn off burst-paste detection in the prompt box. |
| `default_models` | table: agent slug to model | empty | Model for new sessions of each agent. See [agents.md](agents.md). |
| `agents` | array of tables | empty | Custom agents. See [`[[agents]]`](#agents). |
| `acp_adapters` | table | empty | Adapter launch overrides. See [`[acp_adapters.<slug>]`](#acp_adaptersslug). |
| `auth` | table | empty | Per-agent login choice. See [`[auth.<agent>]`](#authagent). |
| `token_providers` | array of tables | empty | Named API connections. See [`[[token_providers]]`](#token_providers). |
| `agent_token_providers` | table: agent slug to provider name | empty | Binds an agent to a token provider. Keys must be canonical agent slugs. An unknown provider name is an error. |
| `mcp_servers` | table | empty | MCP servers. See [`[mcp_servers.<name>]`](#mcp_serversname). |
| `hooks` | table | empty | Lifecycle scripts. See [`[hooks]`](#hooks). |
| `tui` | table | | See [`[tui]`](#tui). |
| `projects` | table | empty | Folder settings. See [`[projects."<path>"]`](#projectspath). |
| `host` | table | | See [`[host]`](#host). |
| `acp_proxy` | table | | See [`[acp_proxy]`](#acp_proxy). |
| `cloud` | table | | See [`[cloud]`](#cloud). |
| `features` | table | | See [`[features]`](#features). |
| `notice` | table | | See [`[notice]`](#notice). |
| `cli` | table | | See [`[cli]`](#cli). |

### `[sandbox_workspace_write]`

**Not enforced yet** for agents (see `sandbox_mode`).

| Key | Type | Default | Effect |
|---|---|---|---|
| `writable_roots` | array of paths | `[]` | Extra writable folders. `--add-dir` adds to this list. |
| `network_access` | boolean | `false` | Allow network access. |
| `exclude_tmpdir_env_var` | boolean | `false` | Do not make `$TMPDIR` writable. |
| `exclude_slash_tmp` | boolean | `false` | Do not make `/tmp` writable. |

### `[shell_environment_policy]`

Applies only to `nori sandbox`.

| Key | Type | Default | Effect |
|---|---|---|---|
| `inherit` | `"core"`, `"all"`, `"none"` | `"all"` | Which variables to start from. |
| `ignore_default_excludes` | boolean | `false` | Keep variables the default filter removes. |
| `exclude` | array of patterns | `[]` | Remove matching names. Patterns use `*` and `?` and ignore case. |
| `set` | table: name to value | `{}` | Set these variables. |
| `include_only` | array of patterns | `[]` | When not empty, keep only matching names. |
| `experimental_use_profile` | boolean | `false` | **No effect.** Parsed but never read. |

## `[tui]`

| Key | Type | Default | Effect |
|---|---|---|---|
| `animations` | boolean | `true` | Shimmer effects and spinners. |
| `resize_reflow` | boolean | `true` | Rebuild scrollback when the terminal width changes. |
| `terminal_notifications` | `"enabled"`, `"disabled"` | `"enabled"` | Terminal notifications (OSC 9). |
| `os_notifications` | `"enabled"`, `"disabled"` | `"enabled"` | Desktop notifications. |
| `program_status` | boolean | `false` | Report the foreground session's status to the terminal (OSC 7501). Off by default because the sequence is not standard. See [tui.md](tui.md#terminal-status). |
| `program_status_approval_titles` | boolean | `false` | Put the real approval title in a blocked status report instead of "Approval needed". Titles can contain command text that terminals may show in tab tooltips and notifications. |
| `notify_after_idle` | `"5s"`, `"10s"`, `"30s"`, `"60s"`, `"disabled"` | `"5s"` | Idle time before a notification. |
| `vertical_footer` | boolean | `false` | Stack footer segments vertically. |
| `vim_mode` | `"newline"`, `"submit"`, `"always_submit"`, `"off"`, or boolean | `"off"` | Vim keys in the prompt box and what Enter does. `newline`: Enter adds a line in INSERT, submits in NORMAL. `submit`: the reverse. `always_submit`: Enter submits in both. `true` means `submit`, `false` means `off`. |
| `script_timeout` | string | `"30s"` | Time limit for hook scripts and custom prompt scripts. `<n>s`, `<n>m`, or `<n>` seconds. A value that does not parse means 30 seconds. |
| `loop_count` | integer | unset | Run the first prompt this many times, each in a fresh session. Unset: off. |
| `max_sessions` | integer | `8` | Most sessions running at once in the TUI and in a local `nori host`. Values below 1 mean 1. |
| `auto_worktree` | `"automatic"`, `"ask"`, `"off"`, or boolean | `"off"` | Create a Git worktree under `<repo>/.worktrees/` when the TUI starts. `true` means `automatic`. Not used by `nori cloud`. |
| `skillset_per_session` | boolean | `false` | Give each session its own skillset. |
| `file_manager` | `"vifm"`, `"ranger"`, `"lf"`, `"nnn"` | unset | File manager for `/browse`. |
| `tool_output` | `"compact"`, `"full"` | `"full"` | Inline tool calls: one line each, or in full. The transcript view always shows them in full. A folder setting beats this key. |
| `tool_output_max_lines` | integer | unset | Most lines of tool output shown inline; head and tail stay. 0 or less shows everything. A folder setting beats this key. |
| `pinned_plan_drawer` | boolean | `false` | Show plan updates in a drawer instead of history cells. |
| `thoughts_in_history` | boolean | `true` | Show agent thoughts as history blocks. `false`: only in the working indicator. |
| `custom_working_messages` | boolean | `true` | Rotate messages while the agent works. |
| `custom_working_message_list` | array of strings | `[]` | Messages to rotate. Empty: the built-in list. |
| `hotkeys` | table | | See [`[tui.hotkeys]`](#tuihotkeys). |
| `footer_segments` | table | | See [`[tui.footer_segments]`](#tuifooter_segments). |
| `footer_layout` | table | | See [`[tui.footer_layout]`](#tuifooter_layout). |

### `[tui.hotkeys]`

A binding is `[modifier+]key`: modifier `ctrl`, `alt`, or `shift`; key a
character, `enter`, `esc`, `f1` to `f12`, and similar names. Bindings are
lowercased. `"none"` unbinds the action.

| Key | Default | Action |
|---|---|---|
| `open_transcript` | `ctrl+t` | Open the transcript pager |
| `open_editor` | `ctrl+g` | Compose in an external editor |
| `move_backward_char` | `ctrl+b` | Cursor one character back |
| `move_forward_char` | `ctrl+f` | Cursor one character forward |
| `move_beginning_of_line` | `ctrl+a` | Cursor to line start |
| `move_end_of_line` | `ctrl+e` | Cursor to line end |
| `move_backward_word` | `alt+b` | Cursor one word back |
| `move_forward_word` | `alt+f` | Cursor one word forward |
| `delete_backward_char` | `ctrl+h` | Delete one character back |
| `delete_forward_char` | `ctrl+d` | Delete one character forward |
| `delete_backward_word` | `ctrl+w` | Delete one word back |
| `kill_to_end_of_line` | `ctrl+k` | Cut to line end |
| `kill_to_beginning_of_line` | `ctrl+u` | Cut to line start |
| `yank` | `ctrl+y` | Paste cut text |
| `history_search` | `ctrl+r` | Search prompt history |
| `toggle_plan_drawer` | `alt+p` | Collapse or expand the plan drawer |
| `previous_session` | `alt+,` | Previous session |
| `next_session` | `alt+.` | Next session |
| `next_waiting_session` | `alt+down` | Next session waiting on you (empty prompt only) |
| `open_session_list` | `alt+s` | Open the session list |
| `parent_session` | `alt+up` | From a delegated child to its parent session |
| `toggle_screen` | `ctrl+l` | Switch between the inline and the full-screen view |

### `[tui.footer_segments]`

Each key is a boolean that shows or hides one segment.

| Key | Default | Shows |
|---|---|---|
| `prompt_summary` | `false` | Task summary |
| `session_title` | `false` | Title the agent gave the session |
| `vim_mode` | `true` | `NORMAL` or `INSERT`, only when vim mode is on |
| `git_branch` | `true` | Git branch |
| `worktree_name` | `true` | Worktree name |
| `git_stats` | `false` | Lines added and removed |
| `context` | `true` | Context window use, such as `27% / 128k` |
| `context_used_percent` | `false` | Context used, percent |
| `context_remaining_percent` | `false` | Context left, percent |
| `context_used_tokens` | `false` | Context used, tokens |
| `context_remaining_tokens` | `false` | Context left, tokens |
| `context_window_tokens` | `false` | Context window size |
| `approval_mode` | `false` | Permission mode |
| `skillset` | `false` | Active skillset |
| `nori_version` | `false` | Skillset version |
| `token_usage` | `false` | Total tokens used |
| `mode_indicator` | `true` | Agent mode, such as `[ Plan ]` |
| `cloud_session` | `true` | Cloud session name, only in `nori cloud` |

### `[tui.footer_layout]`

Each key is a placement. Its value is a list of items. An item is a segment
key from the table above, or `{ format = "..." }`. A format is text with
`{segment_key}` placeholders; `{{` and `}}` are literal braces. An unknown
segment or a malformed format is a load error. Listing a segment in one
placement removes it from the others.

| Key | Default |
|---|---|
| `footer_left` | `cloud_session`, `prompt_summary`, `session_title`, `vim_mode`, `git_stats`, `context_used_percent`, `context_remaining_percent`, `context_used_tokens`, `context_remaining_tokens`, `context_window_tokens`, `approval_mode`, `skillset`, `nori_version`, `token_usage` |
| `footer_right` | `git_branch`, `worktree_name`, `context` |
| `textarea_top_left` | empty |
| `textarea_top_right` | `mode_indicator` |
| `textarea_bottom_left` | empty |
| `textarea_bottom_right` | empty |

A segment renders only when `[tui.footer_segments]` enables it. The one
exception is `approval_mode`, which always shows Ask and Approve all.

## `[projects."<path>"]`

One table per folder. `<path>` is an absolute path. Selection rules:
[Folder trust](#folder-trust).

| Key | Type | Default | Effect |
|---|---|---|---|
| `trust_level` | `"trusted"`, `"untrusted"` | unset | Default `approval_policy` for the folder. |
| `tool_output` | `"compact"`, `"full"` | unset | Beats `[tui] tool_output`. |
| `tool_output_max_lines` | integer | unset | Beats `[tui] tool_output_max_lines`. |

## `[host]`

Unknown keys are errors. Keys marked **managed** apply only when
`managed = true`; their meaning is in
`nori-monorepo: docs/specs/nori-cli/host-contract.md`.

| Key | Type | Default | Effect |
|---|---|---|---|
| `managed` | boolean | `false` | Run `nori host` as a managed host. |
| `default_mode` | `"ask"`, `"agent"`, `"approve-all"` | unset | Permission mode for sessions clients start with ACP `session/new`. Unset: each folder's `approval_policy`. |
| `max_clients` | integer | `16` | Clients the background host serves at once. Values below 1 mean 1. |
| `max_sessions` | integer | `8` | **Managed.** Most managed launches at once. Must be positive. The local host uses `[tui] max_sessions`. |
| `side_sessions` | boolean | `false` | **Managed.** Allow side sessions. |
| `side_confinement` | `"preferred"`, `"required"` | `"preferred"` | **Managed.** `preferred` starts a side session without the OS read-only launch when the machine cannot provide it, and says so. `required` refuses it. |
| `bindings_root` | path | `$NORI_HOME/hosted-sessions` | **Managed.** Root folder of shared-layout launches. Must exist. Cannot be combined with `dedicated_roots`. |
| `dedicated_roots` | table | unset | **Managed.** One fixed launch. Required by `nori host --stdio --launch`. |

### `[host.dedicated_roots]`

Unknown keys are errors. All keys are required.

| Key | Type | Effect |
|---|---|---|
| `binding_id` | string | The one binding this host serves. |
| `cwd` | absolute path | Session folder. Must exist. |
| `home` | absolute path | Home folder for the agent. Must exist. |

## `[mcp_servers.<name>]`

One table per MCP server. Nori forwards enabled servers to each agent; see
[mcp.md](mcp.md). A server is stdio when it has `command`, HTTP when it has
`url`. A table with neither, or with keys of the other transport, is a load
error.

| Key | Transport | Type | Default | Effect |
|---|---|---|---|---|
| `enabled` | both | boolean | `true` | `false` stops forwarding the server. |
| `command` | stdio | string | required | Program to run. |
| `args` | stdio | array of strings | `[]` | Arguments. |
| `env` | stdio | table | unset | Variables to set. |
| `env_vars` | stdio | array of strings | `[]` | Variables copied from Nori's environment. Missing ones are skipped with a log warning. |
| `url` | HTTP | string | required | Streamable HTTP endpoint. |
| `bearer_token_env_var` | HTTP | string | unset | Variable that holds a bearer token for `Authorization`. When unset or missing, Nori uses stored MCP OAuth tokens. |
| `http_headers` | HTTP | table | unset | Static headers. |
| `env_http_headers` | HTTP | table: header to variable | unset | Headers whose values come from variables. |
| `client_id` | HTTP | string | unset | OAuth client ID for `/mcp` sign-in. |
| `client_secret_env_var` | HTTP | string | unset | Variable that holds the OAuth client secret. |
| `startup_timeout_sec` | both | number | unset | **No effect.** ACP has no field for it. |
| `startup_timeout_ms` | both | integer | unset | **No effect.** Alias of `startup_timeout_sec` in milliseconds. |
| `tool_timeout_sec` | both | number | unset | **No effect.** |
| `enabled_tools` | both | array of strings | unset | **No effect.** |
| `disabled_tools` | both | array of strings | unset | **No effect.** |

`bearer_token` (a literal token) is an error for both transports.

## `[hooks]`

Each key is a list of script paths. A leading `~/` expands to the home
folder. Scripts run in list order, each limited by `[tui] script_timeout`.
The interpreter follows the extension: `.sh` runs with `bash`, `.py` with
`python3`, `.js` with `node`; other files run directly. Keys starting with
`async_` start their scripts without waiting.

| Key | Runs |
|---|---|
| `session_start`, `async_session_start` | When a session starts |
| `session_end`, `async_session_end` | When a session ends |
| `pre_user_prompt`, `async_pre_user_prompt` | Before a prompt goes to the agent |
| `post_user_prompt`, `async_post_user_prompt` | After a prompt goes to the agent |
| `post_agent_response`, `async_post_agent_response` | After the agent finishes its answer |
| `pre_tool_call`, `async_pre_tool_call` | **No effect.** Parsed but never run. |
| `post_tool_call`, `async_post_tool_call` | **No effect.** Parsed but never run. |
| `pre_agent_response`, `async_pre_agent_response` | **No effect.** Parsed but never run. |

Scripts get `NORI_HOOK_EVENT` and, by event, `NORI_HOOK_PROMPT_TEXT` or
`NORI_HOOK_RESPONSE_TEXT`. Each stdout line is routed by its prefix:

| Prefix | Effect |
|---|---|
| `::output::` | Text in the TUI |
| `::output-warn::` | Warning in the TUI |
| `::output-error::` | Error in the TUI |
| `::context::` | Added before the next prompt to the agent |
| none | Written to the log |

## `[acp_proxy]`

| Key | Type | Default | Effect |
|---|---|---|---|
| `enabled` | boolean | `false` | Record raw ACP messages to `$NORI_HOME/acp-wire/`. See [storage.md](storage.md). |

## `[cloud]`

| Key | Type | Default | Effect |
|---|---|---|---|
| `broker_url` | string | unset | Passed to `nori-handroll` as `NORI_BROKER_URL` by `nori cloud`. Handroll decides whether it wins over its own settings. |

## `[features]`

| Key | Type | Default | Effect |
|---|---|---|---|
| `skillsets_v2` | boolean | `false` | The skillset picker and footer use the skillsets v2 CLI (list, layer, unlayer). |
| `enable_experimental_windows_sandbox` | boolean | `false` | Windows only. Turn on the experimental Windows sandbox. |

## `[notice]`

Nori sets these when you dismiss a warning or finish a one-time migration.

| Key | Type | Default | Effect |
|---|---|---|---|
| `hide_full_access_warning` | boolean | unset | Hide the full-access warning. |
| `hide_world_writable_warning` | boolean | unset | Hide the world-writable folder warning. |
| `approval_presets_migrated` | boolean | unset | Records that `nori` moved a retired `/approvals` preset pair to `[host] default_mode`. |

## `[cli]`

| Key | Type | Default | Effect |
|---|---|---|---|
| `first_launch_complete` | boolean | unset | Written when first-launch onboarding ends. **No effect:** Nori treats a launch as the first when `config.toml` does not exist. |

## `[auth.<agent>]`

`<agent>` is a canonical agent slug. Unknown keys are errors. Behaviour:
[auth.md](auth.md).

| Key | Type | Default | Effect |
|---|---|---|---|
| `prefer_api_key` | boolean | `false` | Pass the saved API key to the agent at launch. |
| `api_billing_confirmed` | boolean | `false` | The billing warning was accepted. |

## `[[token_providers]]`

One table per named API connection. Unknown keys are errors. All keys are
required. Behaviour: [auth.md](auth.md).

| Key | Type | Effect |
|---|---|---|
| `name` | string | Letters, digits, `-`, `_`. Unique. |
| `preset` | `"openrouter"`, `"subconscious"`, `"fireworks"`, `"cloudflare"`, `"custom"` | Gateway. Every preset except `custom` fixes the allowed `base_url`. |
| `protocol` | `"openai-completions"`, `"openai-responses"`, `"anthropic-messages"` | Wire protocol. Must suit the preset. |
| `base_url` | string | HTTP(S) URL without credentials, query, or fragment. |
| `env_key` | string | Name of the variable that holds the API key. The key itself is never stored here. |
| `model` | string | Model ID. |

Nori also reads the older table form `[token_providers.<name>]` written for
the Pi agent and converts it on load.

## `[[agents]]`

One table per custom agent. Field meanings: [agents.md](agents.md).

| Key | Type | Required |
|---|---|---|
| `name` | string | yes |
| `slug` | string | yes |
| `distribution` | table with exactly one of `local`, `npx`, `bunx`, `pipx`, `uvx` | yes |
| `distribution.local.command` | string | with `local` |
| `distribution.local.args` | array of strings | no |
| `distribution.local.env` | table | no |
| `distribution.local.wrapper` | array of strings | no |
| `distribution.local.env_remove` | array of strings | no |
| `distribution.local.runtime_dir` | absolute path | no |
| `distribution.<npx,bunx,pipx,uvx>.package` | string | with that variant |
| `distribution.<npx,bunx,pipx,uvx>.args` | array of strings | no |
| `context_window_size` | integer | no |
| `auth_hint` | string | no |
| `transcript_base_dir` | string | no |
| `model_override.env`, `model_override.arg` | string | no |

## `[acp_adapters.<slug>]`

Unknown keys are errors. Behaviour: [agents.md](agents.md).

| Key | Type | Default | Effect |
|---|---|---|---|
| `command` | string | unset | Command line that launches the adapter, split with shell rules. |
| `version` | string | unset | Package version, range, or dist-tag instead of `latest` for `npx` or `bunx` launches. |

## Keys Nori writes

| Key | Written by |
|---|---|
| `agent` | Agent picker |
| `default_models.<agent>` | Model picker |
| `projects."<path>".trust_level` | Trust prompt |
| `projects."<path>".tool_output` | Trust prompt |
| `auth.<agent>.prefer_api_key`, `auth.<agent>.api_billing_confirmed` | `nori auth prefer-api-key`, `nori auth prefer-login` |
| `mcp_servers` | `/mcp` |
| `browser_profile`, `acp_proxy.enabled` | `/config` |
| `tui.*`, `tui.hotkeys.*`, `tui.footer_segments.*` | `/config` and its pickers |
| `features.enable_experimental_windows_sandbox` | Windows sandbox prompt |
| `notice.*` | Dismissed warnings |
| `cli.first_launch_complete` | First-launch onboarding |

## `$NORI_HOME/.env`

`nori` loads this file into its environment at startup. Names starting with
`CODEX_` are skipped. See [commands.md](commands.md#nori_homeenv).

## Where `main` differs

- The MCP keys marked **No effect** load without a warning.
