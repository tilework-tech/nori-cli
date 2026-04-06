# Noridoc: codex-acp

Path: @/codex-rs/acp

### Overview

The ACP crate implements the Agent Client Protocol integration for Nori. It manages spawning ACP-compliant agent subprocesses (like Claude Code, Codex, or Gemini), communicating with them over JSON-RPC, and normalizing ACP session-domain data into `nori_protocol::ClientEvent` for the TUI and transcript layers. `codex_protocol::EventMsg` remains only for narrow control-plane concerns that are not ACP session semantics.

### How it fits into the larger codebase

```
nori-tui
    |
    v
codex-acp <---> ACP Agent subprocess (claude-agent-acp, codex-acp, gemini-cli)
    |
    v
nori-protocol (normalized ACP session events)
```

The ACP crate serves as a bridge between:
- The TUI layer (`@/codex-rs/tui/`) which displays UI and collects user input
- External ACP agent processes installed via npm (@anthropic-ai/claude-code, @openai/codex, @google/gemini-cli)
- `nori-protocol`, which is the canonical ACP session event vocabulary used by live rendering and transcript recording
- The shared `codex-protocol` event stream, which is still used for control-plane signals such as warnings, hook output, prompt summaries, shutdown, and other app-level notifications
- `SessionRuntime` in `@/codex-rs/nori-protocol/`, which is now the ACP backend's single source of truth for prompt state, load state, queued prompts, permission ownership, and final assistant-message assembly

Key files:
- `registry.rs` - Agent configuration and npm package detection
- `connection/` - SACP v10-based subprocess spawning and JSON-RPC communication
- `translator.rs` - User input to ACP `ContentBlock` conversion and related parsing helpers
- `backend/mod.rs` - Implements `ConversationClient` trait from codex-core and emits normalized ACP session events
- `transcript_discovery.rs` - Discovers transcript files for external agents
- `auto_worktree.rs` - Orchestrates automatic git worktree creation and summary-based renaming

### Core Implementation

**Agent Registry** (`registry.rs`):

The registry is **data-driven** and **agent-centric**: it combines built-in agents (Claude Code, Codex, Gemini) with user-defined custom agents from `[[agents]]` entries in `config.toml`. The global registry is stored in a `RwLock<Option<Vec<RegisteredAgent>>>` (`AGENT_REGISTRY`) and initialized once at startup via `initialize_registry()`, which is called from `@/codex-rs/tui/src/lib.rs` after config loading. If not initialized, `get_registry()` falls back to built-in defaults.

`RegisteredAgent` is the unified representation for both built-in and custom agents:

| Field | Built-in Agent | Custom Agent |
|-------|---------------|--------------|
| `kind` | `Some(AgentKind)` | `None` |
| `distribution` | `None` (uses auto-detection) | `Some(ResolvedDistribution)` |
| `context_window_size` | From `AgentKind::context_window_size()` | From TOML config (optional) |
| `auth_hint` | From `AgentKind::auth_hint()` | From TOML config (optional) |
| `transcript_base_dir` | From `AgentKind::transcript_base_dir()` | From TOML config (optional) |

**Registry construction** (`build_registry()`): starts with `build_default_agents()` (the three built-ins), then iterates custom agents. If a custom agent's slug matches a built-in slug, it **overrides** the built-in entry in-place. Otherwise it is appended. Duplicate slugs among custom agents are rejected with an error.

**Agent config resolution** (`get_agent_config()`): resolves an agent name to `AcpAgentConfig` using a priority chain:

```
agent_name (normalized to lowercase)
    |
    +--> Mock agents (debug builds only)
    |
    +--> Registry lookup: if slug has a custom distribution
    |      --> use ResolvedDistribution directly (npx/bunx/pipx/uvx/local)
    |
    +--> Built-in auto-detection: AgentKind::from_slug()
    |      --> detect_preferred_package_manager() to choose npx vs bunx
    |      --> use AgentKind::acp_package() for the adapter package
    |
    +--> Error: unknown agent
```

Built-in agents use `detect_preferred_package_manager()` which checks `NORI_MANAGED_BY_BUN`/`NORI_MANAGED_BY_NPM` env vars, then falls back to checking if `bun` is in PATH, defaulting to `npx`. Custom agents bypass auto-detection entirely and use their literal `ResolvedDistribution`.

`AcpAgentConfig` carries `display_name` and `install_hint` as direct `String` fields (rather than deriving them from `AgentKind` methods), so both built-in and custom agents can be handled uniformly by `session.rs` and `spawn_and_relay.rs`. The `install_hint` field contains a distribution-appropriate install command (e.g. `npm install -g @pkg` for npx, `uv tool install pkg` for uvx, `ensure '/path/to/cmd' is in your PATH` for local). The `context_window_size()` and `transcript_base_dir()` methods on `AcpAgentConfig` look up values from the registry by `provider_slug`.

**Serialized ACP session runtime** (`backend/session_reducer.rs`, `backend/session_runtime_driver.rs`):

ACP session-domain state now flows through a single serialized reducer. `SessionDriver` owns a `SessionRuntime` plus `ClientEventNormalizer`, accepts ordered `InboundEvent` values (`PromptSubmit`, `CancelSubmit`, `LoadSubmit`, `Notification`, `PromptResponse`, `PermissionRequest`, etc.), and returns normalized `ClientEvent` projections plus ACP side effects. This removed the old split where prompt tasks emitted lifecycle events directly while a separate notification relay normalized deltas.

`SessionRuntime` is the authoritative model for:
- whether the ACP session is idle, loading, or in a prompt turn
- queued user prompts and compact prompts waiting behind an active request
- request-local message assembly for assistant/reasoning streams
- tool snapshot ownership via `owner_request_id`
- pending permission request ownership and cancellation cleanup
- final assistant message extraction used for `TurnLifecycle::Completed { last_agent_message }`

The live backend path in `user_input.rs`, `submit_and_ops.rs`, `spawn_and_relay.rs`, and `session.rs` all feed reducer events into the same runtime. `resume_session()` uses the same reducer during `session/load`, buffering replay `ClientEvent`s from reducer output and then carrying the resulting `SessionDriver` state into the live backend once setup completes.

**Custom Agent TOML Schema** (`config/types/mod.rs`):

Custom agents are defined under `[[agents]]` in `config.toml`. Each entry is deserialized as `AgentConfigToml`:

```toml
[[agents]]
name = "Kimi"                        # Display name
slug = "kimi"                        # Machine identifier
context_window_size = 128000         # Optional
auth_hint = "Set KIMI_API_KEY"       # Optional
transcript_base_dir = ".kimi/logs"   # Optional, relative to home

[agents.distribution.uvx]            # Exactly one distribution variant
package = "kimi-cli"
args = ["acp"]
```

`AgentDistributionToml` requires exactly one of these distribution variants to be set (validated by `resolve()`):

| Variant | TOML Key | Command Generated | Use Case |
|---------|----------|-------------------|----------|
| `LocalDistribution` | `local` | `{command} {args...}` with env vars | Local binary |
| `PackageDistribution` (npx) | `npx` | `npx {package} {args...}` | Node.js via npm |
| `PackageDistribution` (bunx) | `bunx` | `bunx {package} {args...}` | Node.js via bun |
| `PackageDistribution` (pipx) | `pipx` | `pipx run {package} {args...}` | Python via pipx |
| `PackageDistribution` (uvx) | `uvx` | `uvx {package} {args...}` | Python via uv |

`resolve()` returns `ResolvedDistribution` enum or errors if zero or multiple variants are set.

**Nori Config Path Resolution** (`config/`):

The config module provides the **canonical source of truth** for Nori home path resolution:
- `find_nori_home()`: Returns `~/.nori/cli` or `$NORI_HOME` if set
- `NORI_HOME_ENV`: Environment variable name (`"NORI_HOME"`)
- `NORI_HOME_DIR`: Default relative path (`".nori/cli"`)
- `CONFIG_FILE`: Config filename (`"config.toml"`)
- `DEFAULT_AGENT`: Default agent (`"claude-code"`)

**Agent Config Field Resolution:**

| Field | Purpose | Persistence |
|-------|---------|-------------|
| `agent` | User's persistent agent preference | Saved to config.toml |
| `active_agent` | Active agent for current session (CLI override > config agent > persisted agent) | Not persisted |

**Notification Configuration** (`config/types/mod.rs`):

Three config enums control notification behavior, all stored in the `[tui]` section of `config.toml`:

| Enum | TOML Key | Default | Controls |
|------|----------|---------|----------|
| `TerminalNotifications` | `terminal_notifications` | `Enabled` | OSC 9 escape sequences sent by the TUI (`chatwidget.rs`) |
| `OsNotifications` | `os_notifications` | `Enabled` | Native desktop notifications via `notify-rust` (wired in `backend/mod.rs` to `UserNotifier::new()`) |
| `NotifyAfterIdle` | `notify_after_idle` | `FiveSeconds` (`"5s"`) | Duration to wait before firing an idle notification; `Disabled` suppresses the timer entirely |

