# Noridoc: nori-tui

Path: @/nori-rs/tui

### Overview

- `nori-tui` is the Ratatui frontend for the headless Nori harness. It owns
  input, rendering, presentation-only state, pickers, approvals, and terminal
  lifecycle.
- The crate adapts terminal events and shared component outcomes into
  application events. It does not own ACP transport or expose a second
  protocol vocabulary.

### How it fits into the larger codebase

```text
nori-tui
   |  typed HarnessHandle methods
   |  SessionEvent::{Acp, Nori}
   v
nori-harness -> nori-acp-host -> ACP agent
   ^
   |
nori-config
```

The TUI depends on `nori-harness`, `nori-config`, and `nori-protocol`; it does
not import the ACP host or ACP schema crate directly. Its ACP types arrive
through `nori_protocol::acp`.

Shared pickers receive [`nori-tui-components`](../tui-components/docs.md)
styles through [`component_theme`](src/style.rs). That adapter supplies the
terminal's reported RGB background only when stdout supports true color, so
neutral surfaces remain terminal-relative while the component library owns the
green pointer, cyan information, foreground title, and neutral selected-copy
grammar. Both bottom-pane pickers and the launch-time resume picker use this
path; raw key routing and application actions remain owned by the TUI.

At launch, the TUI supplies the harness with two Nori CLI context-envelope
variants. Both identify the first prompt as coming from Nori CLI; the
non-HTTP-MCP variant also explains the ACP fallback and unavailable MCP-backed
affordances. The harness chooses between them from the connected agent's
reported HTTP MCP capability and injects the selected envelope once. The CLI
does not add a user identity because authenticated Nori Sessions identity is
owned outside this layer. When that first prompt carries active goal context,
the HTTP-MCP envelope also states that Nori CLI owns the goal and routes reads
and updates to `get_goal` and `update_goal` from the `nori-client` MCP server,
never to similarly named native agent tools.

### Core Implementation

#### Source-first event dispatch

The application event loop matches `SessionEvent::Acp` and
`SessionEvent::Nori` before projecting either branch into UI state.

- ACP notifications drive private message, thought, plan, tool, usage, mode,
  live config, capability, and available-command presentation.
- Request-scoped updates belong to the active local prompt or load when one
  exists; otherwise the TUI preserves and renders each update as unowned
  activity without dropping or invented attribution, regardless of source or
  transport. The harness also emits one warning for the unowned burst without
  changing that presentation. Any unowned user, agent, thought, plan,
  tool-call, or tool-update content starts or confirms unowned
  presentation. Without a status hint, a later locally owned prompt or load
  start flushes and separates that output without inventing completion.
- The chat widget tracks local prompt ownership as
  `owned_prompt_request_id` and keeps a separate private
  `proactive_turn_active` presentation bit. The legacy identifier groups
  unowned presentation or presentation of an agent-owned turn, plus statistics
  and explicit-idle notifications. It does not set the bottom pane's
  operational task-running flag, so Ctrl-C cancellation, interrupt hints, and
  in-task command gating remain tied to locally owned requests. It never drives
  queue, cancel, loop, ACP phase, or request state.
- Ordinary metadata does not imply a turn. The Nori Sessions broker's optional
  `_meta.nori.status` extension establishes an agent-owned turn only when no
  locally owned prompt or load is active: exact `working` starts or confirms
  the turn and exact `idle` ends it. The status affects presentation only;
  unknown values remain ordinary session metadata. Known status-only frames
  are hidden; a known status combined with `title` or `updated_at` still
  displays those fields. This distinction lives in
  [`event_handlers.rs`](src/chatwidget/event_handlers.rs) and
  [`presentation/mod.rs`](src/presentation/mod.rs).
- Handroll's `_meta.nori.connection.status` extension is also presentation
  only. Exact `reconnecting` and `connected` status-only frames render as
  “Cloud connection lost. Reconnecting…” and “Cloud connection restored.”
  info entries instead of generic redacted metadata. They do not change turn
  ownership, replay ACP methods, or enter the session-info state reducer.
