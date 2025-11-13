# UI Component Inventory

This document catalogues every user-interface component and helper that currently lives under `codex-rs/tui/src`. It supplements `UI_LEGACY.md` by describing the purpose, relative complexity, major dependencies, and Codex-specific coupling for each piece. Use it as a checklist when promoting code into `tui-components`.

Legend for complexity:
- **Low** – Thin wrappers or helpers with limited branching.
- **Medium** – Moderate state management or multiple dependencies.
- **High** – Significant logic, async/event coupling, or deep Codex integration.

## Already Shared via `tui-components`

| Component | Purpose | Complexity | Key Dependencies | Codex-Specific Logic |
| --- | --- | --- | --- | --- |
| `render::{Renderable, ColumnRenderable, RowRenderable, InsetRenderable}` | Layout primitives that can measure height before rendering. | Medium | `ratatui` widgets & buffer APIs. | Purely generic. |
| `key_hint::KeyHint` | Platform-aware shortcut labels. | Low | `crossterm`, `ratatui`. | None. |
| `shimmer::Shimmer` | Animated loading text. | Low | `ratatui::style`, timers. | None. |
| `scroll_state::ScrollState` | List selection & scroll bookkeeping. | Low | `ratatui`, std collections. | None. |
| `paste_burst::PasteBurst` | Timing-based paste detection. | Medium | `std::time`, `unicode_width`. | None. |

## Codex-Only Component Catalog

### Infrastructure & Shell

- **`app.rs`** – Orchestrates the whole TUI event loop and owns `App` state. **Complexity:** High. **Dependencies:** `codex-core`, `codex-protocol`, `tui::Tui`, async runtime. **Codex logic:** tightly bound to auth, sandboxing, and orchestrating agent turns.
- **`tui.rs`** – Wrapper around the Ratatui terminal backend (setup, draw, resize handling). **Complexity:** Medium. **Dependencies:** `crossterm`, `ratatui`, `custom_terminal`. **Codex logic:** integrates approval policy awareness for pausing draws.
- **`cli.rs`** – CLI argument parser for `codex-tui`. **Complexity:** Medium. **Dependencies:** `clap`, `codex-core` config overrides. **Codex logic:** exposes Codex-specific flags like `--oss`, approvals, extra dirs.
- **`app_event.rs` / `app_event_sender.rs`** – Defines internal event enum plus dispatcher for UI<->background communication. **Complexity:** Medium. **Dependencies:** `tokio::sync`, `codex-core` task responses. **Codex logic:** event variants reference Codex session concepts.
- **`app_backtrack.rs`** – Handles ESC/backtrack flows that rewind history. **Complexity:** Medium. **Dependencies:** `App`, `history_cell`. **Codex logic:** manipulates Codex plan state.
- **`streaming/{mod.rs, controller.rs}`** – Manages SSE streaming from Codex backend into UI-friendly updates. **Complexity:** High. **Dependencies:** `codex-core` streaming APIs, `tokio`. **Codex logic:** all payload schemas Codex-specific.
- **`updates.rs` / `update_prompt.rs`** – Surfaces software update notifications and modal UI. **Complexity:** Medium. **Dependencies:** `codex-common` release metadata, `render` helpers. **Codex logic:** references Codex version channels.
- **`version.rs`** – Reports version/build info shown in UI. **Complexity:** Low. **Dependencies:** build-time env. **Codex logic:** yes.
- **`additional_dirs.rs`** – Validates user-provided writable directories and emits warnings. **Complexity:** Low. **Dependencies:** `codex_core::config`. **Codex logic:** pure sandbox policy.

### Input & Composer Stack (`bottom_pane/`)