`NotifyAfterIdle` accepts serde-renamed string values: `"5s"`, `"10s"`, `"30s"`, `"60s"`, `"disabled"`. Its `as_duration()` method returns `Option<Duration>` (`None` when `Disabled`). The idle timer in `backend/mod.rs` is conditionally spawned only when `as_duration()` returns `Some` -- when `Disabled`, no timer task or abort handle is created.

The `AcpBackendConfig` struct carries both `os_notifications` and `notify_after_idle` so the backend can configure the `UserNotifier` and the idle timer respectively. Terminal notifications flow separately through `codex-core`'s `Config::tui_notifications` bool to the TUI's `ChatWidget::notify()` method.


**Hotkey Configuration** (`config/types/mod.rs`):

Hotkeys are user-configurable keyboard shortcuts stored under `[tui.hotkeys]` in `config.toml`. The config layer defines four types:

| Type | Purpose |
|------|---------|
| `HotkeyAction` | Enum of bindable actions with display names, descriptions, TOML keys, and default bindings. Covers app-level actions (OpenTranscript, OpenEditor), emacs-style editing actions (cursor movement, deletion, kill/yank) used by the textarea, and UI trigger actions (HistorySearch) |
| `HotkeyBinding` | String-based key representation (e.g. `"ctrl+t"`, `"alt+g"`, `"none"` for unbound). Serializes/deserializes via serde for TOML roundtripping |
| `HotkeyConfigToml` | TOML deserialization struct with `Option<HotkeyBinding>` fields for each action |
| `HotkeyConfig` | Resolved config with defaults applied via `from_toml()`. Provides `binding_for()`, `set_binding()`, and `all_bindings()` accessors |

The binding string format is kept terminal-agnostic (no crossterm dependency in the config crate). The TUI layer in `@/codex-rs/tui/src/nori/hotkey_match.rs` handles conversion between binding strings and crossterm `KeyEvent` types. `HotkeyConfig` is carried on `NoriConfig` and resolved during config loading in `loader.rs`.

**Vim Mode Configuration** (`config/types/mod.rs`):

The `vim_mode` field in `TuiConfigToml` and `NoriConfig` uses the `VimEnterBehavior` enum, which doubles as both the vim mode on/off switch and the Enter key behavior selector. Stored under `[tui]` in `config.toml`:

| Field | TOML Key | Default | Controls |
|-------|----------|---------|----------|
| `vim_mode` | `vim_mode` | `"off"` | Vim mode and Enter key behavior: `"newline"` (Enter inserts newline in INSERT, submits in NORMAL), `"submit"` (Enter submits in INSERT, inserts newline in NORMAL), or `"off"` (vim disabled) |

`VimEnterBehavior` has a custom `Deserialize` implementation that accepts both booleans and strings for backwards compatibility: `true` maps to `Submit`, `false` maps to `Off`. New string values are `"newline"`, `"submit"`, `"off"`. Serialization always writes the string form. The enum provides `is_enabled()` (returns `true` for any variant except `Off`), `display_name()` for TUI display, `toml_value()` for persistence, and `all_variants()` for building picker UIs.

The TUI layer (`@/codex-rs/tui/`) handles the vim mode state machine and propagation. The `VimEnterBehavior` flows through the config pipeline: `NoriConfig` -> `App` -> `ChatWidget` -> `BottomPane` -> `ChatComposer`, where it controls how Enter key presses are routed in the key handler.

**Script Timeout Configuration** (`config/types/mod.rs`):

The `ScriptTimeout` type represents a configurable duration for custom prompt script execution. It stores both the raw string (for TOML round-tripping and display) and the parsed `Duration`. Stored under `[tui]` in `config.toml`:

| Field | TOML Key | Default | Controls |
|-------|----------|---------|----------|
| `script_timeout` | `script_timeout` | `"30s"` | Maximum execution time for custom prompt scripts before they are killed |

Supported suffixes: `s` (seconds), `m` (minutes). Bare numbers are treated as seconds. `all_common_values()` provides picker options: 10s, 30s, 1m, 2m, 5m. The setting is resolved in `loader.rs` with `unwrap_or_default()` (30 seconds).

**Loop Count Configuration** (`config/types/mod.rs`):

The `loop_count` field on `NoriConfigToml` and `NoriConfig` controls how many times the TUI re-runs the first user prompt in fresh conversation sessions. Stored as a top-level key in `config.toml`:

| Field | TOML Key | Default | Controls |
|-------|----------|---------|----------|
| `loop_count` | `loop_count` | `None` (disabled) | Number of fresh-session iterations of the first prompt. Values > 1 enable looping; `None` or `0` disables it |

The setting is resolved in `loader.rs` by passing `toml.loop_count` directly. The TUI layer (`@/codex-rs/tui/`) orchestrates the loop lifecycle -- the config layer only stores the value.

**Auto-Worktree Configuration** (`config/types/mod.rs`):

The `auto_worktree` field controls whether and how the TUI creates a git worktree at session start for process isolation. It is an `AutoWorktree` enum stored under `[tui]` in `config.toml`:

| Variant | TOML Value | Behavior |
|---------|------------|----------|
| `Automatic` | `"automatic"` (or legacy `true`) | Always create a worktree at session start |
| `Ask` | `"ask"` | Show a TUI popup at session start asking the user whether to create a worktree |
| `Off` | `"off"` (or legacy `false`) | Never create a worktree automatically |

The default is `Off`. The enum has a custom serde `Deserialize` implementation that accepts both string values (`"automatic"`, `"ask"`, `"off"`) and boolean values (`true` maps to `Automatic`, `false` maps to `Off`) for backwards compatibility with config files written before the enum existed. `Serialize` always writes the string form via `toml_value()`.

Helper methods on `AutoWorktree`:
- `display_name()` -- human-readable label for the TUI config picker (e.g. `"Automatic"`, `"Ask"`, `"Off"`)
- `toml_value()` -- string written to config.toml (e.g. `"automatic"`, `"ask"`, `"off"`)
- `all_variants()` -- returns all three variants in order, used to build the picker UI
- `is_enabled()` -- returns `true` for `Automatic` and `Ask`, `false` for `Off`; used by the backend to gate worktree branch renaming

| Field | TOML Key | Default | Controls |
|-------|----------|---------|----------|
| `auto_worktree` | `auto_worktree` | `Off` | Worktree creation behavior at session start |
| `skillset_per_session` | `skillset_per_session` | `false` | When enabled, each session gets its own skillset. Independent of `auto_worktree` -- does not force it on |
| `file_manager` | `file_manager` | `None` | Terminal file manager for the `/browse` command |
| `pinned_plan_drawer` | `pinned_plan_drawer` | `false` | When enabled, plan updates render in a pinned viewport drawer instead of scrollable history cells |

The `FileManager` enum (`types/mod.rs`) represents supported terminal file managers for the `/browse` slash command. Stored under `[tui]` in `config.toml` as a kebab-case string. Variants: `Vifm`, `Ranger`, `Lf`, `Nnn`. Each variant provides:
- `command_name()` -- binary name to invoke (e.g. `"vifm"`, `"ranger"`)
- `chooser_args(output_path)` -- CLI arguments that put the file manager into chooser mode, writing the selected file path to a temp file. Each file manager uses a different flag convention (e.g. vifm uses `--choose-files`, ranger uses `--choosefile=`, lf uses `-selection-path`, nnn uses `-p`)
- `display_name()` -- human-friendly label for the config picker

The field defaults to `None` (no file manager configured). The TUI layer (`@/codex-rs/tui/`) checks this value when the user invokes `/browse` and shows an error if unset, directing the user to `/config` to choose one. The `FileManager` type is re-exported from `codex_acp` for use by the TUI.

Both `auto_worktree` and `skillset_per_session` are resolved independently in `loader.rs`. The TUI layer (`@/codex-rs/tui/`) matches on the `AutoWorktree` variant in `lib.rs`: `Automatic` calls `setup_auto_worktree()` immediately, `Ask` defers to a TUI popup (`worktree_ask.rs`), and `Off` skips entirely. The config layer stores the enum value -- all orchestration lives in `@/codex-rs/acp/src/auto_worktree.rs` and `@/codex-rs/tui/src/lib.rs`.

**Auto-Worktree Branch Renaming** (`auto_worktree.rs`, `backend/mod.rs`):

When auto-worktree is active (either via `Automatic` or the user confirming in `Ask` mode), the worktree is initially created with a random name (e.g., `auto/swift-oak-20260202-120000`). After the first user prompt's summary is generated, the git branch is renamed to reflect the summary (e.g., `auto/fix-auth-bug-20260202-120000`). The worktree directory path is left unchanged so that processes running inside it are not disrupted. This renaming is orchestrated inside `run_prompt_summary()` in `backend/mod.rs`:

1. The prompt summary is generated via a separate ACP connection (same as before)
2. If `auto_worktree.is_enabled()` and `auto_worktree_repo_root` is set, `rename_auto_worktree_branch()` is called in a blocking task
3. Only the branch is renamed via `git branch -m`; the directory stays at its original path

The `AcpBackend` stores `auto_worktree: AutoWorktree` and `auto_worktree_repo_root: Option<PathBuf>` to support the rename. The `is_enabled()` method returns `true` for both `Automatic` and `Ask` variants, since in both cases a worktree was actually created. The repo root is derived by the TUI layer from the worktree path (going up two directories from `{repo_root}/.worktrees/{name}`).


