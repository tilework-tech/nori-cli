# Noridoc: nori-tui

Path: @/codex-rs/tui

### Overview

The `nori-tui` crate provides the interactive terminal user interface for Nori, built with the Ratatui framework. It handles the fullscreen TUI experience including chat display, input composition, onboarding flows, and real-time streaming of agent responses with markdown rendering.

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

Entry point is `main.rs` which delegates to `run_app()` in `lib.rs`. The `run_main()` function loads `NoriConfig` once early and reuses it for both the auto-worktree setup and the `vertical_footer` setting (passed as a parameter to `run_ratatui_app()`). After loading config, `run_main()` initializes the agent registry via `codex_acp::initialize_registry()` with any custom `[[agents]]` defined in `config.toml` (see `@/codex-rs/acp/docs.md` for registry details). Initialization failure is non-fatal (logged as a warning).

The auto-worktree startup flow branches on the `AutoWorktree` enum (see `@/codex-rs/acp/docs.md`):

| Variant | Timing | Behavior |
|---------|--------|----------|
| `Automatic` | Before TUI init, in `run_main()` | Calls `setup_auto_worktree()` immediately and overrides cwd |
| `Ask` | After TUI init, in `run_ratatui_app()` | Sets `pending_worktree_ask = true`, deferred to a TUI popup shown after onboarding but before `App::run()` |
| `Off` | N/A | Skips worktree creation entirely |

The `Ask` popup is implemented by `nori::worktree_ask::run_worktree_ask_popup()`, a standalone mini-app screen (same pattern as `update_prompt.rs`) that runs its own event loop before the main `App`. It presents two options ("Yes, create a worktree" / "No, continue without a worktree") and returns a boolean. If the user confirms, `setup_auto_worktree()` is called and config is reloaded with the new cwd via `load_config_or_exit()`. Ctrl-C, Escape, and the "No" option all skip worktree creation. On failure, the TUI continues with the original cwd.

The main event loop in `app/mod.rs` processes:

1. **Terminal events** (keyboard input, resize) via `tui.rs`
2. **ACP events** from the backend (streaming content, approval requests, completion)
3. **App events** for state changes (agent selection, config updates)

The chat interface is managed by the `chatwidget/` module (`chatwidget/mod.rs` + submodules), which handles:
- User input composition with multi-line editing
- Message history display with markdown rendering
- File search integration (`file_search.rs`)
- Pager overlay for reviewing long content (`pager_overlay.rs`)

Approval requests from ACP agents are handled through `bottom_pane/approval.rs`, which displays command/patch details and collects user decisions (approve, deny, skip).

**Interrupt Queue & Tool Event Deferral** (`chatwidget/event_handlers.rs`):

When the agent streams text, tool events (ExecBegin/End, McpBegin/End, PatchEnd) can arrive concurrently from the ACP backend. Tool event handlers call `flush_answer_stream_with_separator()` before `defer_or_handle()` to finalize any in-progress text stream, ensuring tool cells appear in their correct interleaved position relative to text rather than being grouped after all text. The `InterruptManager` queues events via `defer_or_handle()` when the queue is already non-empty, preserving FIFO ordering for events that arrive while earlier deferred events are pending.

One operation consumes the queue:

| Method | Called From | Behavior |
|--------|------------|----------|
| `flush_completions_and_clear()` | `on_agent_message()`, `on_task_complete()` | Processes completion events whose Begin was already handled, discards Begin events and any End events whose Begin was discarded. See below. |

The selective flush ensures tool cells that are already visible transition from "Running" to "Ran", while preventing new "Explored" / "Ran" cells from appearing below the agent's final message.

**Begin/End Pairing in `flush_completions_and_clear`**: Begin and End events for the same tool call are always paired in the FIFO queue (Begin precedes its End). When `flush_completions_and_clear` discards a Begin event, it records the `call_id` in a `HashSet`. When it encounters an End event, it checks whether the corresponding Begin was discarded. If so, the End is also discarded. Without this pairing, processing an End whose Begin was discarded causes `handle_exec_end_now` to create an orphan `ExecCell` with the raw `call_id` as the command name (e.g. "Ran toolu_01Lt49..."). This cascade deferral scenario arises when a tool Begin arrives while the queue is non-empty (even if the stream is no longer active), causing the Begin to be deferred and later discarded at task completion.


**Turn-Finished Gate** (`chatwidget/event_handlers.rs`):

The ACP protocol has no end-of-turn synchronization guarantee -- `PromptResponse` and `SessionNotification` messages are independent async streams that race. This means tool call events (`ExecCommandBegin/End`, `McpToolCallBegin/End`) can arrive after the agent's final response text (`AgentMessage`). The `turn_finished: bool` field on `ChatWidget` acts as a gate to silently discard these late-arriving events:

| Transition | Trigger | Effect |
|------------|---------|--------|
| `turn_finished = true` | `on_agent_message()` | Closes the gate -- subsequent tool events are discarded |
| `turn_finished = false` | `on_task_started()` | Opens the gate -- new turn begins accepting tool events |