- ACP requests drive the permission overlay and retain their raw `RequestId`.
- Initialize responses and prompt responses matching the active request update
  UI lifecycle. For prompt errors, the correlated `NoriEvent::RequestFailed`
  drives completion, loop disposition, and user-visible display; the matching
  raw ACP error remains observable but is not rendered or completed a second
  time. Failures unrelated to the active prompt do not complete it. Other typed
  operations complete through their
  `HarnessHandle` return values while their raw responses remain observable on
  the stream.
- Nori events drive lifecycle, queue, replay, compaction, goals, undo,
  user-shell output, hooks, summaries, notices, and classified failures.

The private modules under `tui/src/presentation/` assemble ACP streaming values
into display cells and friendly labels. These view models are allowed to be
lossy and UI-specific; they are not fed back into the harness or exported from
`nori-protocol`.

#### Picker navigation and search state

Searchable pickers use two explicit interaction states instead of interpreting
every printable key as a query. They open in navigation state, where arrows and
`j`/`k` move the cursor. `f`, `/`, or Ctrl-F activates search without inserting
the activation key; active search accepts printable characters, including the
navigation and activation characters. Escape first clears and exits search,
then dismisses the picker if pressed again. The input row is present only while
search is active. Inactive footers intentionally show only the compact
`/ search` affordance; `f` and Ctrl-F remain supported aliases rather than
additional visible hint text. Active footers describe typing and search exit.

- [`BottomPane`](src/bottom_pane/mod.rs) routes Escape through the active
  [`BottomPaneView::on_escape`](src/bottom_pane/bottom_pane_view.rs) hook before
  ordinary key handling. The hook defaults to the existing Ctrl-C cancellation
  behavior, while searchable component and generic selection views override it
  to consume the first Escape without completing the view. This keeps
  multi-stage Escape state transitions independent from Ctrl-C cancellation;
  the next Escape follows the view's normal dismissal path.
- [`SelectionViewParams`](src/bottom_pane/list_selection_view.rs) keeps the
  caller-owned selection model and adds an explicit presentation choice.
  Standard picker factories opt into the shared presentation with `picker()`;
  [`BottomPane::show_selection_view`](src/bottom_pane/mod.rs) then adapts those
  rows into the domain-free picker instead of moving configuration types,
  application events, or callbacks into the component crate. Short legacy
  action lists may retain `ListSelectionView` and its numbered-row behavior.
- [`ComponentPickerView`](src/bottom_pane/component_picker_view.rs) maps
  Crossterm events into the domain-free `PickerAction` vocabulary from
  [`nori-tui-components`](../tui-components/docs.md). The adapter preserves
  current and initial selection, callbacks, keep-open actions, Shift-Tab
  actions, and typed footer hints while projecting names and descriptions into
  picker columns and details. The component's `search_active`, query,
  filtering, and typed outcomes remain the source of truth.
- Specialized picker state machines use the same renderer only for their
  browsing states. For example, the hotkey and MCP views retain rebinding,
  form input, validation, and application-event routing locally while their
  selectable rows use the shared columns, surfaces, hints, and focus rails.
- Transcript search remains composer-owned because it loads `HistoryEntry`
  values asynchronously. [`HistorySearchPopup`](src/bottom_pane/history_search_popup.rs)
  owns its search and filtered-selection state, while
  [`ChatComposer`](src/bottom_pane/chat_composer/key_handling.rs) applies the
  shared keyboard contract and routes an accepted entry back into the composer.

The pre-TUI [`resume_picker`](src/resume_picker/) cannot use a bottom-pane
adapter, but it mirrors the same transitions and projects its active state and
query into the reusable picker renderer. Full-screen CLI picker surfaces use
the component's opt-in symmetric selection rails, including this launch-time
picker; embedded or copyable output does not inherit the rail treatment.

#### Structured session information

Ordinary ACP `SessionInfoUpdate` normalization retains `title`, `updatedAt`,
and the complete `_meta` object as a private `SessionInfoPatch` alongside the
legacy text projection. The chat widget captures the agent identity reported
by ACP initialization and whether the current emission is live, agent replay,
or transcript replay, then sends every retained patch to two presentation
consumers:

- The history renderer emits a visible entry for every update. Known Codex
  status, goal, error, archived, and closed fields receive friendly labels only
  when the initialized agent identity is `codex-acp`. Unknown fields, fields
  from other agents, and malformed known fields are rendered recursively in
  deterministic path order as path/type pairs; their values are never
  displayed. Agent identity and other header components are sanitized and
  length-bounded, while fallback depth, assignment work, path length, and
  output count are capped with an explicit omission marker.