**Default Models Configuration** (`config/types/mod.rs`, `backend/mod.rs`):

Model preferences can be persisted per agent in the `[default_models]` table of `config.toml`. When a session starts, the configured default model is automatically applied if available:

| Field | TOML Section | Purpose |
|-------|--------------|---------|
| `default_models` | `[default_models]` | Maps agent slugs to model IDs (e.g., `claude-code = "haiku"`) |

The config flow is:
1. `NoriConfigToml.default_models` deserializes the `[default_models]` table from TOML (empty HashMap by default via `#[serde(default)]`)
2. `NoriConfig.default_models` stores the resolved map after config loading
3. `AcpBackendConfig.default_model` receives `Option<String>` via lookup by agent slug in `chatwidget/agent.rs`
4. `AcpBackend::spawn()` applies the model via `connection.set_model()` after session creation (behind `#[cfg(feature = "unstable")]`)

The model is only applied if:
- The feature `unstable` is enabled (model switching requires this feature)
- The default model is listed in the agent's `available_models` (checked against `model_state`)
- The session was successfully created

Failures to apply the default model (e.g., model unavailable, API error) produce warnings but do not block session startup. When users switch models via `/model` command, the TUI persists the selection by calling `ConfigEditsBuilder::set_default_model()` (see `@/codex-rs/core/docs.md`).

**Hooks System** (`config/types/mod.rs`, `hooks.rs`, `backend/mod.rs`):

Hooks allow users to run custom scripts at lifecycle boundaries. There are two flavors: **synchronous** hooks (blocking, executed sequentially) and **async** hooks (fire-and-forget, spawned via `tokio::spawn`). Both are configured under `[hooks]` in `config.toml`, are **fail-open** (failures produce warnings but do not halt operations), and share the same execution engine (`execute_hooks_with_env()` in `hooks.rs`) and interpreter detection. Synchronous hooks support output routing and context injection; async hooks route all output exclusively to tracing.

```toml
[hooks]
session_start = ["~/.nori/cli/hooks/start.sh"]
session_end = ["~/.nori/cli/hooks/cleanup.sh"]
pre_user_prompt = ["~/.nori/cli/hooks/pre-prompt.sh"]
post_user_prompt = ["~/.nori/cli/hooks/post-prompt.sh"]
pre_tool_call = ["~/.nori/cli/hooks/pre-tool.sh"]
post_tool_call = ["~/.nori/cli/hooks/post-tool.sh"]
pre_agent_response = ["~/.nori/cli/hooks/pre-response.sh"]
post_agent_response = ["~/.nori/cli/hooks/post-response.sh"]
```

Each synchronous hook has an async counterpart prefixed with `async_`:

```toml
[hooks]
async_session_start = ["~/.nori/cli/hooks/async-start.sh"]
async_session_end = ["~/.nori/cli/hooks/async-cleanup.sh"]
async_pre_user_prompt = ["~/.nori/cli/hooks/async-pre-prompt.sh"]
async_post_user_prompt = ["~/.nori/cli/hooks/async-post-prompt.sh"]
async_pre_tool_call = ["~/.nori/cli/hooks/async-pre-tool.sh"]
async_post_tool_call = ["~/.nori/cli/hooks/async-post-tool.sh"]
async_pre_agent_response = ["~/.nori/cli/hooks/async-pre-response.sh"]
async_post_agent_response = ["~/.nori/cli/hooks/async-post-response.sh"]
```

| Field | TOML Key | Default | Execution Point |
|-------|----------|---------|-----------------|
| `session_start_hooks` | `session_start` | `[]` | After backend construction, before `SessionConfigured` event |
| `session_end_hooks` | `session_end` | `[]` | On `Op::Shutdown`, before transcript recorder shutdown |
| `pre_user_prompt_hooks` | `pre_user_prompt` | `[]` | In `handle_user_input()`, before the prompt is sent to the agent |
| `post_user_prompt_hooks` | `post_user_prompt` | `[]` | After the entire turn completes (agent response + all tool calls finished) |
| `pre_tool_call_hooks` | `pre_tool_call` | `[]` | Inside the update handler task, when a `ToolCall` update arrives |
| `post_tool_call_hooks` | `post_tool_call` | `[]` | Inside the update handler task, when a `ToolCallUpdate` has `Completed` status |
| `pre_agent_response_hooks` | `pre_agent_response` | `[]` | Inside the update handler task, on the first non-empty `AgentMessageChunk` |
| `post_agent_response_hooks` | `post_agent_response` | `[]` | After the update handler completes, with the full accumulated response text |

**Hook execution timing within a turn:**

```
handle_user_input()
  |
  +--> pre_user_prompt hooks (main async context)
  |
  +--> [prompt sent to agent]
  |
  +--> spawned update handler task:
  |      |
  |      +--> pre_agent_response (on first text chunk)
  |      |
  |      +--> pre_tool_call (on each ToolCall)
  |      +--> post_tool_call (on each ToolCallUpdate with Completed status)
  |      |    (pre/post tool call may repeat for multiple tool calls)
  |      |
  |      +--> [stream ends]
  |
  +--> post_agent_response (after update handler joins, if text was accumulated)
  +--> post_user_prompt (after the turn completes)
```

**Environment variables passed to hook scripts:**

Each lifecycle hook receives `NORI_HOOK_EVENT` set to its hook name. Additional variables depend on the hook type:

| Hook | `NORI_HOOK_EVENT` | Additional Environment Variables |
|------|-------------------|----------------------------------|
| `pre_user_prompt` | `"pre_user_prompt"` | `NORI_HOOK_PROMPT_TEXT` |
| `post_user_prompt` | `"post_user_prompt"` | `NORI_HOOK_PROMPT_TEXT` |
| `pre_tool_call` | `"pre_tool_call"` | `NORI_HOOK_TOOL_NAME`, `NORI_HOOK_TOOL_ARGS` |
| `post_tool_call` | `"post_tool_call"` | `NORI_HOOK_TOOL_NAME`, `NORI_HOOK_TOOL_OUTPUT` |
| `pre_agent_response` | `"pre_agent_response"` | (none) |
| `post_agent_response` | `"post_agent_response"` | `NORI_HOOK_RESPONSE_TEXT` |
| `session_start` | (none) | (none) |
| `session_end` | (none) | (none) |

**Hook resolution:** `HooksConfigToml` deserializes the TOML `[hooks]` section. `resolve_hook_paths()` applies tilde expansion via `expand_tilde()` (using `dirs::home_dir()`) and converts strings to `PathBuf`s. The resolved paths are stored on `NoriConfig` and passed through `AcpBackendConfig` to the backend.

**Hook execution** (`hooks.rs`): `execute_hooks_with_env()` is the core execution function -- it runs scripts sequentially with a configurable timeout and injects environment variables into each child process. `execute_hooks()` is a thin wrapper that calls it with an empty env map. Interpreter is auto-detected by file extension:

| Extension | Interpreter |
|-----------|-------------|
| `.sh` | `bash` |
| `.py` | `python3` |
| `.js` | `node` |
| other/none | executed directly |

Hook failures are non-fatal. Failed hooks emit warning events to the TUI via the event channel. A failed hook does not prevent subsequent hooks from executing.

**Image input handling** (`translator.rs`, `backend/mod.rs`):

`handle_user_input()` separates text items from image items during the extraction loop. Text is accumulated into `prompt_text` for use by hooks, compact summary, and transcript recording. Image items (`UserInput::Image` and `UserInput::LocalImage`) are collected separately and converted to ACP `ContentBlock::Image` via `translator::user_inputs_to_content_blocks()`:

- `UserInput::Image` carries a data URI (`data:<mime>;base64,<data>`), which is parsed into mime type and base64 data
- `UserInput::LocalImage` carries a file path; the file is read and base64-encoded, with MIME type inferred from the file extension (defaults to `image/png`)

The resulting image blocks are appended after any text block in the prompt vector sent to the agent. A turn with only images (no text) is permitted; the empty check requires both `prompt_text` and `image_blocks` to be empty before returning early.

**Hook output routing** (`hooks.rs`, `backend/mod.rs`):

Hook scripts can route their stdout lines to different destinations by using line prefixes. `parse_hook_output()` parses each non-empty line of stdout:

| Prefix | Destination | `HookOutputLine` variant |
|--------|-------------|--------------------------|
| (none) | `tracing::info!` | `Log` |
| `::output::` | Plain white text in TUI (`PlainHistoryCell`) | `Output` |
| `::output-warn::` | Yellow warning text in TUI | `OutputWarn` |
| `::output-error::` | Red error text in TUI | `OutputError` |
| `::context::` | Accumulated and prepended to next user prompt | `Context` |

The routing is handled by `route_hook_results()` in `backend/mod.rs`, which is shared across all hook types. It sends `EventMsg::HookOutput` events (from `@/codex-rs/protocol/`) for output/warn/error lines, and accumulates context lines into `pending_hook_context` on the `AcpBackend`.