- **`bottom_pane/mod.rs` (BottomPane)** – Hosts the composer plus pop-up stack, ties into status indicator, queued messages, and approval overlays. **Complexity:** High. **Dependencies:** `AppEventSender`, `StatusIndicatorWidget`, `render::Renderable`. **Codex logic:** drives Ctrl+C backtracking, context window info, queue display.
- **`chat_composer.rs`** – Multi-line text area with slash commands, custom prompts, attachment handling, file search, and footer hints. **Complexity:** High. **Dependencies:** `TextArea`, `ChatComposerHistory`, `SlashCommand`, `PasteBurst`, popups. **Codex logic:** integrates plan commands, context window percent, custom prompt placeholders, `AppEvent` dispatch.
- **`textarea.rs`** – Custom multiline editor with cursor handling, scrollback, bracketed paste support. **Complexity:** Medium. **Dependencies:** `ratatui` buffer math, `unicode_width`, `textwrap`. **Codex logic:** minimal besides placeholder text; prime candidate for extraction.
- **`chat_composer_history.rs`** – Maintains recall of submitted prompts plus slash command tokens. **Complexity:** Low. **Dependencies:** std collections. **Codex logic:** stores Codex prompt metadata (custom prompt IDs).
- **`footer.rs`** – Renders context-sensitive footer hints (shortcuts, status). **Complexity:** Medium. **Dependencies:** `render::Renderable`, composer state. **Codex logic:** references Codex modes (feedback, approval, ESC backtrack).
- **`footer.rs` helpers (`esc_hint_mode`, `footer_height`, etc.)** – Layout math for hint area. **Complexity:** Low. **Dependencies:** `ratatui::layout`. **Codex logic:** references Codex-specific hints.
- **`approval_overlay.rs` / `ApprovalRequest`** – Modal explaining workspace approval prompts. **Complexity:** Medium. **Dependencies:** `AppEventSender`, `render`. **Codex logic:** approval types + copy.
- **`command_popup.rs`** – Renders slash-command picker with filtering. **Complexity:** Medium. **Dependencies:** `SlashCommand`, `scroll_state`. **Codex logic:** lists Codex commands.
- **`file_search_popup.rs`** – UI for inserting file paths from recent search results. **Complexity:** Medium. **Dependencies:** `codex-file-search`, `scroll_state`. **Codex logic:** uses Codex search service.
- **`list_selection_view.rs` / `selection_popup_common.rs`** – Generic modal list selection (reused by feedback, prompt args). **Complexity:** Medium. **Dependencies:** `scroll_state`, `render::Renderable`. **Codex logic:** entries often include Codex-specific copy.
- **`custom_prompt_view.rs` / `prompt_args.rs`** – Handles inserting custom prompt arguments and validation. **Complexity:** Medium. **Dependencies:** `codex_protocol::custom_prompts`. **Codex logic:** yes; references prompt schema.
- **`feedback_view.rs`** – Feedback capture UI with consent toggles. **Complexity:** Medium. **Dependencies:** `AppEventSender`, `QueuedUserMessages`. **Codex logic:** hits Codex feedback endpoints.
- **`queued_user_messages.rs`** – Stores queued outbound messages while agent is busy. **Complexity:** Low. **Dependencies:** `ratatui`, `status_indicator_widget`. **Codex logic:** interacts with session timeline.
- **`paste_burst.rs`** – Older copy of paste detection (superseded by shared crate). **Complexity:** Medium. **Dependencies:** `std::time`. **Codex logic:** none; should be deduped.
- **`scroll_state.rs`** – Popup-specific scroll helper (also superseded by shared version). **Complexity:** Low. **Dependencies:** `ratatui`. **Codex logic:** none.
- **`textarea.rs`** – Already listed; critical nested component.
- **`popup_consts.rs`** – Layout constants for modal paddings. **Complexity:** Low. **Dependencies:** none. **Codex logic:** none.
- **`bottom_pane_view.rs`** – Trait for alternate panes (file search, approvals). **Complexity:** Low. **Dependencies:** `Renderable`. **Codex logic:** hooking into composer focus.
- **`paste_burst.rs::FlushResult`** – Exposes statuses consumed by composer. **Complexity:** Low. **Codex logic:** none.

### Transcript, Cells, and Streaming Output

- **`chatwidget.rs` & `chatwidget/`** – Renders the main conversation timeline, agent headers, interrupt banners. **Complexity:** High. **Dependencies:** `history_cell`, `frames`, `render::Renderable`, `AppEventSender`. **Codex logic:** session models, plan updates, interrupts referencing agent calls.
  - **`chatwidget/agent.rs`** – Agent header rows with status icons. **Complexity:** Medium. **Dependencies:** `codex-core` agent metadata, `status_indicator_widget`. **Codex logic:** displays Codex agent types.
  - **`chatwidget/interrupts.rs`** – Visualizes interrupts/backtracks. **Complexity:** Medium. **Dependencies:** `app_backtrack`, `status`. **Codex logic:** uses Codex interrupt stream.
  - **`chatwidget/session_header.rs`** – Displays conversation info (workspace, sandbox). **Complexity:** Low. **Dependencies:** `codex-core` config. **Codex logic:** yes.