- The latest-state reducer recursively merges partial metadata for future
  footer and status-card consumers. Live values outrank agent replay, which
  outranks transcript replay. Empty metadata objects are merge no-ops and only
  an explicit nested `null` clears a path. Patch traversal, retained field
  count, and retained values are bounded; a new ACP initialization resets the
  accumulated state and its provenance.

This projection remains TUI-private. Raw ACP notifications continue to be the
only transcript records for these updates, so rendering session information
does not create derived transcript events or change the persisted schema.

#### Commands and approvals

User actions call typed `HarnessHandle` methods for prompting, cancellation,
history, prompt discovery, compaction, branching, undo, shell, goals, session
config, session listing, close, and shutdown. The old generic operation bus is
gone.

The `/fork` picker (`tui/src/nori/fork_picker.rs`) offers a "Branch from current
point" entry as its first item when the agent advertises ACP `session/fork`
(`AgentCapabilitiesView.session_fork`, sourced from the `InitializeResponse`);
with fork support it opens even with no earlier user messages. Selecting it
dispatches `AppEvent::BranchFromCurrent` -> `HarnessAction::Branch` ->
`handle.branch()`, which forks the current head via ACP `session/fork` and swaps
the active session. The remaining picker entries are the earlier user messages
and drive the unchanged rewind-to-message local fork path; agents without fork
support see only those.

When an ACP permission request arrives, the overlay renders the ACP subject and
options, then calls `respond_to_agent(request_id, response)` with the exact ID
from the request. Filesystem requests do not enter this path because the host
handles them. Terminal and extension request families are not advertised by
the current host.

#### Settings and session-config pickers

The `/settings` picker (TUI config, in `@/nori-rs/tui/src/nori/config_picker.rs`)
and the `/config` picker (ACP session config, in
`@/nori-rs/tui/src/nori/session_config_picker.rs`) return to their parent panel
after a value is applied, landing the cursor on the just-edited row. This lets a
user change several settings in one visit instead of reopening the slash command
after every change. Each panel re-derives its `initial_selected_idx` from a row
identifier: a `SettingsItem` enum for `/settings`, or the ACP option id for
`/config`.

`/settings` is searchable through the shared-picker adapter. Each setting
builds its search value from the displayed name and optional description before
the panel opens, so filtering can match either the setting identity or its
user-facing explanation without coupling the component state to configuration
types.

- For `/settings`, each `App::persist_*` success path in
  `@/nori-rs/tui/src/app/config_persistence.rs` calls
  `ChatWidget::reopen_settings_focused` (in
  `@/nori-rs/tui/src/chatwidget/pickers.rs`), which rebuilds the panel from the
  current config while preserving the ephemeral per-session loop-count override.
- For `/config`, a successful set in `@/nori-rs/tui/src/chatwidget/pickers.rs`
  re-emits `OpenAcpSessionConfigPicker` with the refreshed options and the edited
  `focus_config_id`.
- The Vim sub-picker is reachable from both `/settings` and the standalone
  `/vim` command, so `AppEvent::SetConfigVimMode` carries a `from_settings` flag;
  only settings-originated changes reopen the panel, while `/vim` just closes.
- Excluded from auto-reopen: the mode-cycle hotkey (Shift+Tab must not pop
  `/config` open), the multi-toggle Footer Segments sub-picker (which already
  stays open by replacing itself), and the bespoke Hotkeys view. Failed persists
  never reopen.
- Model-category value pickers (`acp_session_config_value_picker_params()`)
  split into two labeled sections when both have content. **Recommended** lists
  the values the agent advertises over ACP, exactly as reported. **Other** lists
  a curated, per-agent set of real models the adapter does *not* advertise but
  that generally run when forced via spawn-time injection; it comes from
  `nori_harness::AgentKind::other_models()`, resolved in
  `@/nori-rs/tui/src/chatwidget/pickers.rs` via `AgentKind::from_slug` on the
  active agent (custom/unknown agents resolve to an empty list → no Other
  section). The Other list is deduplicated at render time against the advertised
  values so a model an account already sees under Recommended never appears
  twice. When either side is empty — no advertised catalog, no curated
  complement, or a non-model option — the picker renders one flat list exactly as
  before and the labeled headers do not appear. The durable-complement design is
  intentional: the advertised set is agent/version/account-dependent, so the
  curated list plus runtime dedup avoids a second, drift-prone source of truth.