The gate is checked at the entry point of `on_exec_command_begin()`, `on_exec_command_end()`, `on_mcp_tool_call_begin()`, and `on_mcp_tool_call_end()`. When `turn_finished` is true, these methods return immediately without rendering any UI. This is complementary to the interrupt queue -- the queue handles deferral during streaming within a turn, while `turn_finished` handles events that arrive after the turn ends entirely.

The Nori-specific agent picker UI lives in `nori/agent_picker.rs`, allowing users to select between available ACP agents.

**System Info Collection** (`system_info.rs`):

The `SystemInfo` struct collects environment data in a background thread to avoid blocking TUI startup:

| Field | Source |
|-------|--------|
| `git_branch` | Git repository branch name |
| `nori_profile` | Active Nori profile from `.nori-config.json` (reads `activeSkillset` first, then `agents.claude-code.profile.baseProfile`, then `profile.baseProfile`) |
| `git_lines_added` / `git_lines_removed` | Git diff statistics |
| `is_worktree` | Whether CWD is a git worktree |
| `worktree_name` | Last path component of CWD when parent directory is `.worktrees`; used to display the immutable worktree directory identifier in the footer |
| `transcript_location` | Discovered transcript path and token usage when running within an agent environment |
| `worktree_cleanup_warning` | Warning when git worktrees exist and disk space is below 10% free (unix only) |

The `transcript_location` field includes both `token_usage` (total tokens) and `token_breakdown` (detailed input/output/cached breakdown) which are displayed in the TUI footer when Nori runs as a nested agent inside Claude Code, Codex, or Gemini.

Two collection methods are provided:
- `collect_for_directory()` - Basic collection without first-message matching (test-only)
- `collect_for_directory_with_message()` - Preferred method that passes the first user message to the transcript discovery layer for accurate transcript identification across all agents

The first-message is obtained from `ChatWidget::first_prompt_text()`, which stores the text of the first submitted prompt. This flows through `SystemInfoRefreshRequest` to the background worker, enabling accurate transcript matching when multiple sessions exist in the same project directory.

**Worktree Cleanup Warning:**

During background system info collection on unix, `check_worktree_cleanup()` runs three checks in sequence: confirms the directory is a git repo via `git rev-parse --show-toplevel`, lists extra worktrees via `codex_git::list_worktrees()` (see `@/codex-rs/utils/git/`), and checks disk space via `df -Pk`. If worktrees exist and free disk space is below the `DISK_SPACE_LOW_PERCENT` threshold (10%), a `WorktreeCleanupWarning` is attached to the `SystemInfo` result. When the `App` event loop handles `SystemInfoRefreshed`, it checks for this warning and calls `chat_widget.add_warning_message()` to display a yellow warning cell in the chat history suggesting the user clean up unused worktrees. Non-unix platforms skip this check entirely.

**Slash Commands:**

| Command | Description |
|---------|-------------|
| `/agent` | Switch between available ACP agents (dynamically shows current agent name) |
| `/model` | Choose model (dynamically shows current agent/model name) |
| `/approvals` | Choose what Nori can do without approval (dynamically shows current approval mode) |
| `/config` | Toggle TUI settings (vertical footer, terminal notifications, OS notifications, vim mode, auto worktree, per session skillsets, notify after idle, hotkeys, script timeout, loop count, footer segments) |
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
| `/fork` | Rewind conversation to a previous message |
| `/quit` | Exit Nori |
| `/exit` | Exit Nori (alias for /quit) |

**Slash Command Description Overrides:**

`/agent`, `/model`, and `/approvals` show the current runtime value in parentheses in the slash command popup (e.g., `(current: Mock ACP)`). This is implemented via a `command_description_overrides: HashMap<SlashCommand, String>` that flows through `BottomPane` -> `ChatComposer` -> `CommandPopup`. `BottomPane::set_agent_display_name()` sets overrides for both `/agent` and `/model`; `BottomPane::set_approval_mode_label()` sets the override for `/approvals`. The agent override is populated at startup in `BottomPane::new()` and updated on agent switches. The approval override is set whenever the approval mode changes.

**Selection Popup Row Layout (`bottom_pane/selection_popup_common.rs`):**

`render_rows()` and `measure_rows_height()` are the shared rendering functions used by all selection popups (`ListSelectionView`, `CommandPopup`, `FileSearchPopup`). Each popup item has an optional description that appears alongside the item name. The layout engine chooses between two modes per-row via `wrap_row()`:

| Mode | Condition | Layout |
|------|-----------|--------|
| Side-by-side | `total_width - desc_col >= MIN_DESC_COLUMNS` (12) | Description starts at `desc_col` on the same line as the name, wrapped lines indented to `desc_col` |
| Stacked | `total_width - desc_col < MIN_DESC_COLUMNS` | Name on its own line(s), description on separate line(s) below with 4-space indent |

The `desc_col` is computed once per render pass from the widest visible name plus 2 columns of padding. The stacked fallback prevents descriptions from being squeezed into 1-2 characters of horizontal space on narrow terminals. Because both `render_rows()` and `measure_rows_height()` call the same `wrap_row()` function, layout and height calculation are always consistent.