- **`history_cell.rs`** – Converts Codex history entries (user, tool, plan) into Ratatui renderables. **Complexity:** High. **Dependencies:** `codex-protocol`, `markdown_render`, `diff_render`, `line_utils`, snapshots. **Codex logic:** deep knowledge of Codex task schema.
- **`exec_cell/{mod.rs, model.rs, render.rs}`** – Displays execution cells and streaming shell output. **Complexity:** Medium. **Dependencies:** `diff_render`, `text_formatting`. **Codex logic:** command model references Codex shell runner.
- **`session_log.rs`** – Shows session transcript in onboarding/resume flows. **Complexity:** Low. **Dependencies:** `history_cell`. **Codex logic:** uses session metadata.
- **`frames.rs`** – Scrollback manager for history panes; handles virtualization. **Complexity:** Medium. **Dependencies:** `scroll_state`, `AppEventSender`. **Codex logic:** sized according to Codex turn semantics.
- **`live_wrap.rs`** – Incremental wrapper for streaming lines to match terminal width. **Complexity:** Medium. **Dependencies:** `wrapping`, `ratatui::Line`. **Codex logic:** none beyond hooking into streaming controller.
- **`streaming/` (covered earlier)** – Input to timeline updates.

### Rendering, Styling, and Text Utilities

- **`render/` (mod.rs, renderable.rs, line_utils.rs, highlight.rs)** – Local copy of render traits and helpers still used inside codex before swapping to `tui-components`. **Complexity:** Medium. **Dependencies:** `ratatui`, `textwrap`. **Codex logic:** none; extraction target.
- **`wrapping.rs`** – Text/Line wrapping utilities with indent support. **Complexity:** Medium. **Dependencies:** `textwrap`, `ratatui::Line`. **Codex logic:** none.
- **`text_formatting.rs`** – Helpers for diff prefixes, truncation, width calculations. **Complexity:** Medium. **Dependencies:** `unicode-width`, `textwrap`. **Codex logic:** color choices reflect Codex style.
- **`style.rs`** – Central palette + style helpers for user/assistant/system text. **Complexity:** Medium. **Dependencies:** `ratatui::style`, `terminal_palette`. **Codex logic:** palette derived from Codex brand.
- **`terminal_palette.rs`** – Maps theme colors & shimmer gradients. **Complexity:** Low. **Dependencies:** `ratatui::style`, `supports-color`. **Codex logic:** brand colors.
- **`color.rs`** – Additional color helpers (contrast, gradients). **Complexity:** Low. **Dependencies:** `palette` crates (via `ratatui`). **Codex logic:** brand mapping.
- **`ui_consts.rs`** – Shared layout constants (padding, column widths). **Complexity:** Low. **Dependencies:** none. **Codex logic:** tuned for Codex interface ratios.
- **`ascii_animation.rs`** – Simple spinner/wave animation (legacy). **Complexity:** Low. **Dependencies:** `std::time`. **Codex logic:** copy text.
- **`shimmer.rs` & `key_hint.rs` (local copies)** – Legacy implementations prior to extraction; still referenced until dependency flips. **Complexity:** Low. **Dependencies:** `ratatui`. **Codex logic:** none.
- **`public_widgets/composer_input.rs`** – Public widget used by other crates for composer text plus hint metadata. **Complexity:** Medium. **Dependencies:** `bottom_pane::ChatComposer`. **Codex logic:** references composer commands.

### Markdown, Diff, and Rich Content

- **`markdown.rs` / `markdown_render.rs` / `markdown_stream.rs` / `markdown_render_tests.rs`** – Markdown parser, renderer, stream-friendly adapter, and snapshot tests. **Complexity:** High. **Dependencies:** `pulldown-cmark`, `ratatui`, `textwrap`, `render::line_utils`. **Codex logic:** toggles for plan formatting, links to CLI commands.
- **`diff_render.rs`** – Presents unified/inline diffs with apply/undo hints. **Complexity:** High. **Dependencies:** `diffy`, `textwrap`, `render`. **Codex logic:** annotate Codex plan/apply context, includes MCP tool call references.
- **`get_git_diff.rs`** – Runs git diff commands and preprocesses output for renderer. **Complexity:** Medium. **Dependencies:** `tokio::process`, `diff_render`. **Codex logic:** integrates sandbox path mapping.
- **`file_search.rs`** – Backend for search overlay, formats matches. **Complexity:** Medium. **Dependencies:** `codex-file-search`, `render`. **Codex logic:** uses Codex search RPC.
- **`pager_overlay.rs`** – Fullscreen overlay for transcripts/diffs with scrolling, search, streaming updates. **Complexity:** High. **Dependencies:** `render::Renderable`, `textwrap`, `history_cell`. **Codex logic:** triggered by Codex commands like `/show-resume`.
- **`resume_picker.rs`** – UI to select previous sessions/resumes. **Complexity:** Medium. **Dependencies:** `codex-core` session metadata, `scroll_state`. **Codex logic:** entirely Codex-driven.
- **`selection_list.rs`** – Legacy standalone selector widget (outside bottom pane). **Complexity:** Medium. **Dependencies:** `scroll_state`, `render`. **Codex logic:** generic but currently tied to Codex types.

### Status, Notifications, and Indicators