- An Other row whose id already equals the session's injected `currentValue` is
  marked `(current)` and carries no action; every other Other row emits
  `AppEvent::SetAcpSessionConfigOption { is_custom_model: true }` — the same event
  a free-text custom entry emits — so it follows the identical
  reject → persist-to-`[default_models]` → restart-with-injection recovery path
  (below). `initial_selected_idx` prefers the current row.
- Section labels remain non-selectable through `SelectionItem::is_header` in
  [`list_selection_view.rs`](src/bottom_pane/list_selection_view.rs). The shared
  adapter maps that flag to `PickerItem::section_heading`, so grouped model and
  config rows render as bold structure rather than faded disabled choices;
  keyboard navigation and default selection continue to skip them. Retained
  legacy lists project the same flag into `GenericDisplayRow` and keep their
  selectable-only numbering.
- `acp_session_config_value_picker_params()` also pins a "Use custom model..."
  entry at the bottom of every Model-category picker. Selecting it emits
  `AppEvent::OpenCustomModelInput`, which opens a `CustomModelInputView`
  (`@/nori-rs/tui/src/nori/custom_model_input.rs`) — a `BottomPaneView` text
  input for free-form model IDs. On submit it emits
  `AppEvent::SetAcpSessionConfigOption` and follows the same
  `session/set_config_option` path as selecting from the advertised list.
