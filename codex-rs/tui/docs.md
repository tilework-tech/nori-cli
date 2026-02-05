# Noridoc: nori-tui

Path: @/codex-rs/tui

### Overview

The `nori-tui` crate provides the interactive terminal user interface for Nori, built with the Ratatui framework. It handles the fullscreen TUI experience including chat display, input composition, onboarding flows, and real-time streaming of model responses with markdown rendering.

### How it fits into the larger codebase

```
User Input --> nori-tui --> codex-acp (ACP backend)
                       \--> codex-core (config, auth)
                       \--> codex-protocol (types)
```

The TUI acts as the frontend layer. It:
- Uses `codex-acp` for ACP agent communication (see `@/codex-rs/acp/`)
- Uses `codex-core` for configuration loading and authentication (see `@/codex-rs/core/`)
- Displays approval requests from the ACP layer and forwards user decisions back
- Renders streaming AI responses with markdown and syntax highlighting

The `cli/` crate's `main.rs` dispatches to `nori_tui::run_main()` for interactive mode. Feature flags propagate from CLI to TUI for coordinated modular builds.

Key dependencies: `ratatui` for rendering, `crossterm` for terminal events, `pulldown-cmark` for markdown parsing, `tree-sitter-highlight` for syntax highlighting.

### Core Implementation

Entry point is `main.rs` which delegates to `run_app()` in `lib.rs`. The `run_main()` function loads `NoriConfig` once early and reuses it for both the auto-worktree setup and the `vertical_footer` setting (passed as a parameter to `run_ratatui_app()`). When `auto_worktree` is enabled in config, `run_main()` calls `codex_acp::auto_worktree::setup_auto_worktree()` and overrides the session's working directory to the new worktree path. On failure, it logs a warning and continues with the original cwd.

The main event loop in `app.rs` processes:

1. **Terminal events** (keyboard input, resize) via `tui.rs`
2. **ACP events** from the backend (streaming content, approval requests, completion)
3. **App events** for state changes (model selection, config updates)

The chat interface is managed by `chatwidget.rs`, which handles:
- User input composition with multi-line editing
- Message history display with markdown rendering
- File search integration (`file_search.rs`)
- Pager overlay for reviewing long content (`pager_overlay.rs`)

Approval requests from ACP agents are handled through `bottom_pane/approval.rs`, which displays command/patch details and collects user decisions (approve, deny, skip).

**Interrupt Queue & Tool Event Deferral** (`chatwidget/interrupts.rs`):

When the agent streams text, tool events (ExecBegin/End, McpBegin/End, PatchEnd) can arrive concurrently from the ACP backend. The `InterruptManager` queues these events via `defer_or_handle()` in `chatwidget.rs` so they do not interleave with active text output. The deferral condition is: if a `stream_controller` is active OR the queue is already non-empty, new events are pushed onto the queue to preserve FIFO ordering.

Two operations consume the queue:

| Method | Called From | Behavior |
|--------|------------|----------|
| `flush_all()` | `handle_stream_finished()` | Processes and renders all queued events. Used mid-turn when a text block completes and the next block has not started. |
| `flush_completions_and_clear()` | `on_task_complete()` | Processes completion events whose Begin was already handled, discards Begin events and any End events whose Begin was discarded. See below. |

The selective flush at task completion ensures tool cells that are already visible transition from "Running" to "Ran", while preventing new "Explored" / "Ran" cells from appearing below the agent's final message.

**Begin/End Pairing in `flush_completions_and_clear`**: Begin and End events for the same tool call are always paired in the FIFO queue (Begin precedes its End). When `flush_completions_and_clear` discards a Begin event, it records the `call_id` in a `HashSet`. When it encounters an End event, it checks whether the corresponding Begin was discarded. If so, the End is also discarded. Without this pairing, processing an End whose Begin was discarded causes `handle_exec_end_now` to create an orphan `ExecCell` with the raw `call_id` as the command name (e.g. "Ran toolu_01Lt49..."). This cascade deferral scenario arises when a tool Begin arrives while the queue is non-empty (even if the stream is no longer active), causing the Begin to be deferred and later discarded at task completion.

The Nori-specific agent picker UI lives in `nori/agent_picker.rs`, allowing users to select between available ACP agents.