- **`status_indicator_widget.rs`** – Inline status bubble above composer showing active task + queued messages. **Complexity:** Medium. **Dependencies:** `render`, `textwrap`, `status::helpers`. **Codex logic:** statuses reference Codex task lifecycle.
- **`status/` (account.rs, card.rs, format.rs, helpers.rs, rate_limits.rs, tests.rs)** – Collects data for status sidebar cards (account info, rate limits, rolling windows). **Complexity:** Medium. **Dependencies:** `codex_core::status` data, `render`. **Codex logic:** entirely.
- **`status/mod.rs`** – Coordinates fetching/parsing account status, renders cards via `Renderable`. **Complexity:** High. **Dependencies:** `codex_core`, `AppEventSender`, `status_indicator_widget`. **Codex logic:** all logic.
- **`updates.rs` / `update_prompt.rs`** – (Already noted) overlay for CLI updates.
- **`session_log.rs`** – (mentioned) reused for onboarding/resume.

### Onboarding & Modal Flows (`onboarding/`)

- **`onboarding/mod.rs`** – Entry point for onboarding states (trust dir, login, WSL instructions). **Complexity:** Medium. **Dependencies:** `AppEventSender`, `render`. **Codex logic:** copy flows.
- **`onboarding/onboarding_screen.rs`** – Fullscreen onboarding TUI with multiple steps and async prompts. **Complexity:** High. **Dependencies:** `ratatui`, `scroll_state`, `codex_core` login APIs. **Codex logic:** ensures sandbox/trust requirements described.
- **`onboarding/auth.rs`** – Handles interactive auth prompts. **Complexity:** Medium. **Dependencies:** `CodexAuth`, `render`. **Codex logic:** yes.
- **`onboarding/trust_directory.rs`** – Guides user through trusting directories. **Complexity:** Low. **Dependencies:** `additional_dirs`. **Codex logic:** sandbox policy.
- **`onboarding/welcome.rs` & `windows.rs`** – Platform-specific onboarding copy. **Complexity:** Low. **Dependencies:** `render`. **Codex logic:** yes.

### Supporting Utilities & Glue

- **`clipboard_paste.rs`** – Normalizes pasted paths/images and generates temp files. **Complexity:** Medium. **Dependencies:** `image`, `tempfile`, `codex-core` path rules. **Codex logic:** decides placeholder text for agent uploads.
- **`ascii_animation.rs`** – Legacy spinner (noted above). **Complexity:** Low.
- **`slash_command.rs`** – Defines slash command metadata, parsing, help text. **Complexity:** Medium. **Dependencies:** `codex_protocol::custom_prompts`, `AppEventSender`. **Codex logic:** command set is Codex-specific.
- **`exec_command.rs`** – Runs shell commands triggered via UI (e.g., apply patch). **Complexity:** Medium. **Dependencies:** `tokio::process`, `codex_core::sandbox`. **Codex logic:** uses Codex seatbelt wrappers.
- **`file_search.rs`** – Already covered (content renderer + backend hook).
- **`get_git_diff.rs`** – Already covered.
- **`insert_history.rs`** – Utility for injecting content into history (used by unit tests / onboarding). **Complexity:** Low. **Dependencies:** `history_cell`. **Codex logic:** references internal history format.
- **`frames.rs`** – (covered above) virtualization helper.
- **`custom_terminal.rs`** – Cross-platform terminal backend customizing seatbelt support. **Complexity:** Medium. **Dependencies:** `crossterm`, OS-specific APIs. **Codex logic:** toggles sandbox env vars.
- **`ui_consts.rs`** – (noted).
- **`public_widgets/mod.rs`** – Currently only re-exports `composer_input`; future home for more components. **Complexity:** Low.
- **`test_backend.rs`** – Testing-only backend for vt100 snapshots. **Complexity:** Medium. **Dependencies:** `vt100`, `tokio`. **Codex logic:** none.

### Snapshot Directories

Each major visual module (`bottom_pane/snapshots`, `chatwidget/snapshots`, `status/snapshots`, `snapshots/` root) carries Insta snapshot fixtures. Migrating components requires porting or regenerating these under `tui-components/tests` to preserve visual behavior.

## Extraction Priorities

- Promote the generic pieces (TextArea, wrapping, selection list, popup scaffolding, shimmer/key hints duplicates) first—they have little Codex logic aside from copy.
- Higher-complexity items (ChatComposer, ChatWidget, HistoryCell, PagerOverlay, Status cards) demand new data transfer structs to decouple from Codex `App` structures before moving.
- Infrastructure modules (`app.rs`, streaming, onboarding auth) are intentionally Codex-specific and should stay in `codex-tui`, but documenting them here clarifies why they are out of scope for `tui-components`.

