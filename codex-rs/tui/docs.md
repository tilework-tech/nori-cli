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

Entry point is `main.rs` which delegates to `run_app()` in `lib.rs`. The main event loop in `app.rs` processes:

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
| `flush_completions_and_clear()` | `on_task_complete()` | Processes completion events (ExecEnd, McpEnd, PatchEnd) so in-progress tool cells transition to their finished state, then discards begin events that would create new cells. |

The selective flush at task completion ensures tool cells that are already visible transition from "Running" to "Ran", while preventing new "Explored" / "Ran" cells from appearing below the agent's final message.

The Nori-specific agent picker UI lives in `nori/agent_picker.rs`, allowing users to select between available ACP agents.

**System Info Collection** (`system_info.rs`):

The `SystemInfo` struct collects environment data in a background thread to avoid blocking TUI startup:

| Field | Source |
|-------|--------|
| `git_branch` | Git repository branch name |
| `nori_profile` | Active Nori profile |
| `git_lines_added` / `git_lines_removed` | Git diff statistics |
| `is_worktree` | Whether CWD is a git worktree |
| `transcript_location` | Discovered transcript path and token usage when running within an agent environment |

The `transcript_location` field includes both `token_usage` (total tokens) and `token_breakdown` (detailed input/output/cached breakdown) which are displayed in the TUI footer when Nori runs as a nested agent inside Claude Code, Codex, or Gemini.

Two collection methods are provided:
- `collect_for_directory()` - Basic collection without first-message matching (test-only)
- `collect_for_directory_with_message()` - Preferred method that passes the first user message to the transcript discovery layer for accurate Claude Code transcript identification

The first-message is obtained from `ChatWidget::first_prompt_text()`, which stores the text of the first submitted prompt. This flows through `SystemInfoRefreshRequest` to the background worker, enabling accurate transcript matching when multiple sessions exist in the same project directory.

**Slash Commands:**

| Command | Description |
|---------|-------------|
| `/agent` | Switch between available ACP agents |
| `/model` | Choose model and reasoning effort |
| `/approvals` | Choose what Nori can do without approval |
| `/config` | Toggle TUI settings (vertical footer, terminal notifications, OS notifications, vim mode, notify after idle, hotkeys, script timeout, loop count) |
| `/new` | Start a new chat during a conversation |
| `/init` | Create an AGENTS.md file with instructions |
| `/resume-viewonly` | View a previous session transcript (read-only) |
| `/compact` | Summarize conversation to prevent context limit |
| `/undo` | Open undo snapshot picker to select a restore point |
| `/diff` | Show git diff (including untracked files) |
| `/mention` | Mention a file |
| `/status` | Show session configuration and token usage |
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

The footer displays:
- Vim mode indicator (NORMAL in blue/bold, INSERT in green) when vim mode is enabled -- rendered as the first segment via `FooterProps.vim_mode_state`
- Current git branch (refreshes on transcript activity)
- Git diff statistics (lines added/removed)
- Context window usage (e.g., "Context: 34K (27%)") when running within an agent environment
- Approval mode label (e.g., "Agent", "Full Access", "Read Only")
- Model name
- Token usage breakdown (e.g., "Tokens: 45K in / 78K out (32K cached)") when running within an agent environment
- Key bindings (Ctrl+C, Esc, Enter)

Token data flows from `TranscriptLocation.token_breakdown` (provided by `codex_acp::discover_transcript_for_agent_with_message()`) through `FooterProps` to the footer renderer. The breakdown includes separate input, output, and cached token counts for accurate usage reporting.

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