**System Info Collection** (`system_info.rs`):

The `SystemInfo` struct collects environment data in a background thread to avoid blocking TUI startup:

| Field | Source |
|-------|--------|
| `git_branch` | Git repository branch name |
| `nori_profile` | Active Nori profile |
| `git_lines_added` / `git_lines_removed` | Git diff statistics |
| `is_worktree` | Whether CWD is a git worktree |
| `worktree_name` | Last path component of CWD when parent directory is `.worktrees`; used to display the immutable worktree directory identifier in the footer |
| `transcript_location` | Discovered transcript path and token usage when running within an agent environment |
| `worktree_cleanup_warning` | Warning when git worktrees exist and disk space is below 10% free (unix only) |

The `transcript_location` field includes both `token_usage` (total tokens) and `token_breakdown` (detailed input/output/cached breakdown) which are displayed in the TUI footer when Nori runs as a nested agent inside Claude Code, Codex, or Gemini.

Two collection methods are provided:
- `collect_for_directory()` - Basic collection without first-message matching (test-only)
- `collect_for_directory_with_message()` - Preferred method that passes the first user message to the transcript discovery layer for accurate Claude Code transcript identification

The first-message is obtained from `ChatWidget::first_prompt_text()`, which stores the text of the first submitted prompt. This flows through `SystemInfoRefreshRequest` to the background worker, enabling accurate transcript matching when multiple sessions exist in the same project directory.

**Worktree Cleanup Warning:**

During background system info collection on unix, `check_worktree_cleanup()` runs three checks in sequence: confirms the directory is a git repo via `git rev-parse --show-toplevel`, lists extra worktrees via `codex_git::list_worktrees()` (see `@/codex-rs/utils/git/`), and checks disk space via `df -Pk`. If worktrees exist and free disk space is below the `DISK_SPACE_LOW_PERCENT` threshold (10%), a `WorktreeCleanupWarning` is attached to the `SystemInfo` result. When the `App` event loop handles `SystemInfoRefreshed`, it checks for this warning and calls `chat_widget.add_warning_message()` to display a yellow warning cell in the chat history suggesting the user clean up unused worktrees. Non-unix platforms skip this check entirely.

**Slash Commands:**

| Command | Description |
|---------|-------------|
| `/agent` | Switch between available ACP agents |
| `/model` | Choose model and reasoning effort |
| `/approvals` | Choose what Nori can do without approval |
| `/config` | Toggle TUI settings (vertical footer, terminal notifications, OS notifications, vim mode, notify after idle, hotkeys, script timeout, loop count, footer segments) |
| `/new` | Start a new chat during a conversation |
| `/resume` | Resume a previous ACP session |
| `/init` | Create an AGENTS.md file with instructions |
| `/resume-viewonly` | View a previous session transcript (read-only) |
| `/compact` | Summarize conversation to prevent context limit |
| `/undo` | Open undo snapshot picker to select a restore point |
| `/diff` | Show git diff (including untracked files) |
| `/mention` | Mention a file |
| `/status` | Show session configuration and context window usage |
| `/first-prompt` | Show the first prompt from this session |
| `/mcp` | List configured MCP tools |
| `/login` | Log in to the current agent |
| `/logout` | Show logout instructions |
| `/switch-skillset` | Switch between available skillsets |
| `/quit` | Exit Nori |
| `/exit` | Exit Nori (alias for /quit) |

**Undo Snapshot Picker (`/undo`):**

The `/undo` slash command sends `Op::UndoList` (not `Op::Undo`) to the ACP backend. When the backend responds with `UndoListResult`, the TUI opens a `ListSelectionView` modal (the same pattern used by the approvals popup, etc.) displaying all available snapshots. Each item shows `[short_id] truncated_label` where the label is truncated to 60 characters. Selecting a snapshot dispatches `Op::UndoTo { index }` to restore to that point. If no snapshots are available, an info message is displayed instead of the modal.

Debug-only commands (not shown in help): `/rollout`, `/test-approval`

The `/logout` command is only available when the `login` feature is enabled. The `/config` command requires the `nori-config` feature.


**Status Card (`/status`) (`nori/session_header.rs`):**