**Hook context injection:** Context lines (`::context::`) are accumulated into a `pending_hook_context: Arc<Mutex<Option<String>>>` field on `AcpBackend`. When the next user prompt is submitted via `handle_user_input()`, the accumulated context is consumed and prepended to the user prompt as raw text: `{context}\n{prompt}`. Hook context is applied before compact summary injection so that the `SUMMARY_PREFIX` framing instruction always comes first in the final prompt. Only `pre_user_prompt` and `post_user_prompt` hooks pass the context accumulator to `route_hook_results()`; other hooks pass `None`.

**Session end hook timing:** During `Op::Shutdown`, end hooks execute and their output is routed via `route_hook_results()` before `ShutdownComplete` is sent, so the TUI can still display hook output. Context lines are irrelevant during shutdown, so `None` is passed for the context accumulator.

**Async (fire-and-forget) hooks** (`hooks.rs`, `backend/mod.rs`):

Async hooks fire at the same lifecycle points as their synchronous counterparts, but run in the background without blocking the caller. Key differences from synchronous hooks:

- Dispatched via `execute_hooks_fire_and_forget()`, which calls `tokio::spawn` and returns immediately
- All script output (stdout/stderr) is routed to `tracing::info!`/`tracing::warn!` only -- no TUI output routing, no `::context::` injection
- The spawned task takes owned `Vec<PathBuf>` and `HashMap<String, String>` (moved into the future) to avoid lifetime issues
- Shares the same `script_timeout` and interpreter detection as synchronous hooks
- Both sync and async hooks for the same lifecycle point are dispatched at the same location in `backend/mod.rs`; sync runs first (blocking), then async fires in the background
- `async_session_start` hooks are dispatched during backend construction (not stored on `AcpBackend`); the remaining 7 async hook vectors are stored as fields on `AcpBackend`
- Receive the same environment variables (`NORI_HOOK_EVENT`, `NORI_HOOK_PROMPT_TEXT`, etc.) as their synchronous counterparts

**Message History** (`message_history.rs`):

- File location: `~/.nori/cli/history.jsonl`
- Entry schema: `{"session_id":"<uuid>","ts":<unix_seconds>,"text":"<message>"}`
- Uses advisory file locking for concurrent write safety
- `HistoryPersistence` policy: `SaveAll` (default) or `None` (privacy mode)
- `search_entries()`: Reads all entries from the JSONL file, deduplicates by text (keeping the most recent occurrence of each), sorts newest-first, and returns up to `max_results` entries. Used by the `Op::SearchHistoryRequest` handler to provide history data for the TUI's Ctrl+R reverse-search popup.

**Custom Prompts** (`backend/mod.rs`):

When the TUI sends `Op::ListCustomPrompts`, the ACP backend discovers prompt files (`.md`, `.sh`, `.py`, `.js`) from `{nori_home}/commands/` and returns them via `ListCustomPromptsResponse`. This reuses `codex_core::custom_prompts::discover_prompts_in()` from `@/codex-rs/core/src/custom_prompts.rs` for filesystem discovery. Markdown files have their frontmatter parsed for metadata; script files are returned with empty content and a `CustomPromptKind::Script` kind. The handler spawns an async task and sends results through the existing `event_tx` channel. The TUI receives these prompts in `ChatWidget::on_list_custom_prompts()` and populates the slash command popup.

Note: The ACP backend uses `{nori_home}/commands/` (e.g., `~/.nori/cli/commands/`) rather than `~/.codex/prompts/` which is used by the HTTP/codex-core backend.

**Transcript Discovery** (`transcript_discovery.rs`):

Detects the current running transcript file when Nori runs within an external agent environment. Used by the TUI's `SystemInfo` module (see `@/codex-rs/tui/src/system_info.rs`) to display token usage in the footer.

**Unified Discovery Method:**

All agents use a single unified discovery approach that searches for the session's first user message within transcript files. This avoids coupling to any specific agent's JSON schema.

```
discover_transcript_for_agent_with_message(cwd, agent, first_message)
    |
    v
AgentKind::transcript_base_dir()  -->  Base search directory
    |
    v
find_transcript_by_shell_search(base_dir, normalized_message)
    |
    v
search_with_rg() or search_with_grep()  -->  Files containing message
    |
    v
Pick most recently modified file within 2 days
```

Entry points:
- `discover_transcript_for_agent_with_message()` - Required entry point using first-message matching
- `discover_transcript_for_agent()` - Deprecated, always returns `NoSessionsFound` error

**Agent Transcript Base Directories:**

Each agent's base directory is provided by `AgentKind::transcript_base_dir()` in `registry.rs`:

| Agent | Base Directory | File Types |
|-------|----------------|------------|
| Claude Code | `~/.claude/projects/` | `.jsonl` |
| Codex | `~/.codex/sessions/` | `.jsonl` |
| Gemini | `~/.gemini/tmp/` | `.json` |

**Shell Search Implementation:**

The `find_transcript_by_shell_search()` function uses shell tools to search recursively:

1. Tries `rg` (ripgrep) first for better performance
2. Falls back to `grep -r` if `rg` is unavailable
3. Searches both `.json` and `.jsonl` files
4. Uses fixed-string matching (`-F` flag) for the normalized message fingerprint

**Message Normalization:**

Messages are normalized before searching via `normalize_message_for_matching()` to create a consistent fingerprint:
- Trim leading/trailing whitespace
- Truncate to first 120 characters (`NORMALIZED_MESSAGE_LENGTH`)

Internal whitespace is preserved so the pattern matches the message as it appears in transcript files when searched with `rg --fixed-strings` / `grep -F`.

**Fail-Closed Behavior:**

Discovery requires a `first_message` parameter. If no message is provided or no matching transcript is found, an error is returned. There is no fallback to "most recent file" behavior. The rationale is that showing no tokens is preferable to showing wrong tokens from a different session.

**Token Usage Parsing** (`transcript_discovery.rs`):

The `parse_transcript_tokens()` function extracts token usage breakdown from transcript files. Returns a `TranscriptTokenUsage` struct:

```rust
pub struct TranscriptTokenUsage {
    pub input_tokens: i64,            // Total input tokens
    pub output_tokens: i64,           // Total output tokens
    pub cached_tokens: i64,           // Cached input tokens (subset of input_tokens)
    pub last_context_tokens: Option<i64>, // Context window fill from last main-chain message
}
```

Each agent format requires different parsing:

| Agent | Format | Token Fields |
|-------|--------|--------------|
| Claude Code | JSONL | `input_tokens`, `cache_creation_input_tokens`, `cache_read_input_tokens`, `output_tokens` in `message.usage` |
| Codex | JSONL | `total_token_usage.input_tokens`, `total_token_usage.output_tokens`, `total_token_usage.cached_input_tokens` from last `token_count` event; `last_token_usage.input_tokens` as `last_context_tokens` for context window fill |
| Gemini | JSON | `input`, `output`, `thoughts`, `cached` from each message's `tokens` object |

**Codex Token Semantics:**

Codex `token_count` events contain two token usage objects with different semantics:

| Object | Meaning | Used For |
|--------|---------|----------|
| `total_token_usage` | Cumulative billing counter across ALL API calls in the session; grows unboundedly | `input_tokens`, `output_tokens`, `cached_tokens` fields (the "Tokens" footer segment) |
| `last_token_usage` | Tokens from the most recent API call only; represents actual context window fill | `last_context_tokens` field (the "Context: XK (Y%)" footer segment) |

Using `total_token_usage.input_tokens` for context window percentage would produce nonsensical results (e.g., 995K tokens for a 258K context window) because the cumulative counter sums across all turns. The `last_token_usage.input_tokens` correctly reflects how full the context window is for the current turn. When `last_token_usage` is absent (older transcript formats), `last_context_tokens` is `None` and the context percentage is not displayed.

**Claude Code Streaming Deduplication:**

Claude Code logs multiple JSONL entries per API request due to streaming (each streaming delta contains the same usage data). The parser deduplicates by tracking seen `requestId` values in a `HashSet<String>`. Entries without a `requestId` are still counted for backward compatibility with older transcript formats.

**Claude Token Field Semantics:**

| Field | Meaning | Counted As |
|-------|---------|------------|
| `input_tokens` | Non-cached input tokens sent | Added to `input_tokens` |
| `cache_creation_input_tokens` | Tokens sent and cached for future use | Added to `input_tokens` |
| `cache_read_input_tokens` | Tokens read from cache (discounted) | Reported as `cached_tokens` |
| `output_tokens` | Output tokens generated | Added to `output_tokens` |

The `TranscriptLocation` struct returned by discovery functions includes:
- `token_breakdown: Option<TranscriptTokenUsage>` - Detailed breakdown for input, output, and cached tokens

Token parsing is synchronous because `SystemInfo::collect_fresh` runs in a background thread.

The data flow is:
```
SystemInfo::collect_for_directory_with_message() (background thread)
    |
    v
discover_transcript_for_agent_with_message(cwd, agent_kind, first_message)
    |
    v
parse_transcript_tokens(path, agent_kind)
    |
    v
TranscriptLocation { ..., token_breakdown }
    |
    v
FooterProps { input_tokens, output_tokens, cached_tokens, context_tokens }
    |
    v
Footer renders "Tokens: 45K in / 78K out (32K cached)"
```
**Connection Management** (`connection/`):