- The config-set events (`SetAcpSessionConfigOption` and
  `AcpSessionConfigSetResult` in `@/nori-rs/tui/src/app_event.rs`) carry an
  `is_custom_model` flag that distinguishes injectable Other or free-text model
  choices from an advertised picker choice. When the live RPC rejects one of
  these model choices and the active agent `supports_model_injection()`, the
  handler in `@/nori-rs/tui/src/app/event_handling.rs` treats the rejection as
  recoverable:
  `persist_custom_default_model` (in
  `@/nori-rs/tui/src/app/config_persistence.rs`) writes the value to
  `[default_models]` unconditionally (no config-options snapshot to categorize —
  it is a model by construction), an info message
  ("Saved '<model>' as the default model for <agent> — restarting the session to
  apply it.") is shown, and `AppEvent::NewSession` restarts the session so the
  model is injected at spawn. If the agent has no injection channel, the original
  error is surfaced instead.

#### The `/browser` profile picker

On Unix, `/browser` (in `@/nori-rs/tui/src/chatwidget/key_handling.rs`) reuses a
running browser if one is active; otherwise it loads the saved default
`browser_profile` from config and opens a three-tier picker
(`browser_profile_picker_params` in `@/nori-rs/tui/src/nori/config_picker.rs`)
pre-selected on it. The tiers are `Throwaway` (fresh temp profile, the secure
default), `Persistent nori profile` (nori-owned, logins persist), and `Real
Chrome profile` (the user's real Chrome, danger-noted). Selecting a tier emits
`AppEvent::SetBrowserProfile`, whose handler persists the choice as the new
default (`persist_browser_profile_setting`, writing the top-level
`browser_profile` key) and then launches Chrome with it via
`launch_browser_session`, which spawns `BrowserSession::launch_and_store(mode)`
in `nori-harness`.

#### Terminal requirements

Initialization requires stdout to be a terminal. It does not require the same of
stdin: crossterm reads keys from the controlling terminal whenever stdin is
redirected, so the guard only checks that a controlling terminal (`/dev/tty`, or
`CONIN$` on Windows) can actually be opened. The older stdin-must-be-a-tty rule
was nori's own policy rather than a crossterm limitation, and dropping it does
not change where input comes from.

This is what lets `echo "..." | nori` run as an ordinary interactive session. The
CLI composes the piped text into the normal `TuiCli` prompt field before launch,
so it seeds the first turn and nothing downstream distinguishes it from an
argument prompt (see `@/nori-rs/cli/docs.md`). A pipe never selects headless
behavior; that requires `nori exec` or `nori -p`, which never open a UI.

Having drained the pipe, the CLI re-points file descriptor 0 at the controlling
terminal before the UI starts. This matters to every child the TUI spawns with
inherited stdin -- the external editor and the file browser -- which would
otherwise be handed an EOF'd pipe and exit immediately.

#### Inline transcript reflow

In inline mode, terminal width changes rebuild the visible transcript at the
new width. Completed assistant messages retain their raw Markdown source, so
replay reruns Markdown layout instead of wrapping already-rendered lines.
Streaming output remains transient and is consolidated into one source-backed
assistant cell when the message finishes; user, tool, event, and assistant
cells otherwise keep their semantic order.

Reflow is width-only and trailing-debounced by 75 ms. It waits while a popup,
transcript overlay, alternate screen, or assistant stream owns presentation,
then runs once that boundary closes. The replay retains at most the newest
1,000 semantic cells and 10,000 physical rows, truncating from the oldest edge
and prefixing `… history truncated` plus a blank line when either limit applies.
These bounds limit terminal output, not the in-memory transcript.

Reflow deliberately clears the visible screen and the terminal's native
scrollback, including output from before Nori started, before replaying the
bounded history. It does not attempt to discover or preserve a scrollback
origin. The behavior is enabled by default and can be toggled through
`/settings` or `[tui] resize_reflow` in `config.toml`; disabling it cancels any
pending replay.

#### Lifecycle behavior

Connection preparation is separate from session activation. Every subprocess
agent starts through [`session_setup.rs`](src/app/session_setup.rs), including
ordinary local agents and a registered `nori-handroll acp --type remote`
adapter. `App` retains the harness's opaque `PreparedAgent` after `initialize`,
capability inspection, and optional `session/list`; no session directive is
issued merely because startup completed. Advertised listing runs in the same
background preparation while the composer remains usable. Unsupported listing
is distinct from an empty successful catalog, and preparation or
advertised-list failure leaves the widget sessionless.
[`chatwidget/agent.rs`](src/chatwidget/agent.rs) is the boundary that consumes
the exact prepared connection into the harness runtime. An ordinary-agent
failure reopens the existing agent picker so the user can recover by choosing
another agent unless a live candidate already owns its picker; cloud remains on
its sessionless `/resume` or `/new` retry flow.

Primary preparation is owned as a generation, task abort handle, current
intent, retained fork context, and optional pending activation. `/new` and a
first genuine user prompt record New; `/resume` changes the intent to open the
prepared catalog and then records the selected Resume. Esc-Esc backtrack and
transcript fork also queue deferred New while preserving their selected history
summary as initial context. If preparation is in flight, these decisions wait
for it instead of cancelling it; if preparation is complete, they consume the
stored connection. Thus `initialize`, optional `session/list`, and the chosen
`session/new`, `session/load`, or `session/resume` use one child process. This
orchestration lives in [`event_handling.rs`](src/app/event_handling.rs).

Every primary activation calls
[`take_refreshed_prepared_agent`](src/app/session_setup.rs) before consuming the
connection. The refresh applies current mutable approval and sandbox policy and
retains fork context. If process-defining identity changed, the TUI reaps the
stale child and reprepares while keeping the pending New or Resume decision;
stale policy is never activated.

The sessionless composer distinguishes activation input from local behavior.
The first text or image prompt remains in frontend state, requests New, and is
transferred without rewriting to the activated widget. The widget submits it
exactly once when `SessionStarted` establishes the configured session. Initial
positional prompts use the same path. Slash commands and local shell commands
are handled before this implicit-New decision; a shell command reports that no
harness is active until activation. Neither can claim an ACP session. These
ownership rules live in
[`user_input.rs`](src/chatwidget/user_input.rs) and
[`helpers.rs`](src/chatwidget/helpers.rs).

An `AgentPrepared` result is accepted only when its generation still matches
the owned preparation; a late successful result is explicitly shut down.
Primary and switch-candidate preparation both use the same 20-second
wall-clock bound in [`session_setup.rs`](src/app/session_setup.rs). Close,
cancellation, timeout, and exit invalidate preparation and reap its subprocess.
Together these rules keep a cancelled or hung initialize/list task from
repopulating the picker, leaking its child, or displacing a usable session.

Candidate state retains the candidate identity and opaque prepared connection;
it does not separately snapshot the full `NoriConfig`. When the user chooses
New or Resume, the TUI derives activation config from the latest `App` config
and asks the harness to refresh the prepared agent. Session-time settings
therefore include changes made while the picker was open. Agent identity,
working directory, ACP proxy/wire-recording settings, and the resolved default
model must still match preparation because they determine the existing process
or transport; a mismatch tears down the candidate and asks the user to retry
the switch. On `SessionStarted`, only the active-agent identity is committed
into `App`, so other current settings are never overwritten by candidate-era
state.

Agent switching is transactional. Selecting an `/agent` row immediately starts
a live candidate without changing the active `ChatWidget`; the private
`CandidateAgent` state records preparation, picker-ready, and activation
phases. The current and candidate subprocesses coexist while the candidate
initializes and while its session picker is open. Choosing a session activates
the candidate in a separate widget, and `SessionStarted` is the commit event:
only then does `App` swap widgets, persist the selected agent, and shut down the
replaced process. Until that event, positional or deferred input remains owned
by the current widget; after commit it transfers to the candidate and is
submitted once. Preparation failure, activation failure, picker dismissal,
supersession, or application exit tears down only the candidate and leaves the
current session promptable. Prompt submission always targets the current
widget; there is no pending switch-on-next-prompt state.

Bare `/login` has a narrow candidate-target override. Selecting a candidate
sets it, and preparation or activation failure leaves it available so the user
can authenticate that attempted agent without recreating pending-switch state.
Explicit cancellation and successful authentication clear the override; a
successful switch replaces the widget and returns bare `/login` to the active
agent. The override affects login resolution only—prompts and lifecycle actions
continue to target the current session unless a real candidate state exists.

The process-wide `HarnessRemoteHost` follows the same commit boundary. A
candidate widget starts with remote attachment disabled, so a failed or
cancelled candidate cannot displace the current remotely controlled session.
After `SessionStarted`, the committed widget attaches with the observed start
record to seed identity that its new subscription could no longer replay; only
then is the old remote attachment replaced.

An orderly ACP close completes the typed close call, leaves the raw close
response observable on the stream, observes `SessionEnded(Closed)`, and then
handles stream closure. The TUI does not render a successful close-response
message. Explicit application shutdown uses `SessionEnded(Shutdown)`. Local
exit requests immediate owned ACP process-group cleanup; cloud exit allows a
short detach grace. The TUI exits when cleanup publishes `SessionEnded`, rather
than using an independent timer that can abandon reaping.

Events entering the application are tagged with their session generation.
Candidate events are routed to the candidate widget until activation commits;
after a session is replaced, events from older generations are discarded.
Replacement shutdown is based on the live harness handle rather than transcript
recorder or conversation-ID availability. Preparation generations apply the
same stale-result rule before a widget or active session exists.

Unexpected child or transport loss emits a request failure when work was in
flight followed by `SessionEnded(ConnectionLost)`. The TUI stays open so the
user can read the failure and choose the next action; connection loss is not
treated as a successful quit.

Cloud sessions use standard ACP `session/list`, `session/resume`,
`session/load`, and `session/close`. Capabilities describe what an initialized
ACP facade supports; they are not a sound test for whether its process
represents a remote VM. The top-level `nori cloud` launch supplies explicit
`cloud_mode` state through `TuiCli`, `App`, and `ChatWidget`. Cloud entry is
normally picker-first: `App::run` prepares one connection and opens its session
picker before any session directive can claim a VM, with "Start a new session"
as an explicit pick. The prepared child remains alive behind that picker and
the selection consumes it rather than spawning a second agent. The clap-skipped
`cloud_onboard` flag (`nori cloud --onboard`, for customer onboarding) skips
the picker but runs the same bounded connection preparation. The broker projects
the config-pinned onboarding session as `_meta.nori.purpose = "onboarding"`;
when that tag is present, the TUI emits the existing resume action and the
harness uses `session/load`, including recorded-history replay. If no tagged
session is present, or listing is unsupported, onboarding explicitly starts a
new session on that same prepared `cloud-acp --onboard` connection, preserving
the broker's serialized acquire-or-resume behavior and compatibility with older
components. An initialization or advertised-list failure is shown as a
preparation failure rather than being treated as an empty catalog. A
sessionless `/new` consumes that valid prepared onboarding connection; after an
active session, `/new` prepares another connection with the same onboarding
registry entry. `/close` retains its ordinary picker-first lifecycle. Because
the `--onboard` argv is part of the process-wide agent registry entry, every
fallback acquisition remains onboarding-only. While a
picker-first launch waits for a choice, its initial positional prompt and image
attachments remain owned by the deferred widget. Choosing Start new transfers
that input into the replacement widget before the deferred widget shuts down,
so it auto-sends at the same `SessionStarted` boundary as every other entry
path.

The shared ACP resume picker treats session source as first-class presentation
instead of requiring users to inspect raw metadata. Cloud rows with a typed
`_meta.nori.sessionType` are labeled `Slack`, `CLI`, or `Web`; typed internal
sources are excluded from this user-facing picker, while untyped legacy cloud
rows remain available as `Unknown`. When every listed session is cloud-backed,
the table drops local-only working-directory and turn-status columns and gives
the session title most of the row width. The reusable picker renderer follows
the same priority at wide terminal sizes: once its detail pane becomes visible,
the list and detail panes receive a responsive 2:1 share of the available
width rather than fixed list sizing.

That launch-origin state retains the cloud ACP session id for footer and
welcome-card identity, rejects local-only commands, and selects cloud
detach/reattach wording. Reattach copy does not promise whether history is
replayed because that is selected independently from ACP capabilities.
Quitting an attached cloud session detaches through connection teardown;
`/close` is available only in cloud mode and remains gated by the facade's
`session/close` support.

#### Remote ACP transport activation

`--remote <ADDR>` (in [`cli.rs`](src/cli.rs)) serves the running interactive
session as a remote ACP agent over WebSocket, per
`@/docs/specs/remote-acp-transport.md`. A bare port binds loopback; `IP:PORT`
binds that address; a non-loopback bind additionally requires
`--remote-allow-nonloopback` because the surface is unauthenticated.

`run_main` (in [`lib.rs`](src/lib.rs)) parses the bind spec, binds the
`RemoteAcpServer`, and installs the process-global `HarnessRemoteHost` via
`set_active_host` before the app runs, so a controller can connect as soon as
the session launches. The server value is held for the whole app run.

Ordinary harness launches in
[`chatwidget/agent.rs`](src/chatwidget/agent.rs) attach their new
`HarnessHandle` immediately so the remote host subscribes before startup
events. Switch-candidate launches suppress that attachment until their
`SessionStarted` commit, then use the observed start record to attach the
committed handle without losing its outward identity. Thus new-session and
resume paths remain eager, while a failed or cancelled candidate cannot replace
the current remote session. Attaching the committed replacement closes any
current remote controller; after reconnecting, the controller discovers the
replacement through `session/list`. All remote types reach the TUI through
`nori_harness::remote_agent` re-exports, preserving the rule that the TUI never
imports the ACP host crate directly.

While a remote controller drives the session, the TUI stays attached to the
same handle and ordered event stream and renders remote-driven activity as an
observer. The fan-out delivers every event to both consumers: the remote host
forwards a delegated permission request to its controller only when the remote
controller owns the turn, while the TUI continues to observe the same request
on its own stream. Policy for simultaneous local and remote input is
deliberately deferred by the spec.

#### Footer configuration

`[tui.footer_layout]` accepts the same layout entries in `footer_left`,
`footer_right`, and all four `textarea_*` corners. An entry is either a built-in
segment name or an inline custom chunk:

```toml
[tui.footer_layout]
footer_left = [
  "git_branch",
  { format = "{context_used_percent} / {context_window_tokens}" },
  "approval_mode",
]
```

Built-in names are `prompt_summary`, `session_title`, `vim_mode`, `git_branch`,
`worktree_name`, `git_stats`, `context`, `context_used_percent`,
`context_remaining_percent`, `context_used_tokens`,
`context_remaining_tokens`, `context_window_tokens`, `approval_mode`,
`skillset`, `nori_version`, `token_usage`, `mode_indicator`, and
`cloud_session`. The default `context` segment renders used percentage and
maximum window size, such as `44% / 272k`. The five atomic context segments are
off as standalone entries by default so custom chunks can compose only the
values they need.

`session_title` shows the title the agent reports over ACP session-info updates
(`Title: Fix login flakes`). It self-hides for agents that never send one, and
the same value reaches the `title:` row of the `/status` card.

Custom formats are deliberately limited to literal text and built-in
placeholders. `{{` and `}}` emit literal braces. Unknown placeholders,
unbalanced braces, expressions, conditions, and format specifiers are rejected
when configuration loads. Referenced segments keep their styles and ignore
their standalone `[tui.footer_segments]` toggle; if any referenced runtime value
is unavailable, the complete custom chunk is hidden.

`FooterLayoutItem` and the load-time parser live in
`@/nori-rs/nori-config/src/types/mod.rs`. Composition and context formatting
live in `@/nori-rs/tui/src/bottom_pane/footer.rs`; resolved ACP or transcript
usage reaches it through
`@/nori-rs/tui/src/bottom_pane/chat_composer/rendering.rs`.

#### Session-info verbosity

ACP session-info updates carry an agent-defined metadata blob. Rendering all of
it documents what a harness supports and how agents differ, which is worth the
transcript noise while developing against an agent and not otherwise, so the
metadata history cell is limited to unstable builds — debug builds and
`X.Y.Z-next*` prereleases, per `is_unstable_build` in
`@/nori-rs/tui/src/version.rs`.

`SessionInfoDetail` in `@/nori-rs/tui/src/nori/session_info.rs` carries the
decision, and `ChatWidget` holds it as a field so tests can exercise the stable
path. `ErrorsOnly` is not silence: `error.*` assignments still render on every
build, because this cell is their only surface and dropping them would turn an
agent-side failure into a silent stall. Every build also merges every update
into `SessionInfoState`, so the session title keeps reaching the footer and the
`/status` card; only the dump is suppressed.

#### Status card

`/status` renders the bordered session card (`NoriSessionHeaderCell`,
`@/nori-rs/tui/src/nori/session_header/`) as a by-default superset of the
footer's information categories, independent of the user's footer configuration:
directory, session id (the conversation id, shown for every agent — cloud
sessions append the broker title), agent, skillset (with detected skillsets
version), approvals, ACP mode, a git row (branch / worktree / +added −removed /
untracked), instruction files, a single consolidated context row (`% left
(used / window)`), and cumulative token usage. The footer-derived values are
pulled in one shot via `ChatComposer::status_card_info()` (a `StatusCardInfo`
built from `footer_props()`); the aligned row helpers and the git/context
formatting live in `@/nori-rs/tui/src/nori/session_header/status_card.rs`. After a
branch-at-head fork the card also shows a `forked from:` row (the parent
conversation id): the harness emits `NoriEvent::SessionForked` when it forks the
transcript, and `on_session_forked` (`@/nori-rs/tui/src/chatwidget/event_handlers.rs`)
updates `conversation_id`, records `forked_from`, and drops a copy-pasteable
`nori resume <previous>` hint cell so the previous (now frozen) conversation
stays resumable.

#### Transcripts and view-only mode

Between `ReplayStarted` and `ReplayFinished`, replayed user and assistant
messages are assembled in event order and rendered as static conversation
history with turn boundaries. They are not handled as live output streams;
replayed assistant messages use the same raw-Markdown-backed cells as completed
live messages so later width changes use the same rendering path.
View-only rendering recovers initialization identity and replay source from the
stored lifecycle events, then runs raw session-information notifications
through the same private normalizer and renderer as the live TUI.

Transcript schema v3 contains lifecycle events even before
a prompt, so session pickers determine whether a transcript is empty by its
exact user-turn count rather than total record count. The loader retains private
v2 compatibility.

Goodbye-card statistics are accumulated from the TUI's private projection of
raw ACP updates and from Nori lifecycle state. They are presentation data, not
part of the public harness protocol.

### Things to Know

- Never add a public normalized ACP enum to make rendering convenient. Keep
  display inference private in `presentation/`.
- Query results arrive as typed method returns; do not add correlated Nori
  response events.
- The PTY suite in `@/nori-rs/tui-pty-e2e/` is the regression boundary for real
  terminal behavior, event ordering, approvals, lifecycle, and transcript
  selection.
- `nori-config` is the source of approval and sandbox policy; ACP session config
  options remain ACP schema values.
- `-p` belongs to the top-level CLI as `--print`. The TUI's own flag set must not
  claim it, and the legacy Codex `--profile` / `-p` selector stays rejected
  outright.

Created and maintained by Nori.