The `/status` command renders a bordered card in the chat history showing session state. The card is built by `new_nori_status_output()` which creates a `CompositeHistoryCell` containing the `/status` echo and a `NoriSessionHeaderCell`.

Data flows from `ChatWidget::add_status_output()` which pulls live state from `BottomPane`:

```
ChatWidget::add_status_output()
    |-- bottom_pane.prompt_summary()              --> task summary
    |-- bottom_pane.transcript_token_breakdown()   --> token counts from transcript
    |-- bottom_pane.context_window_percent()        --> context % from live API
    |-- approval_mode_label(config)                --> approval mode from config
    v
new_nori_status_output() --> NoriSessionHeaderCell::new_with_status_info()
```

The card always shows: version, directory, agent, skillset (Nori profile). Optionally it shows:

| Section | Condition | Example |
|---------|-----------|---------|
| Task summary | `prompt_summary` present | "Task: Fix auth bug" |
| Approval mode | `approval_mode_label` present | "approvals: Agent" |
| Context line | `context_window_percent` present, with or without token data | "Context: 77.0K (27%)" or just "42%" |
| Token totals | `token_breakdown` has non-zero total | "Tokens: 123K total (32.0K cached)" |

The Tokens section renders if either `token_breakdown` has a non-zero total OR `context_window_percent` is present. This means context window percentage from the live API (`TokenUsageInfo`) can appear even before transcript token data is available.

Task summaries are truncated to 50 characters via `truncate_summary()`, which uses char-level operations (`chars().count()` / `chars().take()`) rather than byte slicing for UTF-8 safety with multi-byte characters.

**Skillset Switching (`nori/skillset_picker.rs`):**

The `/switch-skillset` command integrates with the external `nori-skillsets` CLI tool to manage skillsets:

1. Checks if `nori-skillsets` is available in PATH
2. If not available, shows a message prompting the user to install it with `npm i -g nori-skillsets`
3. If available, runs `nori-skillsets list-skillsets` to get available skillsets
4. On success (exit code 0), displays a searchable picker with skillset names
5. On selection, runs `nori-skillsets install <NAME>` to install the selected skillset
6. Shows the first line of the install output as a confirmation message

Events: `AppEvent::SkillsetListResult`, `AppEvent::InstallSkillset`, `AppEvent::SkillsetInstallResult`

**Notification Configuration:**

Three notification settings are toggled via `/config` and persisted to the `[tui]` section of `config.toml`:

- **Terminal Notifications** (`TerminalNotifications` enum from `@/codex-rs/acp/src/config/types.rs`): Controls OSC 9 escape sequences. The ACP config value flows through `codex-core`'s `Config::tui_notifications` as a `bool`, and `chatwidget.rs::notify()` gates on that bool.
- **OS Notifications** (`OsNotifications` enum from `@/codex-rs/acp/src/config/types.rs`): Controls native desktop notifications via `notify-rust`. Passed as `os_notifications` in `AcpBackendConfig` and read in `backend.rs` to set the `use_native` flag on `UserNotifier`.
- **Notify After Idle** (`NotifyAfterIdle` enum from `@/codex-rs/acp/src/config/types.rs`): Controls how long after the agent goes idle before a notification is sent. Unlike the toggle-style notification settings, this uses a sub-picker pattern (like agent picker) where selecting the config item opens a second selection view with radio-select style options (5s, 10s, 30s, 1 minute, Disabled). The selected value flows through `AcpBackendConfig` to `backend.rs` where it controls the idle timer spawn behavior.

Config changes for terminal and OS notifications emit `AppEvent::SetConfigTerminalNotifications` or `AppEvent::SetConfigOsNotifications`, handled in `app.rs` via `persist_notification_setting()`. The notify-after-idle setting uses a separate flow: `AppEvent::OpenNotifyAfterIdlePicker` opens the sub-picker, and `AppEvent::SetConfigNotifyAfterIdle` persists the chosen value via `persist_notify_after_idle_setting()`. All settings are written to the `[tui]` section of `config.toml`.

**Custom Prompt Script Execution:**

When a user invokes a `Script`-kind custom prompt (`.sh`, `.py`, `.js` files discovered from `~/.nori/cli/commands/`), the TUI follows an async execution pattern:

```
ChatComposer (Enter key)           app.rs                       codex_core::custom_prompts
       |                              |                                |
       |-- AppEvent::ExecuteScript -->|                                |
       |                              |-- execute_script(prompt, args, timeout) -->
       |                              |                                |
       |                              |<-- Ok(stdout) / Err(msg) ------|
       |                              |
       |<-- ScriptExecutionComplete --|
       |     (queued as user message) |
```

The composer intercepts Script-kind prompts in two places: when a command popup selection is confirmed, and when the user types a `/prompts:<name>` command directly and presses Enter. In both cases, positional arguments are extracted via `extract_positional_args_for_prompt_line()` and the `ExecuteScript` event is dispatched. The composer is cleared immediately.

In `app.rs`, the `ExecuteScript` handler shows an info message ("Running script..."), spawns a tokio task that calls `codex_core::custom_prompts::execute_script()` with the configured `script_timeout` from `NoriConfig`, and on completion sends `ScriptExecutionComplete`. On success, the stdout is submitted as a user message via `queue_text_as_user_message()`. On failure, an error message is displayed and the error context is also submitted as a user message so the model can see it.

The script timeout is configurable via `/config` -> "Script Timeout" which opens a sub-picker (same pattern as Notify After Idle). The sub-picker is built by `script_timeout_picker_params()` in `@/codex-rs/tui/src/nori/config_picker.rs` and uses `AppEvent::OpenScriptTimeoutPicker` / `AppEvent::SetConfigScriptTimeout` events for the two-step flow. The setting is persisted to `[tui]` in `config.toml` via `persist_script_timeout_setting()`.

**Configurable Hotkeys:**

Keyboard shortcuts are configurable through the `/config` panel ("Hotkeys" item) and persisted under `[tui.hotkeys]` in `config.toml`. The implementation is split across two layers:

- **Config layer** (`@/codex-rs/acp/src/config/types.rs`): Defines `HotkeyAction`, `HotkeyBinding`, and `HotkeyConfig` as terminal-agnostic string-based types. No crossterm dependency.
- **TUI layer** (`@/codex-rs/tui/src/nori/hotkey_match.rs`): Converts `HotkeyBinding` strings to crossterm `KeyEvent` matches via `parse_binding()` and `matches_binding()`. Also provides `key_event_to_binding()` for the reverse direction (capturing a key press as a binding string).

The `App` struct holds a `hotkey_config: HotkeyConfig` field loaded at startup. In `handle_key_event()`, configurable hotkeys are checked before the structural `match` block -- if a binding matches, the action fires and returns early. Changes are persisted via `persist_hotkey_setting()` which uses `ConfigEditsBuilder` to write to `[tui.hotkeys]` and updates the in-memory `HotkeyConfig` for immediate effect.

Hotkey actions fall into two categories that are consumed at different layers:

| Category | Actions | Consumed By |
|----------|---------|-------------|
| App-level | OpenTranscript, OpenEditor | `app.rs::handle_key_event()` |
| Editing | MoveBackwardChar, MoveForwardChar, MoveBeginningOfLine, MoveEndOfLine, MoveBackwardWord, MoveForwardWord, DeleteBackwardChar, DeleteForwardChar, DeleteBackwardWord, KillToEndOfLine, KillToBeginningOfLine, Yank | `textarea.rs::input()` |

Editing hotkeys are propagated from `App` down to the textarea via a `set_hotkey_config()` chain: App -> ChatWidget -> BottomPane -> ChatComposer -> TextArea. This propagation occurs at startup, after config changes via `persist_hotkey_setting()`, and when new sessions or agent switches create fresh ChatWidgets.

The textarea's `input()` method processes key events in three priority stages: (1) C0 control character fallbacks for terminals that send raw control codes without modifier flags, (2) configurable bindings checked via `matches_binding()` against the propagated `HotkeyConfig`, and (3) remaining hardcoded bindings (character insertion, Enter, arrow keys, Home/End, etc.).

The hotkey picker (`@/codex-rs/tui/src/nori/hotkey_picker.rs`) implements `BottomPaneView` directly (not `ListSelectionView`) because rebinding requires raw key capture. It uses a videogame-style rebind flow: select an action, press Enter, press the desired key. Conflicts are resolved by swapping bindings. The `r` key resets the selected action to its default.