The ACP connection layer uses SACP v10 (`sacp` crate) to communicate with agent subprocesses over stdin/stdout JSON-RPC. The central type is `SacpConnection` (in `connection/sacp_connection.rs`), which is `Send + Sync` and runs directly on the main tokio runtime without a dedicated worker thread.

```
┌─────────────────────────┐   SACP v10 (JSON-RPC)   ┌─────────────────────────┐
│   Main Tokio Runtime    │◄────────────────────────►│  ACP Agent Subprocess   │
│                         │   stdin/stdout           │  (spawned child process)│
│   SacpConnection        │                          │                         │
│   - spawn()             │                          │  Receives:              │
│   - create_session()    │                          │  - InitializeRequest    │
│   - load_session()      │                          │  - NewSessionRequest    │
│   - prompt()            │                          │  - PromptRequest        │
│   - cancel()            │                          │  - CancelNotification   │
│   - set_model() [unst]  │                          │                         │
└─────────────────────────┘                          └─────────────────────────┘
```

**Builder-based handler registration:** `SacpConnection::spawn()` uses `ClientToAgent::builder()` with chained `.on_receive_request()` calls to register handlers for `RequestPermissionRequest` (approval flow), `WriteTextFileRequest` (workspace-bounded file writes), and `ReadTextFileRequest` (unrestricted file reads), plus `.on_receive_notification()` for `SessionNotification`. All handlers are registered before `run_until()` is called.

**Connection initialization:** Inside `run_until()`, the connection sends `InitializeRequest` to the agent, validates the protocol version (minimum V1), and clones the `JrConnectionCx` out of the closure via a oneshot channel. The background task then awaits `futures::future::pending()` to keep the connection alive until the task is aborted on drop.

**Dynamic update channel routing:** Session notifications from the agent are routed via `Arc<Mutex<Option<Sender<SessionUpdate>>>>`. During a prompt, the sender is set to the caller's `update_tx`; between turns, it is `None` and notifications fall through to the persistent channel. This swap happens in `prompt()` and `load_session()`.

**Approval flow:** The `RequestPermissionRequest` handler translates the request to a Codex `ApprovalRequest`, sends it through the `approval_tx` channel, and spawns a concurrent task via `cx.spawn()` to wait for the user's response. The spawn avoids blocking the SACP dispatch loop while the UI collects user input.

**MCP Server Forwarding** (`connection/mcp.rs`):

CLI-configured MCP servers (from `config.toml`) are converted to SACP protocol types and passed to the agent via `NewSessionRequest.mcp_servers` at session creation time. The `to_sacp_mcp_servers()` function in `connection/mcp.rs` bridges `codex_core::config::types::McpServerConfig` to `sacp::schema::McpServer`:

| Transport | SACP Type | Key Fields |
|-----------|-----------|------------|
| `Stdio` | `McpServer::Stdio` | command, args, env (explicit key-value pairs + env vars resolved from process environment) |
| `StreamableHttp` | `McpServer::Http` | url, headers (static headers + env-resolved headers + bearer token from env var as `Authorization: Bearer` header) |

Environment variable references (`bearer_token_env_var`, `env_http_headers`, `env_vars`) are resolved eagerly from the current process environment at conversion time. Missing variables are logged as warnings and skipped -- they do not cause errors. The `client_id` and `client_secret_env_var` fields on `StreamableHttp` are not forwarded to the agent -- they are only used by the TUI/rmcp-client layer for OAuth login flows (see `@/codex-rs/rmcp-client/docs.md`). All servers are included regardless of the `enabled` flag; the agent decides how to handle them. Results are sorted by server name for deterministic ordering.

`create_session()` accepts a `mcp_servers: Vec<McpServer>` parameter that is populated by calling `to_sacp_mcp_servers()` at each session creation site:
- `spawn_and_relay.rs` -- initial session creation during backend spawn
- `session.rs` -- both the server-side `load_session` fallback and client-side replay paths during session resume
- `submit_and_ops.rs` -- fresh session creation after context compaction
- `hooks.rs` -- passes an empty vec (hook sessions do not need MCP servers)

### Transcript Persistence

The ACP module provides client-side transcript persistence that captures a full view of conversations (user input + assistant responses) without relying on agent-side storage. This enables viewing previous sessions without replaying agent mechanics.

**Storage Structure:**

Transcripts are stored at `{nori_home}/transcripts/by-project/{project-id}/sessions/{session-id}.jsonl`:

```
~/.nori/cli/
├── commands/                           # Custom prompt .md files
├── history.jsonl                       # Message history
└── transcripts/
    └── by-project/
        └── {project-id}/           # 16-hex-char hash
            ├── project.json        # Project metadata
            └── sessions/
                └── {session-id}.jsonl  # JSONL transcript file
```

**Project Identification:**

Project IDs are derived from the workspace to group sessions by project:
- Git repositories: SHA-256 hash of normalized git remote URL (SSH and HTTPS normalize to same hash)
- Non-git directories: SHA-256 hash of canonicalized path
- Hash is truncated to 16 hex characters for compact directory names

Key exports from `@/codex-rs/acp/src/transcript/project.rs`:
- `compute_project_id()`: Computes project ID for a working directory
- `ProjectId`: Contains id, name, git_remote, git_root, and cwd

**Transcript Schema (JSONL):**

Each line in the transcript file is a JSON object with:
- `ts`: ISO 8601 timestamp
- `v`: Schema version (currently 1)
- `type`: Entry type discriminator

Entry types (from `@/codex-rs/acp/src/transcript/types.rs`):

| Type | Description | Key Fields (JSON) |
|------|-------------|-------------------|
| `session_meta` | First line, session metadata | session_id, project_id, started_at, cwd, agent, cli_version, git, acp_session_id |
| `user` | User message | id, content, attachments |
| `assistant` | Complete assistant turn | id, content (blocks), agent |
| `tool_call` | Tool execution start | call_id, name, input |
| `tool_result` | Tool execution result | call_id, output, truncated, exit_code |
| `patch_apply` | File modification result | call_id, operation (edit/write/delete), path, success, error |

**Schema Field Naming:**

The `SessionMetaEntry.agent` and `AssistantEntry.agent` fields identify which ACP agent (e.g., "claude-code", "codex", "gemini") processed the session or message. The field is named `agent` rather than `model` to emphasize that it identifies the agent software, not a specific model variant.

The `SessionMetaEntry.acp_session_id` field stores the ACP agent's session ID (from `session/new` or `session/load`). This enables the `/resume` command to reconnect to the same agent session. The field is `Option<String>` with `skip_serializing_if = "Option::is_none"` and `default` for backward compatibility with transcripts created before this field existed.

**TranscriptRecorder:**

The `TranscriptRecorder` (in `@/codex-rs/acp/src/transcript/recorder.rs`) handles async, non-blocking writes:

```
┌─────────────────────────┐   mpsc channel   ┌─────────────────────────┐
│   AcpBackend            │─────────────────►│   Writer Task           │
│                         │  TranscriptCmd   │   (background)          │
│   record_user_message() │                  │                         │
│   record_assistant_msg()│                  │   - Writes to JSONL     │
│   flush() / shutdown()  │                  │   - Creates directories │
└─────────────────────────┘                  └─────────────────────────┘
```

Key methods:
- `new()`: Creates recorder, writes session_meta (including optional `acp_session_id`) and project.json
- `record_user_message()`: Records user input with optional attachments
- `record_assistant_message()`: Records complete assistant turn with content blocks
- `record_tool_call()` / `record_tool_result()`: Records tool execution
- `record_patch_apply()`: Records file modification operations (edit/write/delete)
- `flush()`: Ensures pending writes are persisted
- `shutdown()`: Flushes and terminates writer task

**TranscriptLoader:**

The `TranscriptLoader` (in `@/codex-rs/acp/src/transcript/loader.rs`) reads transcripts for viewing:

Key methods:
- `list_projects()`: List all projects with transcripts
- `list_sessions()`: List sessions for a specific project
- `find_sessions_for_cwd()`: Find sessions for current working directory
- `load_transcript()`: Load complete transcript with all entries
- `load_session_meta()`: Load just session metadata (for quick listing)

**ACP Integration:**

The `AcpBackend` automatically:
1. Creates a `TranscriptRecorder` on spawn or resume (with graceful fallback if creation fails), persisting `acp_session_id` for session resume support
2. Records user messages when `Op::UserInput` is processed
3. Accumulates assistant text during the turn and records when turn completes
4. Records normalized ACP session events via `record_client_event()` in the update and approval handlers
5. Shuts down recorder on `Op::Shutdown`

**Tool Event Recording Flow:**

Live ACP session semantics are recorded as normalized client-event entries:

```
ACP session activity         Transcript Entry
────────────────────────     ─────────────────────────
Message / reasoning deltas → client_event entry
Plan snapshot            → client_event entry
Tool snapshot            → client_event entry
Approval request         → client_event entry
Turn lifecycle           → client_event entry
```