`SelectionViewParams` supports an optional `on_dismiss: Option<SelectionAction>` callback that fires when the picker is dismissed without selection (Escape or Ctrl-C). The callback is invoked in `ListSelectionView::on_ctrl_c()` before marking the view as complete. It does not fire when the user makes a selection via `accept()`. This is used by the skillset picker to send `SkillsetPickerDismissed` when the deferred agent spawn needs a fallback trigger.

**Undo Snapshot Picker (`/undo`):**

The `/undo` slash command sends `Op::UndoList` (not `Op::Undo`) to the ACP backend. When the backend responds with `UndoListResult`, the TUI opens a `ListSelectionView` modal (the same pattern used by the approvals popup, etc.) displaying all available snapshots. Each item shows `[short_id] truncated_label` where the label is truncated to 60 characters. Selecting a snapshot dispatches `Op::UndoTo { index }` to restore to that point. If no snapshots are available, an info message is displayed instead of the modal.

**Compact Session Boundary (`/compact`):**

When the ACP backend sends a `ContextCompactedEvent` with a summary, `on_context_compacted()` renders a visual session boundary to show that a new session has begun. The sequence is:

1. Flush the in-progress streamed summary (old session content)
2. Show "Context compacted" as an info message
3. Insert a `NoriSessionHeaderCell` (the "Nori CLI" card, same as starting a fresh session) by constructing a `SessionConfiguredEvent` from the current widget config state
4. Reprint the summary text as the first assistant message of the new session (temporarily clears `turn_finished` to allow streaming)

When the event has no summary (core backend path), only the "Context compacted" info message is shown. This asymmetry exists because the core backend compacts history in-place without producing a summary for the TUI.

**Fork Conversation (`/fork`) (`nori/fork_picker.rs`, `app_backtrack.rs`):**

The `/fork` slash command lets users rewind to a previous user message and branch the conversation from that point. It is only available when no task is running (`available_during_task = false`). The flow:

1. `SlashCommand::Fork` dispatches `AppEvent::OpenForkPicker`
2. The handler calls `collect_user_messages()` in `app_backtrack.rs` to gather all user messages from the current session segment (messages after the last `SessionInfoCell`). If none exist, an info message is shown instead of the picker.
3. `fork_picker_params()` in `nori/fork_picker.rs` builds a `SelectionViewParams` with items displayed newest-first (reversed from chronological order). Message previews are truncated to 80 characters; multiline messages show only the first line with an ellipsis.
4. Selecting a message fires `AppEvent::ForkToMessage { nth_user_message, prefill }`
5. The `ForkToMessage` handler:
   - Calls `build_fork_summary()` to create a plain-text summary of the conversation up to (but not including) the selected message, formatted as `User: ...\nAssistant: ...\n` pairs
   - Shuts down the current conversation
   - Creates a new `ChatWidget` with `fork_context` set to the summary string
   - Trims `transcript_cells` to the fork point via `trim_transcript_cells_to_nth_user()` so the TUI preserves visual history before the fork
   - Prefills the composer with the selected message text

The fork context flows through `ChatWidgetInit.fork_context` -> `spawn_agent()` -> `spawn_acp_agent()` -> `AcpBackendConfig.initial_context`, which initializes the ACP backend's `pending_compact_summary`. This reuses the same mechanism as `/compact` and `/resume` -- the summary is prepended to the first user prompt in the new session, giving the agent prior conversation context without a protocol-level session fork.

Debug-only commands (not shown in help): `/rollout`, `/test-approval`

The `/logout` command is only available when the `login` feature is enabled. The `/config` command requires the `nori-config` feature.


**Status Card (`/status`) (`nori/session_header/mod.rs`):**

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
3. If available, runs `nori-skillsets list` to get available skillsets
4. On success (exit code 0), displays a picker with skillset names. Each `SelectionItem` sets `search_value` to the skillset name so the picker's search filtering can match against it. When `skillset_per_session` is enabled, a "No Skillset" option is prepended to the list; selecting it sends `AppEvent::SkillsetPickerDismissed` (same as Escape/Ctrl-C dismiss), giving users an explicit way to skip skillset selection.
5. On selection, if an `install_dir` is set (worktree context), runs `nori-skillsets --non-interactive switch <NAME> --install-dir <path>`; otherwise runs `nori-skillsets --non-interactive install <NAME>`. The `--non-interactive` flag is required because the TUI captures stdout/stderr via `.output()` and provides no stdin, so any interactive prompt would hang indefinitely.
6. Shows the install output as a confirmation message (for long output, extracts the last section after double newlines)
7. On successful switch/install, updates `ChatWidget.session_skillset_name` which flows to the footer

The worktree context is detected by `handle_switch_skillset_command()`: if the cwd's parent directory is named `.worktrees`, the cwd is passed as `install_dir`. When `skillset_per_session` is enabled, the cwd is used as `install_dir` even when not in a worktree. This enables per-worktree or per-session skillset installation.