**Vim Mode:**

The textarea supports an optional vim-style navigation mode, toggled via `/config` ("Vim Mode" item) and persisted to `config.toml` under `[tui]`:

```toml
[tui]
vim_mode = true
```

When enabled, the textarea operates in two modes:

| Mode | Behavior |
|------|----------|
| Insert | Default mode. Characters are inserted as typed. Press `Escape` to enter Normal mode. |
| Normal | Navigation and editing mode. Keys are interpreted as commands rather than character input. |

Normal mode supports standard vim keybindings:

| Category | Keys | Behavior |
|----------|------|----------|
| Navigation | `h`/`j`/`k`/`l` | Move cursor left/down/up/right |
| Navigation | `w`/`b` | Forward/backward by word (`w` lands on start of next word via `beginning_of_next_word()`) |
| Navigation | `0`/`$`/`^` | Beginning of line / end of line / first non-whitespace on line |
| Navigation | `G`/`gg` | End of text / beginning of text |
| Insert entry | `i`/`a` | Enter Insert at cursor / after cursor |
| Insert entry | `I`/`A` | Enter Insert at beginning of line / end of line |
| Insert entry | `o`/`O` | Open new line below/above and enter Insert |
| Editing | `x` | Delete character under cursor |
| Editing | `D`/`C` | Delete to end of line (`C` also enters Insert mode) |
| Editing | `dd` | Delete current line |
| Editing | `p` | Paste from kill buffer |

Two-key sequences (`gg`, `dd`) use a `vim_pending_key: Option<char>` field on TextArea. Pressing `g` or `d` sets the pending key; the second keypress either completes the sequence or cancels it (non-matching keys are discarded).

The state machine is implemented in `textarea.rs` via the `VimModeState` enum. Vim mode handling runs as "stage 0" in the `input()` method, before C0 control fallbacks, configurable hotkey bindings, and hardcoded bindings. When in Normal mode, `chat_composer.rs` bypasses paste burst detection and sends input directly to the textarea so navigation keys work without interference.

Config changes emit `AppEvent::SetConfigVimMode`, handled in `app.rs` via `persist_vim_mode_setting()`. The setting propagates down the same chain as hotkeys: App -> ChatWidget -> BottomPane -> ChatComposer -> TextArea via `set_vim_mode_enabled()`. When vim mode is disabled, the state resets to Insert mode.

**Status Line Footer:**

The footer displays configurable segments, each of which can be enabled/disabled via `/config` -> "Footer Segments" or via `[tui.footer_segments]` in config.toml:

| Segment | TOML Key | Description |
|---------|----------|-------------|
| Task Summary | `prompt_summary` | "Task: <summary>" (dim) - generated by ACP backend on first user prompt |
| Vim Mode | `vim_mode` | "NORMAL" (blue/bold) or "INSERT" (green) when vim mode is enabled |
| Git Branch | `git_branch` | Current branch name with ⎇ symbol (yellow for main repo, orange for worktree) |
| Worktree Name | *(always shown)* | "Worktree: {name}" (light red) when running in an auto-worktree session -- the immutable directory name, distinct from the git branch which gets renamed after the first prompt |
| Git Stats | `git_stats` | Lines added/removed in current session |
| Context Window | `context` | "Context: 34K (27%)" when running within an agent environment |
| Approval Mode | `approval_mode` | "Approvals: Agent/Full Access/Read Only" |
| Nori Profile | `nori_profile` | "Skillset: <name>" |
| Nori Version | `nori_version` | "Skillsets v<version>" |
| Token Usage | `token_usage` | "Tokens: 123K total (32K cached)" when running within an agent environment |

Example config.toml to disable specific segments:
```toml
[tui.footer_segments]
token_usage = false
git_stats = false
```

All segments are enabled by default. The order of segments in the footer is fixed (cannot be reordered via config).

Token data flows from `TranscriptLocation.token_breakdown` (provided by `codex_acp::discover_transcript_for_agent_with_message()`) through `FooterProps` to the footer renderer. The breakdown includes separate input, output, and cached token counts for accurate usage reporting.