Older `tool_call`, `tool_result`, and `patch_apply` transcript entry types remain in the schema for legacy read compatibility, but ACP live recording now uses normalized `ClientEvent` entries so transcript persistence matches the live TUI path.

Tool output for non-patch `tool_result` entries is truncated to 10,000 bytes when recording to transcript. Both this truncation and the `truncate_for_log()` helper (used for tracing previews) use `codex_utils_string::take_bytes_at_char_boundary()` to avoid slicing inside multi-byte UTF-8 characters.

Configuration:
- `AcpBackendConfig.cli_version`: CLI version included in session metadata
- `AcpBackendConfig.default_model`: Default model to apply at session start (from config.toml [default_models])
- `AcpBackendConfig.initial_context`: Optional string injected into `pending_compact_summary` at spawn time. Used by the TUI's `/fork` command to pass a plain-text conversation summary into a new ACP session, giving the agent prior context without a protocol-level session fork. When `None` (the default), `pending_compact_summary` starts empty as before. The same `pending_compact_summary` mechanism is shared by `/compact` and `/resume`.

**Re-exported Types:**

Public exports from `@/codex-rs/acp/src/transcript/mod.rs`:
- `TranscriptRecorder`, `TranscriptLoader`
- `ProjectId`, `ProjectInfo`, `SessionInfo`, `Transcript`
- Entry types: `SessionMetaEntry`, `UserEntry`, `AssistantEntry`, `ToolCallEntry`, `ToolResultEntry`, `PatchApplyEntry`
- `PatchOperationType`: Enum for patch operations (Edit, Write, Delete)
- `ContentBlock` (Text and Thinking variants), `Attachment`, `GitInfo`
- `now_iso8601()`: Utility function returning current time as ISO 8601 string

### Stderr Capture Implementation

- Buffer lines per session for access between reader task and caller
- Bounded at 500 lines with FIFO eviction when full
- Individual lines truncated to 10KB
- Reader task runs until EOF or error, logging warnings via tracing

### File-Based Tracing

The `init_rolling_file_tracing()` function in `@/codex-rs/acp/src/tracing_setup.rs` provides structured file logging:
- Sets global tracing subscriber that writes to rolling daily log files
- Log files are named `nori-acp.YYYY-MM-DD` in the configured log directory
- Filters at DEBUG level (debug builds) or WARN with INFO for codex_tui/acp (release builds)
- RUST_LOG environment variable overrides default log level
- Uses non-blocking file appender for async-safe writes
- Creates log directory automatically if it doesn't exist
- Returns error on re-initialization since global subscriber can only be set once per process
- Guard is intentionally leaked via `std::mem::forget()` to keep non-blocking writer alive for program lifetime
- ANSI colors disabled for clean file output
- Automatically initialized by the CLI (`@/codex-rs/cli/src/main.rs`) at startup, writing to `$NORI_HOME/log/nori-acp.YYYY-MM-DD`

### Core Implementation

**Subprocess Lifecycle Management:**

Multi-layer cleanup strategy for robust process termination:

1. **Process Group Isolation (Unix)**: Agent spawns in own process group via `setpgid(0, 0)`. Enables killing entire process tree with `killpg()`.
2. **Kernel-Level Parent Death Signal (Linux)**: `PR_SET_PDEATHSIG` set to `SIGTERM`. Guarantees agent receives signal if parent crashes.
3. **Process Group Kill**: On drop, `SIGKILL` is sent to the entire process group via `kill_child_process_group()`, ensuring grandchildren are terminated.
4. **Async Drop**: `SacpConnection::drop()` aborts the connection and stderr tasks, then kills the child process. No blocking wait is required because SACP v10's `ClientToAgent` is `Send + Sync` and runs as a regular tokio task.

**Environment Isolation** (`sacp_connection.rs`):

`CODEX_HOME` is explicitly stripped from the subprocess environment via `.env_remove("CODEX_HOME")` in `SacpConnection::spawn()`. Nori sets `CODEX_HOME=~/.nori/cli` in its own process so its config loader finds the right directory. Third-party ACP agents inherit the parent environment and use the upstream Codex config parser, which cannot parse Nori-specific TOML fields like `[[agents]]` -- causing a parse error on startup. Stripping `CODEX_HOME` before spawn causes those agents to fall back to their own default config paths. Custom agents defined under `[[agents]]` in Nori's config are unaffected because they communicate via the ACP protocol, not by reading Nori's config files.

**File Operation Security Boundaries** (`sacp_connection.rs`):

File operation handlers are registered as `.on_receive_request()` handlers during connection setup:

- **WriteTextFileRequest**: Writes are restricted to the workspace directory (canonicalized cwd) or `/tmp`. Path canonicalization prevents symlink-based directory traversal. Parent directories are created if needed. A synthetic `ToolCall` `SessionUpdate` is emitted for TUI rendering.
- **ReadTextFileRequest**: Reads are unrestricted -- relative paths are resolved against cwd. A synthetic `ToolCall` `SessionUpdate` is emitted for TUI rendering.

**Session Transcript Parsing** (`session_parser.rs`):

Parses token usage from agent session files:

| Agent | Path Format |
|-------|-------------|
| Codex | `~/.codex/sessions/<YEAR>/<MM>/<DD>/rollout-<ISODATE>T<HH-MM-SS>-<SESSION_GUID>.jsonl` |
| Gemini | `~/.gemini/tmp/<HASHED_PATHS>/chats/session-<ISODATE>T<HH-MM>-<SESSIONID>.json` |
| Claude | `~/.claude/projects/<PROJECT_PATH>/<SESSIONID>.jsonl` |

**Approval Bridging:**

| Policy | Behavior |
|--------|----------|
| `AskForApproval::UnlessTrusted` | Auto-approve known-safe read-only commands, prompt for all else |
| `AskForApproval::OnFailure` | Auto-approve in sandbox, prompt on failure to escalate |
| `AskForApproval::OnRequest` | (Default) Agent decides when to request approval |
| `AskForApproval::Never` | Auto-approve all requests (yolo mode) |

Dynamic policy updates via `tokio::sync::watch` channel enable `/approvals` command to take effect immediately.

`run_approval_handler()` in `backend/mod.rs` enforces a strict ordering invariant: the normalized approval event is forwarded to the TUI via `BackendEvent::Client`, and the request is pushed into `pending_approvals`, **before** the OS notification fires (`user_notifier.notify()`). This ordering is critical because `notify-rust`'s `notif.show()` blocks synchronously on some platforms (macOS), so if the notification were sent first, the TUI would never receive the approval overlay.

The `ApprovalRequest` struct (`connection/mod.rs`) carries an optional `tool_call_metadata: Option<ToolCallMetadata>` field. `ToolCallMetadata` holds the title, kind, and raw_input extracted from the ACP permission request's tool call fields. This metadata is populated in `ClientDelegate::request_permission()` (`connection/client_delegate.rs`) and consumed by the approval handler to seed the shared `pending_tool_calls` map, bridging the gap between the permission request path and the normalized tool snapshot path.

**Normalized File Mutations:**

For Edit/Write/Delete operations, the ACP backend normalizes file mutations into `nori_protocol::ClientEvent` snapshots that carry file-operation details for the TUI and transcript recorder. The same normalized snapshot drives approval prompts, live rendering, and persistence so the UI does not need to infer edits from Codex-shaped tool events.

| Operation | Approval Event | Result Event |
|-----------|----------------|--------------|
| Edit (old_string + new_string) | `ApprovalRequest` with file-operation details | Normalized file-operation snapshot |
| Write (content only) | `ApprovalRequest` with file-operation details | Normalized file-operation snapshot |
| Delete | `ApprovalRequest` with file-operation details | Normalized file-operation snapshot |
| Execute, Read, etc. | `ApprovalRequest` or auto-approval depending on policy | Normalized tool snapshot |

The transcript recorder uses the same normalized snapshot data when deciding how to persist tool activity, so the recorded transcript and live TUI stay aligned without requiring a separate patch translation path.

For Codex specifically, the normalized tool snapshot path now understands the provider's `rawInput.command` shell-wrapper arrays (for example `["/usr/bin/zsh", "-lc", "df -h ."]`) and `rawInput.parsed_cmd` objects. This means execute tools normalize to `Invocation::Command`, read tools can recover paths from `parsed_cmd[0].path`, and search/list-files tools can recover query/path semantics from Codex's parsed command metadata instead of falling back to raw JSON in the TUI.

**Tool Call Event Filtering and Title Accumulation:**

The ACP backend accumulates display metadata across multiple `ToolCall` and `ToolCallUpdate` messages so the normalized tool snapshot keeps a stable title, kind, and raw input. The ACP protocol emits multiple events for the same `call_id` in a lifecycle:

1. `ToolCall` with a generic title and empty `raw_input` (skipped, but data stored in `pending_tool_calls`)
2. `ToolCallUpdate` with the real title/kind/raw_input but not yet completed (accumulated into `pending_tool_calls`)
3. `ToolCallUpdate` with `status: Completed` but typically no title/kind/raw_input