When `skillset_per_session` is enabled in `NoriConfig`, the skillset picker is automatically triggered at startup in `App::run()`, regardless of whether the session is in a worktree. The agent spawn is deferred (`ChatWidgetInit::deferred_spawn = true`) so that `nori-skillsets switch` can write `.claude/CLAUDE.md` to disk before the agent reads it. During the deferred period, a dummy channel is created in `constructors.rs` so the widget has a valid `op_tx`. The real agent spawns after the user picks a skillset (`SkillsetSwitchResult` triggers `spawn_deferred_agent()`). If the user dismisses the picker without selecting a skillset (Escape/Ctrl-C or choosing the "No Skillset" option), the `AppEvent::SkillsetPickerDismissed` event triggers `spawn_deferred_agent()` -- the agent starts without a skillset, behaving as if the feature were disabled. The `server_for_deferred_spawn` field on `App` holds the `ConversationManager` until one of these paths consumes it via `.take()`.

When `skillset_per_session` is on and `auto_worktree` is `Off`, the picker subtitle changes from "Switching skillset in {dir}" to "Warning: skillset files will be added to {dir}" to warn that skillset files will be written directly to the current working directory (no worktree isolation). The `on_skillset_list_result()` method in `pickers.rs` loads `NoriConfig` to determine both the `show_no_skillset` flag (true when `skillset_per_session` is enabled) and the `auto_worktree_off` flag (true when per-session is on and `auto_worktree` is not enabled).

Events: `AppEvent::SkillsetListResult` (carries `install_dir: Option<PathBuf>`), `AppEvent::InstallSkillset`, `AppEvent::SwitchSkillset`, `AppEvent::SkillsetInstallResult`, `AppEvent::SkillsetSwitchResult`, `AppEvent::SkillsetPickerDismissed`, `AppEvent::OpenSkillsetPerSessionWorktreeChoice`

The "Per Session Skillsets" toggle in `/config` is built in `nori/config_picker.rs`. Toggling it on emits `AppEvent::OpenSkillsetPerSessionWorktreeChoice`, which opens a worktree choice modal (`skillset_worktree_choice_params()`) letting the user choose between "With Auto Worktrees" (sets `auto_worktree` to `Automatic`) and "Without Auto Worktrees". Toggling it off emits `AppEvent::SetConfigSkillsetPerSession`, handled in `app/config_persistence.rs` via `persist_skillset_per_session_setting()` to write `skillset_per_session` under `[tui]` in `config.toml`.

The "Auto Worktree" item in `/config` uses a sub-picker pattern (matching Notify After Idle / Script Timeout): selecting the config item emits `AppEvent::OpenAutoWorktreePicker`, which opens a second selection view listing all `AutoWorktree` variants (`Automatic`, `Ask`, `Off`) with radio-select style (current variant marked). The config item's display name shows the current mode in parentheses (e.g. "Auto Worktree (automatic)"). Selecting a variant emits `AppEvent::SetConfigAutoWorktree(variant)`, persisted via `persist_auto_worktree_setting()` which writes the string value (e.g. `"automatic"`, `"ask"`, `"off"`) to `[tui]` in `config.toml`.

The `session_skillset_name` field propagates through the widget hierarchy: `ChatWidget` -> `BottomPane` -> `ChatComposer` -> `Footer`. In the footer, `session_skillset_name` takes priority over `nori_profile` from `SystemInfo` for the skillset display segment.


**Notification Configuration:**

Three notification settings are toggled via `/config` and persisted to the `[tui]` section of `config.toml`:

- **Terminal Notifications** (`TerminalNotifications` enum from `@/codex-rs/acp/src/config/types/mod.rs`): Controls OSC 9 escape sequences. The ACP config value flows through `codex-core`'s `Config::tui_notifications` as a `bool`, and `chatwidget/user_input.rs::notify()` gates on that bool.
- **OS Notifications** (`OsNotifications` enum from `@/codex-rs/acp/src/config/types/mod.rs`): Controls native desktop notifications via `notify-rust`. Passed as `os_notifications` in `AcpBackendConfig` and read in `backend/mod.rs` to set the `use_native` flag on `UserNotifier`.
- **Notify After Idle** (`NotifyAfterIdle` enum from `@/codex-rs/acp/src/config/types/mod.rs`): Controls how long after the agent goes idle before a notification is sent. Unlike the toggle-style notification settings, this uses a sub-picker pattern (like agent picker) where selecting the config item opens a second selection view with radio-select style options (5s, 10s, 30s, 1 minute, Disabled). The selected value flows through `AcpBackendConfig` to `backend.rs` where it controls the idle timer spawn behavior.

Config changes for terminal and OS notifications emit `AppEvent::SetConfigTerminalNotifications` or `AppEvent::SetConfigOsNotifications`, handled in `app/config_persistence.rs` via `persist_notification_setting()`. The notify-after-idle setting uses a separate flow: `AppEvent::OpenNotifyAfterIdlePicker` opens the sub-picker, and `AppEvent::SetConfigNotifyAfterIdle` persists the chosen value via `persist_notify_after_idle_setting()`. All settings are written to the `[tui]` section of `config.toml`.

**Custom Prompt Script Execution:**

When a user invokes a `Script`-kind custom prompt (`.sh`, `.py`, `.js` files discovered from `~/.nori/cli/commands/`), the TUI follows an async execution pattern:

```
ChatComposer (Enter key)           app/mod.rs                       codex_core::custom_prompts
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

In `app/event_handling.rs`, the `ExecuteScript` handler shows an info message ("Running script..."), spawns a tokio task that calls `codex_core::custom_prompts::execute_script()` with the configured `script_timeout` from `NoriConfig`, and on completion sends `ScriptExecutionComplete`. On success, the stdout is submitted as a user message via `queue_text_as_user_message()`. On failure, an error message is displayed and the error context is also submitted as a user message so the agent can see it.

The script timeout is configurable via `/config` -> "Script Timeout" which opens a sub-picker (same pattern as Notify After Idle). The sub-picker is built by `script_timeout_picker_params()` in `@/codex-rs/tui/src/nori/config_picker.rs` and uses `AppEvent::OpenScriptTimeoutPicker` / `AppEvent::SetConfigScriptTimeout` events for the two-step flow. The setting is persisted to `[tui]` in `config.toml` via `persist_script_timeout_setting()`.

**Configurable Hotkeys:**

Keyboard shortcuts are configurable through the `/config` panel ("Hotkeys" item) and persisted under `[tui.hotkeys]` in `config.toml`. The implementation is split across two layers:

- **Config layer** (`@/codex-rs/acp/src/config/types/mod.rs`): Defines `HotkeyAction`, `HotkeyBinding`, and `HotkeyConfig` as terminal-agnostic string-based types. No crossterm dependency.
- **TUI layer** (`@/codex-rs/tui/src/nori/hotkey_match.rs`): Converts `HotkeyBinding` strings to crossterm `KeyEvent` matches via `parse_binding()` and `matches_binding()`. Also provides `key_event_to_binding()` for the reverse direction (capturing a key press as a binding string).

The `App` struct holds a `hotkey_config: HotkeyConfig` field loaded at startup. In `handle_key_event()` (`app/event_handling.rs`), configurable hotkeys are checked before the structural `match` block -- if a binding matches, the action fires and returns early. Changes are persisted via `persist_hotkey_setting()` (`app/config_persistence.rs`) which uses `ConfigEditsBuilder` to write to `[tui.hotkeys]` and updates the in-memory `HotkeyConfig` for immediate effect.

Hotkey actions fall into two categories that are consumed at different layers:

| Category | Actions | Consumed By |
|----------|---------|-------------|
| App-level | OpenTranscript, OpenEditor | `app/event_handling.rs::handle_key_event()` |
| Editing | MoveBackwardChar, MoveForwardChar, MoveBeginningOfLine, MoveEndOfLine, MoveBackwardWord, MoveForwardWord, DeleteBackwardChar, DeleteForwardChar, DeleteBackwardWord, KillToEndOfLine, KillToBeginningOfLine, Yank | `textarea/mod.rs::input()` |
| UI triggers | HistorySearch | `chat_composer/key_handling.rs` |

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
| Insert | Default mode. Characters are inserted as typed. Press `Escape` to enter Normal mode; the cursor moves back one position (standard vim behavior), but never past the beginning of the current line. |
| Normal | Navigation and editing mode. Keys are interpreted as commands rather than character input. |

Normal mode supports standard vim keybindings:

| Category | Keys | Behavior |
|----------|------|----------|
| Navigation | `h`/`j`/`k`/`l` (or arrow keys) | Move cursor left/down/up/right |
| Navigation | `w`/`b`/`e` | Forward/backward/end-of-word navigation (`w` moves to start of next word, `b` to start of previous word, `e` to end of current/next word) |
| Navigation | `0`/`$`/`^` | Beginning of line / end of line / first non-whitespace on line |
| Navigation | `G`/`gg` | End of text / beginning of text |
| Insert entry | `i`/`a` | Enter Insert at cursor / after cursor |
| Insert entry | `I`/`A` | Enter Insert at beginning of line / end of line |
| Insert entry | `o`/`O` | Open new line below/above and enter Insert |
| Editing | `x` | Delete character under cursor |
| Editing | `D`/`C` | Delete to end of line (`C` also enters Insert mode) |
| Editing | `dd` | Delete current line |
| Editing | `p` | Paste from kill buffer |
| Undo/Redo | `u` | Undo last edit or insert session |
| Undo/Redo | `Ctrl-R` | Redo last undone edit or insert session |

Two-key sequences (`gg`, `dd`) use a `vim_pending_key: Option<char>` field on TextArea. Pressing `g` or `d` sets the pending key; the second keypress either completes the sequence or cancels it (non-matching keys are discarded).

**Undo/Redo with Insert-Session Grouping:**

The textarea maintains undo/redo stacks of `(text, cursor_pos)` snapshots, capped at 500 entries. In vim mode, all edits made during a single insert session (from entering Insert mode to pressing Escape) are grouped into a single undo unit. This matches standard vim behavior where `u` undoes the entire insert session rather than individual keystrokes.

The grouping mechanism uses `begin_undo_group()` / `end_undo_group()`: entering Insert mode (via `i`, `a`, `A`, `I`, `o`, `O`, `C`, `S`) saves a snapshot and sets `in_undo_group = true`, suppressing per-keystroke snapshots. Pressing Escape to return to Normal mode calls `end_undo_group()`. Outside of vim mode (or when `in_undo_group` is false), each mutation via `insert_str_at()` or `replace_range_raw()` saves its own snapshot. `set_text()` clears both stacks since it represents a complete replacement of the buffer content (e.g., history navigation).

The state machine is implemented in `textarea/mod.rs` via the `VimModeState` enum. Vim mode handling runs as "stage 0" in the `input()` method, before C0 control fallbacks, configurable hotkey bindings, and hardcoded bindings. When in Normal mode, `chat_composer/mod.rs` bypasses paste burst detection and sends input directly to the textarea so navigation keys work without interference.

Config changes emit `AppEvent::SetConfigVimMode`, handled in `app/config_persistence.rs` via `persist_vim_mode_setting()`. The setting propagates down the same chain as hotkeys: App -> ChatWidget -> BottomPane -> ChatComposer -> TextArea via `set_vim_mode_enabled()`. When vim mode is disabled, the state resets to Insert mode.


**History Search (Configurable Hotkey):**

The history search hotkey is configurable via the `HotkeyAction::HistorySearch` binding (default: `Ctrl+R`). The `ChatComposer` key handler uses `matches_binding()` against the configured binding rather than a hardcoded key pattern. This allows users to remap history search when `Ctrl+R` conflicts with other bindings (e.g., vim redo).

In vim Normal mode, `Ctrl+R` is handled by the textarea as redo before the composer's key handler runs, so the default `HistorySearch` binding does not fire. In Insert mode, the composer's key handler runs and opens history search as expected. Users who want history search accessible in Normal mode can rebind it to a different key.

The history search popup follows the same `ActivePopup` pattern as the slash command popup (`Command`) and file mention popup (`File`). The popup is implemented in `history_search_popup.rs` using the shared `ScrollState` and `MAX_POPUP_ROWS` infrastructure from `popup_consts.rs`.

Data flow:
```
History search hotkey pressed in ChatComposer
  -> Op::SearchHistoryRequest { max_results: 500 }
  -> AcpBackend spawns blocking read of history.jsonl via search_entries()
  -> EventMsg::SearchHistoryResponse
  -> ChatWidget -> BottomPane -> ChatComposer::on_search_history_response()
  -> HistorySearchPopup::set_entries()