The prompt summary flows from the ACP backend as an `EventMsg::PromptSummary` event, handled by `ChatWidget::on_prompt_summary()`, which propagates it down: `ChatWidget` -> `BottomPane::set_prompt_summary()` -> `ChatComposer::set_prompt_summary()` -> `FooterProps.prompt_summary` -> `footer_segments()` renderer.

The TUI detects the repo root for auto-worktree branch renaming by inspecting the cwd path structure: when `auto_worktree` is enabled and the cwd's parent directory is named `.worktrees`, the grandparent is treated as the repo root. This value is passed as `auto_worktree_repo_root` in `AcpBackendConfig` (see `chatwidget/agent.rs`). The branch rename is fire-and-forget; the working directory does not change during a session, so the TUI does not need to handle directory changes.

**External Editor Integration (`editor.rs`):**

The external editor hotkey (default Ctrl-G, configurable via hotkeys) opens the user's preferred text editor for composing prompts. The editor is resolved from `$VISUAL` > `$EDITOR` > platform default (`vi` on Unix, `notepad` on Windows). The lifecycle in `app.rs::open_external_editor()`:

1. Reads current composer text via `ChatWidget::composer_text()`
2. Writes content to a temp file (`nori-editor-*.md`)
3. Suspends the TUI via `tui::restore()`
4. Spawns the editor synchronously (blocking) via shell delegation (`sh -c` on Unix, `cmd /C` on Windows)
5. Re-enables the TUI via `tui::set_modes()`
6. On success, reads the temp file content back into the composer; on failure or non-zero exit, discards changes

This uses the same terminal suspend/resume pattern as job control in `lib.rs` (SIGTSTP handling).

**View-Only Transcript Viewing:**
The `/resume-viewonly` command allows viewing previous session transcripts without replaying the conversation. Implementation in `@/codex-rs/tui/src/`:

- `viewonly_transcript.rs`: Converts `codex_acp::transcript::Transcript` entries to `ViewonlyEntry` enum (User, Assistant, Thinking, Info variants)
- `nori/viewonly_session_picker.rs`: Session picker UI for selecting past sessions
- `app.rs::display_viewonly_transcript()`: Renders entries in the chat history

Rendering behavior:
- User messages display via `UserHistoryCell` with standard user styling
- Assistant messages render via `AgentMessageCell` with `append_markdown()` for syntax highlighting
- Thinking blocks display with dimmed styling (matching live reasoning display)
- Tool calls, tool results, and patch operations are skipped to focus on conversation content
- Blank line separators between entries improve readability

The async flow uses three AppEvents: `ShowViewonlySessionPicker` -> `LoadViewonlyTranscript` -> `DisplayViewonlyTranscript`.

**Session Resume (`/resume`):**

The `/resume` command allows reconnecting to a previous ACP session. It uses the ACP agent's `session/load` RPC when available, and falls back to client-side replay when the agent does not support it (see `@/codex-rs/acp/docs.md` for the dual-path architecture).

The flow involves three layers:

```
SlashCommand::Resume
    |
    v
ChatWidget::open_resume_session_picker()
    |  (async: loads sessions via TranscriptLoader, filters by agent)
    v
AppEvent::ShowResumeSessionPicker -> resume_session_picker modal
    |  (user selects session)
    v
AppEvent::ResumeSession { nori_home, project_id, session_id }
    |  (loads full Transcript, extracts acp_session_id as Option<String>)
    v
App::shutdown_current_conversation()
    |
    v
ChatWidget::new_resumed_acp(init, acp_session_id, transcript)
    |
    v
spawn_acp_agent_resume() -> AcpBackend::resume_session()
```

The `ResumeSession` handler loads the full transcript (not just metadata) via `TranscriptLoader::load_transcript()`. The `acp_session_id` is extracted as `Option<String>` from `transcript.meta.acp_session_id` -- sessions without an `acp_session_id` are still resumable via the client-side replay fallback.

Session filtering: `load_resumable_sessions()` in `@/codex-rs/tui/src/nori/resume_session_picker.rs` loads all sessions for the current working directory via the viewonly session picker's `load_sessions_with_preview()`, then filters to only sessions whose `agent` field matches the currently active agent.