Some agents (notably Gemini) route shell commands through the `request_permission` ACP path (`client_delegate.rs`) instead of emitting standard `ToolCall` events with populated fields. In this path, the permission request carries full metadata (title, kind, raw_input) via `ToolCallMetadata` on the `ApprovalRequest`. The approval handler in `run_approval_handler()` extracts this metadata and populates `pending_tool_calls` before processing the approval, so that when the subsequent `ToolCallUpdate(completed)` arrives with empty fields, the normalized snapshot can still resolve the actual command name. Gemini's permission request titles follow a compound format (`command [current working directory path] (description)`), which is stripped by `extract_command_from_permission_title()` in `tool_display.rs` to extract just the command portion.

The `pending_tool_calls` HashMap (keyed by `call_id`) accumulates title, kind, raw_input, and `_meta.claudeCode.toolName` from events 1 and 2 (and from permission request metadata for the Gemini path). On the completion event (step 3), the normalizer resolves the best available title using a fallback chain:

```
Title from completion update fields (if non-empty and not a raw ID)
    |
    v (fallback)
Accumulated title from intermediate updates
    |
    v (fallback)
_meta.claudeCode.toolName from the ACP adapter
    |
    v (fallback)
kind_to_display_name() mapping (e.g., Read -> "Read", Execute -> "Terminal")
    |
    v (fallback)
Hardcoded "Tool"
```

Raw Anthropic `tool_use` IDs (e.g., `toolu_015Xtg1GzAd6aPH6oiirx5us`) are detected by `title_is_raw_id()` in `tool_display.rs` and treated as empty titles, triggering the fallback chain. The `kind_to_display_name()` function maps `ToolKind` variants to human-readable strings (e.g., `Execute` -> `"Terminal"`, `SwitchMode` -> `"Switch Mode"`).

The same merged completion state is also used to decide whether a completed tool update should be treated as a file mutation or a generic tool snapshot. In practice this means a generic `ToolCall("Edit")` or a completion that arrives without earlier metadata can still resolve to a normalized file-operation snapshot if the resolved `kind`/`raw_input` identifies a file edit.

The `pending_tool_calls` state is shared via `Arc<Mutex<HashMap<String, AccumulatedToolCall>>>` across the approval handler, persistent relay, and prompt relay tasks. This sharing is necessary because the approval handler (which receives permission requests) and the relay tasks (which receive `ToolCallUpdate` completions) run as separate spawned tasks. The map is created during session setup in `spawn()` and `resume_session()`, and `Arc::clone`d into each task. Completed normalized snapshots consume the shared `pending_tool_calls` metadata used for title/raw-input resolution once the lifecycle reaches its terminal phase.

Late-arriving tool events that race past the agent's final response are handled at the TUI layer via the `turn_finished` gate (see `@/codex-rs/tui/docs.md`).

**Prompt Update Channel Lifecycle** (`sacp_connection.rs`, `user_input.rs`, `submit_and_ops.rs`):

Each prompt/load_session call gets a dedicated `mpsc` channel (`update_tx`/`update_rx`) for receiving `SessionUpdate` notifications from the ACP agent. The connection layer routes notifications through a shared `active_update_tx` slot (an `Arc<Mutex<Option<(u64, Sender)>>>`) that pairs the sender with a monotonic generation counter. The routing logic in the notification handler uses `try_send` with fallthrough: if the per-prompt channel fails (receiver dropped, or channel full), the notification falls through to the `persistent_tx` channel instead of being silently dropped.

The critical invariant is that `prompt()` does **not** clear `active_update_tx` when it returns. This is because `block_task()` (the SACP request/response mechanism) can return before all `SessionNotification` events have been delivered. Instead, callers use a `done_tx`/`done_rx` oneshot to signal the `update_handler` task:

```
prompt() returns
    |
    v
done_tx.send(())  -- signals update_handler that prompt is done
    |
    v
update_handler enters drain mode:
    tokio::select! switches from waiting on (update_rx OR done_rx)
    to waiting on update_rx with a 500ms timeout
    |
    v
After timeout or channel close, update_handler exits
    (dropping update_rx, which causes future try_send to fail)
    |
    v
Next prompt() overwrites active_update_tx slot with a fresh sender
```

The generation counter on `active_update_tx` prevents stale cleanup: `close_update_channel(generation)` only clears the slot if the generation matches, so it is safe for `load_session` (which is sequential) to clear its own channel without risking a concurrent prompt's channel. `prompt()` callers do not call `close_update_channel` at all — they rely on the done/drain pattern instead.

**Turn Interrupt Guard — Monotonic Turn Counter** (`submit_and_ops.rs`, `user_input.rs`):

When `Op::Interrupt` fires, the backend emits `TurnLifecycle::Aborted` synchronously and calls `cancel()` on the ACP connection. However, the background tokio task spawned by `handle_user_input()` (and `handle_compact()`) continues running after cancellation and may emit stale `TurnLifecycle::Completed` or `ErrorEvent` at the end of its event loop. If the user submits a new message before these stale events arrive, they race with the next turn and can prematurely terminate it.

The `turn_id: Arc<AtomicU64>` field on `AcpBackend` is a monotonic counter that eliminates this race. It is incremented on every `Op::Interrupt` and on every new turn (`handle_user_input()`, `handle_compact()`). Each spawned task captures its own turn ID at spawn time and only emits tail events (errors, warnings, `Completed`) if the counter still matches:

```
Op::Interrupt:
  1. turn_id.fetch_add(1)           -- advance the counter, invalidating the current task
  2. connection.cancel()            -- cancel the ACP session

handle_user_input() / handle_compact():
  1. my_turn_id = turn_id.fetch_add(1) + 1  -- advance counter, capture this turn's ID
  ...
  spawned task epilogue:
    if turn_id.load() == my_turn_id          -- only emit tail events if still current
      emit ErrorEvent (if error)
      emit TurnLifecycle::Completed
```

Because the counter is monotonic and never reset, there is no TOCTOU window: an interrupt always invalidates any previously spawned task, and a new turn always gets a fresh ID that cannot collide with prior tasks. The TUI does not need any complementary guard — stale events are fully suppressed at the backend layer.

**Tool Classification System:**

| ACP ToolKind | ParsedCommand | TUI Rendering |
|--------------|---------------|---------------|
| `Read` | `ParsedCommand::Read` | Exploring (compact, grouped) |
| `Search` | `ParsedCommand::Search` | Exploring (compact, grouped) |
| `Execute`, `Edit`, `Delete`, etc. | `ParsedCommand::Unknown` | Command (full display) |

**Plan Event Translation:**

ACP agents emit `SessionUpdate::Plan` events containing checklist/task entries. The ACP backend normalizes these into `nori_protocol::ClientEvent::PlanSnapshot`, enabling the TUI's existing plan rendering to display them as checkbox checklists without relying on a Codex-shaped `PlanUpdate` event.

Each `acp::PlanEntry` is mapped to a `codex_protocol::plan_tool::PlanItemArg`:

| ACP Field | Internal Field | Notes |
|-----------|---------------|-------|
| `PlanEntry.content` | `PlanItemArg.step` | Step description text |
| `PlanEntry.status` | `PlanItemArg.status` | `Pending`/`InProgress`/`Completed` mapped 1:1; unknown variants default to `Pending` |
| `PlanEntry.priority` | (dropped) | Not present in the internal `PlanItemArg` type |

The simpler `translator.rs` helper functions are unrelated to ACP session translation; they remain focused on user input conversion and other local parsing helpers.

**Conversation Compaction:**