```

All entries are loaded once when the popup opens; filtering is performed client-side (case-insensitive substring match on each keystroke). The popup manages its own lifecycle -- the post-key-event `sync_command_popup()` / `sync_file_search_popup()` cycle is skipped when `ActivePopup::HistorySearch` is active, preventing those syncs from closing the history popup.

Vim mode is inherited from the composer's current vim state. When vim mode is enabled, the popup starts in Insert mode (for typing search queries) and supports Esc to enter Normal mode (j/k navigation), then a second Esc to close.

**Status Line Footer:**

The footer displays configurable segments, each of which can be enabled/disabled via `/config` -> "Footer Segments" or via `[tui.footer_segments]` in config.toml:

| Segment | TOML Key | Description |
|---------|----------|-------------|
| Task Summary | `prompt_summary` | "Task: <summary>" (dim) - generated by ACP backend on first user prompt |
| Vim Mode | `vim_mode` | "NORMAL" (blue/bold) or "INSERT" (green) when vim mode is enabled |
| Git Branch | `git_branch` | Current branch name with ⎇ symbol (yellow for main repo, orange for worktree) |
| Worktree Name | `worktree_name` | "Worktree: {name}" (light red) when running in an auto-worktree session -- the immutable directory name, distinct from the git branch which gets renamed after the first prompt |
| Git Stats | `git_stats` | Lines added/removed in current session |
| Context Window | `context` | "Context: 34K (27%)" when running within an agent environment |
| Approval Mode | `approval_mode` | "Approvals: Agent/Full Access/Read Only" |
| Nori Profile | `nori_profile` | "Skillset: <name>" (prefers session_skillset_name when set) |
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

The TUI detects the repo root for auto-worktree branch renaming by inspecting the cwd path structure: when `auto_worktree.is_enabled()` (true for both `Automatic` and `Ask` variants) and the cwd's parent directory is named `.worktrees`, the grandparent is treated as the repo root. This value is passed as `auto_worktree_repo_root` in `AcpBackendConfig` (see `chatwidget/agent.rs`). The branch rename is fire-and-forget; the working directory does not change during a session, so the TUI does not need to handle directory changes.

**External Editor Integration (`editor.rs`):**

The external editor hotkey (default Ctrl-G, configurable via hotkeys) opens the user's preferred text editor for composing prompts. The editor is resolved from `$VISUAL` > `$EDITOR` > platform default (`vi` on Unix, `notepad` on Windows). The lifecycle in `app/session_setup.rs::open_external_editor()`:

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
- `app/session_setup.rs::display_viewonly_transcript()`: Renders entries in the chat history

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

`spawn_acp_agent_resume()` in `@/codex-rs/tui/src/chatwidget/agent.rs` mirrors `spawn_acp_agent()` but calls `AcpBackend::resume_session()` instead of `AcpBackend::spawn()`, passing both the optional `acp_session_id` and the full `Transcript`. The spawned task structure (op forwarding, event forwarding, agent command handling) is identical.

**Agent Connection Lifecycle & Failure Recovery:**

Agent registration validation is performed exclusively in `spawn_agent()` (`chatwidget/agent.rs`). When `acp_allow_http_fallback` is disabled and the configured model is not in the ACP registry, `spawn_agent()` routes to `spawn_error_agent()` which sends `AppEvent::AgentSpawnFailed` -- triggering `on_agent_spawn_failed()` to display the error and reopen the agent picker for recovery. There is no early validation in `App::run()`; this single validation point ensures that unregistered agents (including custom agents that were configured but later removed) always get graceful recovery through the agent picker rather than a fatal startup error.

When the user selects an agent (or resumes a session), the TUI shows a "Connecting to [Agent]" status indicator via `ChatWidget::show_connecting_status()`. Each spawn function (`spawn_acp_agent`, `spawn_acp_agent_resume`, `spawn_http_agent`) uses a `tokio::select!` to race three concurrent futures during backend initialization:

| Arm | Trigger | Action |
|-----|---------|--------|
| Backend init completes (success) | `AcpBackend::spawn()` / `resume_session()` returns `Ok` | Proceeds to op forwarding and event forwarding |
| Backend init completes (failure) | Returns `Err` | Sends `AppEvent::AgentSpawnFailed`, drops `codex_op_rx` |
| `drain_until_shutdown()` | User sends `Op::Shutdown` during connection | Sends `AppEvent::ExitRequest`, drops `codex_op_rx` |
| `spawn_timeout_sequence()` | 8s warning + 30s abort elapse | Sends warning at 8s, then `AgentSpawnFailed` at 38s, drops `codex_op_rx` |

`drain_until_shutdown()` reads ops from the channel, discarding everything until it sees `Op::Shutdown`. This allows the user to exit (via `/exit`, Ctrl-C) even while the backend is still attempting to connect. `spawn_timeout_sequence()` provides user feedback: at 8 seconds it sends a `WarningEvent` visible in the chat, and after 30 more seconds it aborts the connection attempt entirely.

`on_agent_spawn_failed()` in `chatwidget/helpers.rs` performs three recovery steps in order:
1. Clears the "Connecting" status indicator via `bottom_pane.hide_status_indicator()`
2. Displays an error message in chat history: "Failed to start agent '{name}': {error}"
3. Reopens the agent picker so the user can select a different agent

**Status Indicator Whimsical Messages (`status_indicator_widget.rs`):**

When the agent begins processing a task, the `StatusIndicatorWidget` displays an animated header with a randomly selected tongue-in-cheek message (e.g., "Thinking really hard", "Hallucinating responsibly") drawn from the `WHIMSICAL_STATUS_MESSAGES` pool via `random_status_message()`. A new random message is selected each time `on_task_started()` fires in `chatwidget/event_handlers.rs`. During streaming, reasoning chunk headers (extracted from bold markdown text) dynamically replace this initial message via `update_status_header()`.

**Exit Path When Backend Is Dead:**

Every error/timeout/shutdown arm in the `tokio::select!` explicitly calls `drop(codex_op_rx)` before returning. This closes the receiver end of the channel so that `codex_op_tx` (held by `ChatWidget`) has no listener. If the user then attempts to exit (via `/exit`, `/quit`, or Ctrl-C), `submit_op(Op::Shutdown)` detects the dead channel (the `send()` returns `Err`) and falls back to sending `AppEvent::ExitRequest` directly via `app_event_tx`. This ensures the TUI can always exit cleanly even when no backend is running.

**Loop Mode (Prompt Repetition):**

Loop mode allows the same first prompt to be re-run multiple times, each time in a completely fresh conversation session. This is configured via `/config` -> "Loop Count" or by setting `loop_count` in `config.toml` (see `@/codex-rs/acp/src/config/types/mod.rs`).

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

The loop is cancelled (both fields set to `None`) when an error occurs (`on_error()`) or the user interrupts (`on_interrupted_turn()`). The `/config` sub-picker is a custom `BottomPaneView` implemented by `LoopCountPickerView` in `@/codex-rs/tui/src/nori/loop_count_picker.rs`. It offers preset options (Disabled, 2, 3, 5, 10) plus a "Custom..." option that enters an input mode where the user can type an arbitrary number (2-1000). Values <= 1 are treated as disabled, values > 1000 are capped. This follows the same `BottomPaneView` pattern used by `HotkeyPickerView`. The setting persists to `[tui]` in `config.toml` via `persist_loop_count_setting()`.

**History Insertion and Scrollback (`insert_history.rs`, `tui.rs`):**

`insert_history_lines()` pushes content into the terminal's native scrollback buffer above the ratatui viewport without disturbing ratatui's diff-based renderer. It works by manipulating ANSI scroll regions (DECSTBM, `\x1b[Pt;Pbr`) directly against the crossterm backend writer, bypassing the normal ratatui render pass. It returns `io::Result<bool>` where `false` means no room was available above the viewport (`area.top() == 0`) and the lines were not inserted.

The insertion algorithm:

```
1. If viewport is not at screen bottom: scroll viewport downward using RI (ESC M) inside
   a temporary scroll region covering [viewport.top()+1 .. screen_height].
2. Early return false if area.top() == 0 (viewport fills the whole screen; no space above it).
3. Set scroll region to [1 .. area.top()] (only the history area above the viewport).
4. Write lines into that region with \r\n advancement.
5. Reset scroll region to full screen.
6. Restore cursor to its pre-call position.
7. Return true.
```

The critical invariant: **DECSTBM `Pb=0` means "bottom of screen"**, not row 0. Calling `SetScrollRegion(1..0)` when `area.top() == 0` produces `\x1b[1;0r`, which sets the scroll region to the entire terminal rather than an empty region. Any subsequent writes then scroll through the viewport, corrupting ratatui's content in ways the diff-based renderer cannot detect. The `area.top() == 0` early return guards against this.

Two crossterm `Command` implementations support the function:
- `SetScrollRegion(Range<u16>)` — emits `\x1b[{start};{end}r`
- `ResetScrollRegion` — emits `\x1b[r` (restores full-screen scrolling)

**Viewport Repositioning in the Draw Loop (`tui.rs` `Tui::draw`):**

The draw loop manages viewport position bidirectionally to ensure the viewport stays anchored to the bottom of the terminal screen:

```
area.bottom() > size.height  --> viewport grew past screen bottom
                                  scroll history up, reposition viewport to bottom

area.y == 0 && height < size --> viewport was full-screen and has shrunk
                                  write pending lines directly into vacated rows,
                                  then reposition viewport to bottom
```

Both branches set `area.y = size.height - area.height`. The shrink branch guards on `area.y == 0` specifically because the stale-content problem only occurs when the viewport was at the top of the screen (full-screen). Normal height fluctuations where `area.y > 0` do not need repositioning because the viewport is already positioned with room above it.

When the shrink branch fires, the rows above the new viewport position contain stale rendered widget content from when the viewport was full-screen. Using `insert_history_lines()` here would push that stale content into terminal scrollback via the DECSTBM scroll region mechanism. Instead, the draw loop calls `write_pending_lines_directly()` to overwrite those rows in-place. If there are no pending history lines, the vacated rows are cleared directly.

**Direct Write for Vacated Rows (`insert_history.rs` `write_pending_lines_directly`):**

`write_pending_lines_directly()` writes history lines to specific terminal positions using `MoveTo` commands without scroll regions. This prevents stale viewport content from leaking into terminal scrollback. It is only used during the viewport shrink-from-full-screen transition in `Tui::draw`.

The function bottom-aligns content within the available rows (the last consumed line sits immediately above the viewport). It word-wraps each line individually to count screen rows, drains as many lines as fit from the input `Vec`, clears any remaining rows above the written content, then writes each wrapped line at its target position. Unconsumed lines remain in the `Vec` for later insertion via `insert_history_lines()`.

**Pending History Lines Retry Semantics:**

`Tui` holds a `pending_history_lines: Vec<Line>` buffer. On each draw, if the buffer is non-empty, `insert_history_lines()` is called. The buffer is only cleared when `insert_history_lines` returns `true` (lines were actually inserted). When it returns `false` (viewport at `y=0`, no room), the buffer is retained and insertion is retried on subsequent draws. This means once the viewport repositioning logic moves the viewport away from `y=0`, the retained lines will be inserted on the next frame. The buffer is capped at 1000 lines to prevent unbounded growth while the viewport is full-screen and insertion is blocked.

### Things to Know

**Module Structure Convention:**

Large modules use a directory layout (`foo/mod.rs` + submodules) instead of a single `foo.rs` file. This separates concerns and keeps individual files manageable. Modules using this pattern include `app/` (with `event_handling.rs`, `config_persistence.rs`, `session_setup.rs`), `chatwidget/` (with `event_handlers.rs`, `helpers.rs`, `user_input.rs`, `key_handling.rs`, `constructors.rs`, `approvals.rs`, `pickers.rs`, `login.rs`, `agent.rs`, `session_header.rs`, `interrupts.rs`, `pending_exec_cells.rs`), `bottom_pane/chat_composer/` (with `key_handling.rs`, `paste_handling.rs`, `popup_management.rs`, `rendering.rs`), `bottom_pane/textarea/`, `resume_picker/` (with `helpers.rs`, `rendering.rs`, `state.rs`, `tests.rs`), `history_cell/`, and `nori/session_header/`. Test submodules use `tests/mod.rs` + `tests/part*.rs` for large test suites (e.g., `bottom_pane/textarea/tests/`). Snapshot `.snap` files live in a `snapshots/` subdirectory within each test module directory.

**Cargo Feature Flags:**

| Feature | Dependencies | Default | Purpose |
|---------|--------------|---------|---------|
| `unstable` | `codex-acp/unstable` | Yes | Unstable ACP features like agent switching |
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
- The `chatwidget/` module (split across `mod.rs` + submodules) contains most of the chat rendering logic
- The `first_prompt_text` field in `ChatWidget` is set when the user submits their first message and is used for both transcript matching in Claude Code sessions and as the prompt text replayed during loop mode iterations

Created and maintained by Nori.