The resume session picker reuses the `SessionPickerInfo` type and `format_relative_time()` utility from `@/codex-rs/tui/src/nori/viewonly_session_picker.rs`. The `format_relative_time` function was made `pub(crate)` for this reuse.

`spawn_acp_agent_resume()` in `@/codex-rs/tui/src/chatwidget/agent.rs` mirrors `spawn_acp_agent()` but calls `AcpBackend::resume_session()` instead of `AcpBackend::spawn()`, passing both the optional `acp_session_id` and the full `Transcript`. The spawned task structure (op forwarding, event forwarding, model command handling) is identical.

**Loop Mode (Prompt Repetition):**

Loop mode allows the same first prompt to be re-run multiple times, each time in a completely fresh conversation session. This is configured via `/config` -> "Loop Count" or by setting `loop_count` in `config.toml` (see `@/codex-rs/acp/src/config/types.rs`).

The loop is orchestrated entirely within the TUI layer -- `codex-core` has no awareness of loop semantics:

```
User submits first prompt
       |
       v
ChatWidget::submit_user_message()
  - Reads NoriConfig::loop_count
  - If count > 1: sets loop_remaining = count-1, loop_total = count
       |
       v
Agent completes task -> on_task_complete()
  - If loop_remaining > 0: emits AppEvent::LoopIteration
       |
       v
App::handle_event(LoopIteration)
  - Shuts down current conversation
  - Creates a fresh ChatWidget with the same prompt
  - Calls set_loop_state() on the new widget
  - Displays "Loop iteration N of M" info message
       |
       v
(repeat until remaining == 0)
```

State fields on `ChatWidget`: `loop_remaining: Option<i32>` and `loop_total: Option<i32>`. These are initialized on the first `submit_user_message()` call and carried forward across iterations via `App`-level event handling.

The loop is cancelled (both fields set to `None`) when an error occurs (`on_error()`) or the user interrupts (`on_interrupted_turn()`). The `/config` sub-picker is built by `loop_count_picker_params()` in `@/codex-rs/tui/src/nori/config_picker.rs` with preset options: Disabled, 2, 3, 5, 10. The setting persists to `[tui]` in `config.toml` via `persist_loop_count_setting()`.

### Things to Know

**Cargo Feature Flags:**

| Feature | Dependencies | Default | Purpose |
|---------|--------------|---------|---------|
| `unstable` | `codex-acp/unstable` | Yes | Unstable ACP features like model switching |
| `nori-config` | - | Yes | Use Nori's simplified ACP-only config |
| `login` | `codex-login`, `codex-utils-pty` | Yes | ChatGPT/API login functionality |
| `otel` | `opentelemetry-appender-tracing` | No | OpenTelemetry tracing export |
| `vt100-tests` | - | No | vt100-based emulator tests |
| `debug-logs` | - | No | Verbose debug logging |

**--yolo Flag:**

The `--dangerously-bypass-approvals-and-sandbox` flag (alias: `--yolo`) works in all builds. When enabled, it overrides any configured sandbox or approval policies to auto-approve all tool operations without prompting.

**Update Checking:**

The TUI uses Nori-specific update checking via files in `@/codex-rs/tui/src/nori/`:
- `update_action.rs`: Update action handling
- `updates.rs`: Version checking against GitHub releases
- `update_prompt.rs`: User prompting for updates

**Error Reporting:**

When errors occur, users are directed to report bugs at `https://github.com/tilework-tech/nori-cli/issues`.

- Snapshot testing via `insta` is used extensively - see `snapshots/` directory
- Markdown rendering uses `pulldown-cmark` for parsing with `tree-sitter-highlight` for syntax highlighting
- Clipboard integration provided via `arboard` crate (disabled on Android/Termux)
- Terminal state is restored on exit or crash via the `tui.rs` module using `color-eyre` for panic handling. The `tui::restore()` / `tui::set_modes()` pair is also used for temporary terminal suspension (job control signals, external editor spawning).
- The `chatwidget.rs` file is large (~165K) and contains most of the chat rendering logic
- The `first_prompt_text` field in `ChatWidget` is set when the user submits their first message and is used for both transcript matching in Claude Code sessions and as the prompt text replayed during loop mode iterations

Created and maintained by Nori.