Unlike core's direct history manipulation, ACP uses a **prompt-based approach**:
1. `/compact` sends summarization prompt to agent
2. Agent's summary response is streamed to the TUI as deltas and captured in `pending_compact_summary`
3. A new ACP session is created (the old session's context is discarded)
4. The `ContextCompactedEvent` is emitted with the summary text cloned from `pending_compact_summary`, enabling the TUI to render a visual session boundary
5. Summary is prepended to the next user message (via `SUMMARY_PREFIX` framing)

The `ContextCompactedEvent.summary` field is the coupling point between the ACP backend and the TUI's session boundary rendering. The TUI uses it to flush the streamed summary, show a "Context compacted" info message, insert a new session header, and reprint the summary as the first assistant message of the new session (see `@/codex-rs/tui/docs.md`).

**Session Resume** (`backend/mod.rs`, `connection.rs`):

`AcpBackend::resume_session()` allows reconnecting to a previous ACP session. It takes `acp_session_id: Option<&str>`, `transcript: Option<&Transcript>`, and a single `backend_event_tx`, then selects between two resume strategies based on agent capabilities. The resulting `BackendEvent` stream carries both normalized ACP session events and shared control-plane events:

```
AcpBackend::resume_session(config, acp_session_id, transcript, backend_event_tx)
    |
    v
SacpConnection::spawn() -> check capabilities().load_session
    |
    ├── Agent supports session/load AND acp_session_id is Some:
    │       |
    │       v
    │   SacpConnection::load_session(session_id, cwd, update_tx)
    │       |
    │       ├── Success:
    │       │   Agent streams SessionUpdate notifications (history replay)
    │       │   Collect task buffers updates into Vec (no backpressure)
    │       │   returns (session_id, deferred_replay_events)
    │       │
    │       └── Failure (runtime error):
    │           Collect task aborted
    │           Falls through to client-side replay (see below)
    │           WarningEvent emitted to TUI about the fallback
    │
    └── Otherwise (client-side replay fallback):
            |
            v
        SacpConnection::create_session() (normal session/new)
            |
            v
        transcript_to_summary()       -> pending_compact_summary (for agent context)
            |
            v
        returns (session_id, summary)
    |
    v
SessionConfigured event sent to TUI
    |
    v
Deferred replay relay spawned (sends buffered events to backend_event_tx)
```

**Server-side path:** A collect task runs concurrently during `load_session()`, receiving `SessionUpdate` notifications via an `mpsc` channel and buffering the normalized `ClientEvent` stream into a `Vec`. `SacpConnection::load_session()` installs the `update_tx` channel into the shared `active_update_tx` slot before sending the request, ensuring history replay notifications are captured. On `#[cfg(feature = "unstable")]` builds, model state is also extracted from the `LoadSessionResponse` if available. The buffered events are returned as `deferred_replay_events` and a relay task is spawned only *after* all setup events (`SessionConfigured`, `Warning`, etc.) have been sent to the outbound backend-event channel. This deferred-relay pattern prevents a deadlock: the outbound channel is bounded, and the TUI consumer only starts after `resume_session()` returns, so sending replay events before setup events would fill the channel and block `resume_session()` from making progress. If `load_session()` fails at runtime (e.g., the agent advertises the capability but the call itself errors), the collect task is aborted and the method falls back to a fresh session. A `WarningEvent` is emitted to inform the user that the restored session will not have server-side replay.

**Client-side path:** When the agent does not support `session/load` (e.g., Claude Code's ACP adapter returns `method_not_found`), or when the server-side `load_session()` call fails at runtime, a fresh session is created via `session/new`. The previous conversation is replayed through normalized `ClientEvent::ReplayEntry` items derived from the transcript rather than through `SessionConfigured.initial_messages`. The transcript summary path remains available for context management and `/compact`-style behavior. A `TRANSCRIPT_SUMMARY_WARN_CHARS` threshold (200K chars) logs a warning when summaries are very large; the actual safety net is the agent-side "prompt too long" rejection, which the caller handles gracefully.

A new `TranscriptRecorder` is created for the resumed session in all paths, persisting the `acp_session_id` so the session can be resumed again in the future.

**Prompt Summary** (`backend/mod.rs`):

On the first user prompt of a session, the ACP backend spawns a fire-and-forget task that generates a short summary of the prompt and emits it as a `PromptSummary` event for display in the TUI footer.

The summarization uses a completely separate ACP connection (`SacpConnection::spawn` + `create_session`) so it does not interfere with the main agent conversation. The `run_prompt_summary()` free function in `backend/mod.rs` handles this:
1. Spawns a new agent subprocess via `get_agent_config()` with the same agent name
2. Sends a "summarize in 5 words or fewer" prompt to the separate session
3. Collects the streamed text response via an `mpsc` channel and a collector task
4. If `auto_worktree.is_enabled()` (true for `Automatic` or `Ask`), renames the branch based on the summary (see Auto-Worktree Branch Renaming above) -- the directory is left unchanged
5. Emits `EventMsg::PromptSummary(PromptSummaryEvent { summary })` through the shared `event_tx`

State tracking: `AcpBackend` holds `is_first_prompt: Arc<Mutex<bool>>` which is set to `false` after the first prompt fires the summarization task. The `agent_name: String` field stores the agent name for spawning the separate connection.

The `cwd` field on `AcpBackend` is a plain `PathBuf` since the working directory does not change during a session.

Failures in the summarization task (including branch rename failures) are logged at debug/warn level and do not affect the main conversation flow.

**Undo / Ghost Snapshots** (`undo.rs`, `backend/mod.rs`):

The ACP backend supports undo via git ghost snapshots, using the `codex-git` crate (`@/codex-rs/utils/git`). The undo system supports both sequential undo (`Op::Undo`) and selective snapshot restoration via a modal picker (`Op::UndoList` / `Op::UndoTo`).

**Snapshot storage:**

`GhostSnapshotStack` is a thread-safe stack (`Mutex<Vec<SnapshotEntry>>`) stored as `Arc<GhostSnapshotStack>` on `AcpBackend`. Each `SnapshotEntry` pairs a `GhostCommit` with a `label` string (the user's prompt text at that turn). The label is captured in `handle_user_input()` when the snapshot is created.

**Snapshot lifecycle:**
1. At the **start** of each user turn (in `handle_user_input()`), before sending the prompt to the agent, a ghost commit captures the current working tree state via `codex_git::create_ghost_commit()`
2. The snapshot is pushed onto `GhostSnapshotStack` along with the user's prompt text as a label

**Undo operations:**

| Protocol Op | Handler | Behavior |
|-------------|---------|----------|
| `Op::Undo` | `handle_undo()` | Pops and restores the most recent snapshot (sequential undo) |
| `Op::UndoList` | `handle_list_snapshots()` | Returns `UndoListResult` event with all snapshots in reverse chronological order |
| `Op::UndoTo { index }` | `handle_undo_to()` | Cancels any in-progress agent turn, then restores the snapshot at the given display index and truncates all newer entries |

The display index scheme: index 0 = most recent snapshot (last element in the internal vec), index 1 = second most recent, etc. `restore_to_index()` removes the selected entry and all entries newer than it from the stack. The `list()` method returns `Vec<SnapshotInfo>` with `index`, `short_id` (first 7 chars of commit hash), and `label` fields.

The `handle_undo_to()` completion message includes a warning: "the agent is not aware that files have changed", because undo is purely a filesystem restoration that is not communicated to the ACP agent.

**Key behaviors:**
- If the cwd is not a git repository, snapshot creation is silently skipped (logged at debug level)
- If no snapshots exist when undo is requested, `UndoCompleted` reports `success: false`
- Ghost commits are unreferenced git objects (not on any branch) created by the `codex-git` crate
- `GhostSnapshotStack` is deliberately a standalone type (not embedded inside `AcpBackend`) so it can be tested independently without requiring an ACP agent connection

**ACP Error Categorization:**

| Category | Detection Patterns | User Message |
|----------|-------------------|--------------|
| `Authentication` | "auth", "-32000", "api key", "unauthorized" | "Authentication required for {provider}. {auth_hint}" |
| `QuotaExceeded` | "quota", "rate limit", "429", "usage limit" | "Rate limit or quota exceeded for {provider}" |
| `ExecutableNotFound` | "not found", "command not found" | "Could not find the {agent} CLI. Install with: npm install -g {package}" |
| `Initialization` | "initialization", "handshake", "protocol" | "Failed to initialize {provider}" |
| `PromptTooLong` | "prompt is too long" | "Prompt is too long. Try using /compact to reduce context size, or start a new session." |
| `ApiServerError` | "500", "502", "503", "504", "server error", "api_error", "overloaded" | "The API returned a server error. This is usually temporary -- please try again." |

The priority chain is: Auth > Quota > ExecutableNotFound > Initialization > PromptTooLong > ApiServerError > Unknown. Earlier categories take precedence when an error message matches multiple patterns (e.g., "500 authentication service unavailable" categorizes as `Authentication`, not `ApiServerError`).

Error categorization operates on the `Debug`-formatted (`{e:?}`) anyhow error to inspect the full error chain, while the user-facing `display_error` string uses `{e:#}` (alternate Display) in the prompt error handler to show the complete chain rather than just the outermost `.context()` wrapper. Both error message paths (spawn errors via `enhanced_error_message()` in `backend/mod.rs` and prompt errors via the match block in `user_input.rs`) handle all categories.

### Things to Know

**Module Structure Convention:**

Large modules use a directory layout (`foo/mod.rs` + submodules) instead of a single `foo.rs` file. This separates concerns and keeps individual files manageable. Modules using this pattern include `backend/` (with `session.rs`, `user_input.rs`, `hooks.rs`, `helpers.rs`, `tool_display.rs`, `transcript.rs`, `spawn_and_relay.rs`, `submit_and_ops.rs`), `connection/` (with `sacp_connection.rs`, `sacp_connection_tests.rs`), and `config/types/`. Test submodules use `tests/mod.rs` + `tests/part*.rs` for large test suites.

- Agent subprocess communication uses stdin/stdout with JSON-RPC 2.0 framing
- The minimum supported ACP protocol version is V1
- The `unstable` feature gates agent switching functionality
- Approval requests are translated to use appropriate UI (exec approval for shell commands, patch approval for file edits)
- Config loading uses Nori-specific paths (`~/.nori/cli/config.toml`) when the `nori-config` feature is enabled in the TUI
- Transcript discovery is synchronous and intended for use in background threads (e.g., the TUI's `SystemInfo` collection thread)
- Transcript discovery for all agents requires the first user message to function correctly; without it, the discovery returns an error. This is enforced via shell-based search using `rg` or `grep`.

**Event Flow Tracing:**

```bash
RUST_LOG=acp_event_flow=debug cargo run
```

The `acp_event_flow` target logs streaming deltas, tool calls, and dispatch loop event counts. Pairs with TUI-side tracing (`tui_event_flow`, `cell_flushing`).

Created and maintained by Nori.
