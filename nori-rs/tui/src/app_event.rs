use std::path::PathBuf;

use codex_common::approval_presets::ApprovalPreset;
use codex_file_search::FileMatch;
use nori_protocol::acp::v1::SessionConfigOption;

use crate::bottom_pane::ApprovalRequest;
use crate::history_cell::HistoryCell;
use crate::nori::session_config_mode::AcpModeConfig;
use crate::system_info::SystemInfo;

use nori_config::AskForApproval;
use nori_config::SandboxPolicy;

pub(crate) type SessionGeneration = i64;

#[derive(Debug, Clone)]
pub(crate) struct ConversationPathResponseEvent {
    pub(crate) conversation_id: nori_harness::ConversationId,
}

#[derive(Debug)]
pub(crate) enum HarnessAction {
    Cancel,
    Shutdown {
        child_grace: std::time::Duration,
    },
    Compact,
    Branch,
    UndoTo(i64),
    LoadUndoSnapshots,
    RunUserShell(String),
    AddHistory(String),
    HistoryEntry {
        log_id: i64,
        offset: i64,
    },
    SearchHistory {
        max_results: i64,
    },
    LoadCustomPrompts,
    LoadGoal,
    SetGoal {
        objective: String,
        status: Option<nori_protocol::ThreadGoalStatus>,
    },
    SetGoalStatus(nori_protocol::ThreadGoalStatus),
    ClearGoal,
    RespondToAgent {
        request_id: nori_protocol::acp::v1::RequestId,
        response:
            Box<Result<nori_protocol::acp::v1::ClientResponse, nori_protocol::acp::v1::Error>>,
    },
}

#[allow(clippy::large_enum_variant)]
#[derive(Debug)]
pub(crate) enum AppEvent {
    SessionEvent {
        generation: SessionGeneration,
        event: nori_protocol::SessionEvent,
    },

    /// Start a new session.
    NewSession,

    /// A `/close` failed: surface the error and unblock the widget.
    SessionCloseFailed {
        message: String,
    },

    /// A `/close` succeeded: the session is released. Return to the agent
    /// session picker instead of auto-claiming a fresh session.
    SessionClosed,

    /// The pre-session agent probe finished (picker-first entry and
    /// post-/close). `fallback_to_spawn` says what a failure should do:
    /// entry falls back to the plain spawn (the old `nori cloud` behavior);
    /// post-/close must NOT — the user just released a session, so silently
    /// claiming a fresh one is forbidden.
    AgentSessionListProbed {
        probe: Result<nori_harness::AgentSessionsProbe, nori_harness::ProbeError>,
        intent: AgentSessionProbeIntent,
    },

    /// Re-run the pre-session probe and reopen the session picker (e.g.
    /// /resume on a deferred widget that has no live agent connection).
    OpenAgentSessionPicker,

    /// Begin the quit flow (feedback, input freeze, bounded cleanup) on the chat
    /// widget — used by input surfaces that don't own the widget (Ctrl+D).
    BeginExit,

    /// Request to exit the application gracefully.
    ExitRequest,

    HarnessAction(HarnessAction),
    HistoryEntryLoaded {
        log_id: i64,
        offset: i64,
        entry: Option<nori_harness::HistoryEntry>,
    },
    HistorySearchLoaded(Vec<nori_harness::HistoryEntry>),
    CustomPromptsLoaded(Vec<nori_harness::CustomPrompt>),
    UndoSnapshotsLoaded(Vec<nori_harness::UndoSnapshot>),
    GoalLoaded(Option<nori_protocol::ThreadGoal>),
    HarnessActionFailed(String),

    /// Kick off an asynchronous file search for the given query (text after
    /// the `@`). Previous searches may be cancelled by the app layer so there
    /// is at most one in-flight search.
    StartFileSearch(String),

    /// Result of a completed asynchronous file search. The `query` echoes the
    /// original search term so the UI can decide whether the results are
    /// still relevant.
    FileSearchResult {
        query: String,
        matches: Vec<FileMatch>,
    },

    /// Result of background system info collection for the footer.
    SystemInfoRefreshed(SystemInfo),

    /// Request to refresh system info for a specific directory.
    /// This is triggered when the effective CWD changes during agent operations.
    ///
    /// The optional agent name is used to determine which agent's transcripts to search for.
    RefreshSystemInfoForDirectory {
        /// The directory to collect system info for
        dir: PathBuf,
        /// Optional agent name (e.g., "claude-code", "gemini") to determine agent kind
        agent: Option<String>,
    },

    /// Result of computing a `/diff` command.
    DiffResult(String),

    InsertHistoryCell(Box<dyn HistoryCell>),

    /// Replace the just-finished assistant stream cells with one raw-Markdown cell.
    ConsolidateAgentMessage {
        source: String,
        cwd: PathBuf,
    },

    StartCommitAnimation,
    StopCommitAnimation,
    CommitTick,

    /// Open the confirmation prompt before enabling full access mode.
    OpenFullAccessConfirmation {
        preset: ApprovalPreset,
    },

    /// Open the Windows world-writable directories warning.
    /// If `preset` is `Some`, the confirmation will apply the provided
    /// approval/sandbox configuration on Continue; if `None`, it performs no
    /// policy change and only acknowledges/dismisses the warning.
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    OpenWorldWritableWarningConfirmation {
        preset: Option<ApprovalPreset>,
        /// Up to 3 sample world-writable directories to display in the warning.
        sample_paths: Vec<String>,
        /// If there are more than `sample_paths`, this carries the remaining count.
        extra_count: usize,
        /// True when the scan failed (e.g. ACL query error) and protections could not be verified.
        failed_scan: bool,
    },

    /// Prompt to enable the Windows sandbox feature before using Agent mode.
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    OpenWindowsSandboxEnablePrompt {
        preset: ApprovalPreset,
    },

    /// Enable the Windows sandbox feature and switch to Agent mode.
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    EnableWindowsSandboxForAgentMode {
        preset: ApprovalPreset,
    },

    /// Apply an approval preset atomically across app state, widget state,
    /// and the backend's turn context.
    ApplyApprovalPreset {
        approval: AskForApproval,
        sandbox: SandboxPolicy,
    },

    /// Update whether the full access warning prompt has been acknowledged.
    UpdateFullAccessWarningAcknowledged(bool),

    /// Update whether the world-writable directories warning has been acknowledged.
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    UpdateWorldWritableWarningAcknowledged(bool),

    /// Persist the acknowledgement flag for the full access warning prompt.
    PersistFullAccessWarningAcknowledged,

    /// Persist the acknowledgement flag for the world-writable directories warning.
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    PersistWorldWritableWarningAcknowledged,

    /// Skip the next world-writable scan (one-shot) after a user-confirmed continue.
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    SkipNextWorldWritableScan,

    /// Re-open the approval presets popup.
    OpenApprovalsPopup,

    /// Forwarded conversation history snapshot from the current conversation.
    ConversationHistory(ConversationPathResponseEvent),

    /// Open the approval popup.
    FullScreenApprovalRequest(ApprovalRequest),

    /// Set a pending agent selection. The agent switch will happen on the next
    /// prompt submission to avoid disrupting active prompt turns.
    SetPendingAgent {
        /// The agent name of the selected agent (e.g., "mock-model", "gemini-2.5-flash")
        agent_name: String,
        /// The display name for the status indicator
        display_name: String,
    },

    /// Submit a message with a pending agent switch. The agent will be switched
    /// first, then the message will be submitted to the new agent.
    SubmitWithAgentSwitch {
        /// The agent name of the agent to switch to
        agent_name: String,
        /// The display name for the status indicator
        display_name: String,
        /// The user message text to submit after switching
        message_text: String,
        /// Optional image paths to include with the message
        image_paths: Vec<PathBuf>,
    },

    /// Agent failed to spawn (ACP or HTTP backend). Show error and prompt user
    /// to select a different agent.
    AgentSpawnFailed {
        /// The agent name of the agent that failed to spawn
        agent_name: String,
        /// The error message describing the failure
        error: String,
    },

    /// Agent is connecting (spawning subprocess). Show "Connecting to [Agent]" status.
    /// Sent before AcpBackend::spawn() and cleared when SessionConfigured is received.
    AgentConnecting {
        /// The display name of the agent being connected to
        display_name: String,
    },

    /// Open the model picker placeholder shown when the active agent does not
    /// expose a Model session config option.
    OpenAcpModelPickerUnsupported,

    /// Open the generic ACP session config picker. `focus_config_id` optionally
    /// names the option whose row should be selected when the panel opens (used
    /// to return the cursor to the just-edited option).
    OpenAcpSessionConfigPicker {
        config_options: Vec<SessionConfigOption>,
        focus_config_id: Option<String>,
    },

    /// Open the value picker for a specific ACP session config option.
    OpenAcpSessionConfigValuePicker {
        option: SessionConfigOption,
    },

    /// Set an ACP session config option value.
    SetAcpSessionConfigOption {
        config_id: String,
        value: String,
        option_name: String,
        value_name: String,
    },

    /// Result of setting an ACP session config option.
    AcpSessionConfigSetResult {
        success: bool,
        /// Agent slug active when the option was set; selections are persisted
        /// under this key even if the user switches agents mid-flight.
        agent: String,
        config_id: String,
        value: String,
        option_name: String,
        value_name: String,
        config_options: Option<Vec<SessionConfigOption>>,
        error: Option<String>,
    },

    /// Latest raw ACP session config for silently seeding local UI state.
    AcpSessionConfigSnapshot {
        generation: i64,
        config_options: Vec<SessionConfigOption>,
    },

    /// Latest derived ACP mode config for the active session.
    AcpModeConfigSnapshot {
        generation: i64,
        mode: Option<AcpModeConfig>,
    },

    /// Result of OAuth login flow completion.
    LoginComplete {
        /// Whether the login was successful
        success: bool,
    },

    /// Output from external CLI login process (e.g., gemini login)
    ExternalCliLoginOutput {
        /// Raw output string from the CLI (ANSI codes stripped)
        data: String,
    },

    /// External CLI login process completed
    ExternalCliLoginComplete {
        /// Whether the process exited successfully (exit code 0)
        success: bool,
        /// The agent name for display purposes
        agent_name: String,
    },

    /// Set the TUI vertical footer config setting.
    SetConfigVerticalFooter(bool),

    /// Set the TUI terminal notifications config setting.
    SetConfigTerminalNotifications(bool),

    /// Set a hotkey binding for a specific action.
    SetConfigHotkey {
        action: nori_config::HotkeyAction,
        binding: nori_config::HotkeyBinding,
    },

    /// Set the TUI OS notifications config setting.
    SetConfigOsNotifications(bool),

    /// Open the vim mode sub-picker.
    OpenVimModePicker,

    /// Set the TUI vim mode config setting. `from_settings` is true when the
    /// change originated from the `/settings` panel (so the panel should be
    /// reopened afterward) and false when it came from the standalone `/vim`
    /// command.
    SetConfigVimMode {
        value: nori_config::VimEnterBehavior,
        from_settings: bool,
    },

    /// Open the notify-after-idle sub-picker.
    OpenNotifyAfterIdlePicker,

    /// Open the script timeout sub-picker.
    OpenScriptTimeoutPicker,

    /// Open the hotkey picker sub-view.
    OpenHotkeyPicker,

    /// Set the TUI notify-after-idle config setting.
    SetConfigNotifyAfterIdle(nori_config::NotifyAfterIdle),

    /// Set the TUI script timeout config setting.
    SetConfigScriptTimeout(nori_config::ScriptTimeout),

    /// Open the loop count sub-picker.
    OpenLoopCountPicker,

    /// Set the loop count config setting. `None` means disabled.
    SetConfigLoopCount(Option<i32>),

    /// Open the auto worktree sub-picker.
    OpenAutoWorktreePicker,

    /// Set the TUI auto worktree config setting.
    SetConfigAutoWorktree(nori_config::AutoWorktree),

    /// Set the TUI skillset per session config setting.
    SetConfigSkillsetPerSession(bool),

    /// Set the TUI pinned plan drawer config setting.
    SetConfigPinnedPlanDrawer(bool),

    /// Set width-resize transcript reflow.
    SetConfigResizeReflow(bool),

    /// Set ACP wire JSONL recording for future ACP child subprocesses.
    SetConfigAcpWireRecording(bool),

    /// Set the TUI custom working messages config setting.
    SetConfigCustomWorkingMessages(bool),

    /// Open the worktree choice modal when enabling per-session skillsets.
    OpenSkillsetPerSessionWorktreeChoice,

    /// Open the footer segments sub-picker.
    OpenFooterSegmentsPicker,

    /// Toggle a footer segment's enabled state.
    SetConfigFooterSegment(nori_config::FooterSegment, bool),

    /// Start the next loop iteration with a fresh conversation.
    /// Sent by ChatWidget::on_task_complete when loop mode is active.
    LoopIteration {
        /// The prompt text to replay.
        prompt: String,
        /// Remaining iterations after this one.
        remaining: i32,
        /// Total iterations configured.
        total: i32,
    },

    /// Result of listing available skillsets via nori-skillsets CLI.
    SkillsetListResult {
        /// List of skillset names on success (exit code 0), None if command not found.
        names: Option<Vec<String>>,
        /// Error message if command failed (non-zero exit) or not found.
        error: Option<String>,
        /// When in a worktree, the directory to install skillsets into.
        install_dir: Option<PathBuf>,
    },

    /// Request to install a skillset by name.
    InstallSkillset {
        /// The name of the skillset to install.
        name: String,
    },

    /// Request to switch to a skillset in a specific directory.
    SwitchSkillset {
        /// The name of the skillset to switch to.
        name: String,
        /// The directory to install the skillset into.
        install_dir: PathBuf,
    },

    /// Result of installing a skillset.
    SkillsetInstallResult {
        /// The name of the skillset that was installed.
        name: String,
        /// Whether the installation succeeded (exit code 0).
        success: bool,
        /// Filtered install output on success, or error message on failure.
        message: String,
    },

    /// Result of switching a skillset.
    SkillsetSwitchResult {
        /// The name of the skillset that was switched to.
        name: String,
        /// Whether the switch succeeded.
        success: bool,
        /// Filtered output on success, or error message on failure.
        message: String,
    },

    /// The skillset picker was dismissed without selection. When agent spawn was
    /// deferred for skillset_per_session, this triggers spawning the agent
    /// without a skillset (behaves as if the feature is disabled).
    SkillsetPickerDismissed,

    /// Execute a custom prompt script asynchronously.
    ExecuteScript {
        /// The custom prompt to execute.
        prompt: nori_harness::custom_prompts::CustomPrompt,
        /// Positional arguments from the command line.
        args: Vec<String>,
    },

    /// Result of executing a custom prompt script.
    ScriptExecutionComplete {
        /// Name of the script that was executed.
        name: String,
        /// Ok(stdout) on success, Err(message) on failure.
        result: Result<String, String>,
    },

    /// Show the viewonly session picker with loaded sessions.
    ShowViewonlySessionPicker {
        /// The loaded session metadata for the picker
        sessions: Vec<crate::nori::viewonly_session_picker::SessionPickerInfo>,
        /// The NORI_HOME path for loading transcripts
        nori_home: PathBuf,
    },

    /// Load and display a transcript in view-only mode.
    LoadViewonlyTranscript {
        /// The NORI_HOME path
        nori_home: PathBuf,
        /// Project identifier
        project_id: String,
        /// Session identifier
        session_id: String,
    },

    /// Display a loaded transcript in the history view.
    DisplayViewonlyTranscript {
        /// The transcript entries to display
        entries: Vec<crate::viewonly_transcript::ViewonlyEntry>,
    },

    /// Show the resume session picker with loaded sessions.
    ShowResumeSessionPicker {
        /// The loaded session metadata for the picker
        sessions: Vec<crate::nori::viewonly_session_picker::SessionPickerInfo>,
        /// The NORI_HOME path
        nori_home: PathBuf,
        /// Monotonic generation for ignoring stale lazy summary updates.
        generation: u64,
    },

    /// Update one row in the active resume picker after lazy transcript scanning.
    ResumeSessionSummaryReady {
        /// Monotonic generation for ignoring stale lazy summary updates.
        generation: u64,
        /// Session identifier to update.
        session_id: String,
        /// Session start timestamp for rebuilding the row name.
        started_at: String,
        /// Preview of the first user message, if discovered.
        first_message_preview: Option<String>,
        /// Exact number of user turns, once known.
        user_turn_count: Option<usize>,
    },

    /// Resume a previous session via ACP session/load or client-side replay.
    ResumeSession {
        /// The NORI_HOME path
        nori_home: PathBuf,
        /// Project identifier (needed to load transcript)
        project_id: String,
        /// Session identifier to resume
        session_id: String,
    },

    /// Show the resume session picker sourced from the live agent's ACP
    /// `session/list` rather than the local transcript store.
    ShowAcpResumeSessionPicker {
        /// Schema-native session records reported by the agent.
        sessions: Vec<nori_protocol::acp::v1::SessionInfo>,
    },

    /// Resume a session reported by the agent's `session/list`, via
    /// `session/load` with no local transcript (the agent replays history).
    ResumeAcpSession {
        /// The agent's session identifier to load.
        acp_session_id: String,
        /// The broker-reported session title, when known, so the reattach
        /// message and cloud surfaces can name the session.
        title: Option<String>,
    },

    /// Launch a terminal file manager to browse and optionally edit files.
    BrowseFiles(nori_config::FileManager),

    /// Set the configured file manager for the `/browse` command.
    SetConfigFileManager(nori_config::FileManager),

    /// Open the file manager sub-picker.
    OpenFileManagerPicker,

    /// Persist the full MCP servers map to config.toml.
    SaveMcpServers(std::collections::BTreeMap<String, nori_config::McpServerConfig>),

    /// Trigger an MCP OAuth login flow for a server.
    McpOAuthLogin {
        server_name: String,
        server_url: String,
        http_headers: Option<std::collections::HashMap<String, String>>,
        env_http_headers: Option<std::collections::HashMap<String, String>>,
        /// Pre-registered OAuth client ID (for servers without dynamic registration).
        client_id: Option<String>,
        /// Environment variable name holding the OAuth client secret.
        client_secret_env_var: Option<String>,
    },

    /// Cancel an in-progress MCP OAuth login flow.
    McpOAuthLoginCancel {
        server_name: String,
    },

    /// MCP OAuth login flow completed (success or failure).
    McpOAuthLoginComplete {
        server_name: String,
        success: bool,
        error: Option<String>,
    },

    /// Request async computation of MCP server auth statuses.
    ComputeMcpAuthStatuses,

    /// Deliver computed MCP auth statuses to the active picker view.
    McpAuthStatusesReady(std::collections::HashMap<String, codex_rmcp_client::McpAuthStatus>),

    /// Browser launched successfully with CDP details.
    BrowserLaunched {
        ws_url: String,
        cdp_port: i32,
    },

    /// Browser launch failed with an error message.
    BrowserLaunchFailed(String),

    /// Persist the chosen `/browser` profile as the default and launch with it.
    SetBrowserProfile(nori_config::BrowserProfileMode),

    /// Open the fork picker modal showing previous user messages.
    OpenForkPicker,

    /// Branch the conversation at its current head via ACP `session/fork`,
    /// swapping the active session to the forked one.
    BranchFromCurrent,

    /// Fork the conversation to just before the selected user message.
    ForkToMessage {
        /// Index of the target user message cell in `transcript_cells`.
        cell_index: usize,
        /// The text of the selected message, to prefill the composer.
        prefill: String,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AgentSessionProbeIntent {
    Picker { fallback_to_spawn: bool },
    Onboarding,
}

impl AgentSessionProbeIntent {
    pub(crate) fn fallback_to_spawn(self) -> bool {
        match self {
            Self::Picker { fallback_to_spawn } => fallback_to_spawn,
            Self::Onboarding => true,
        }
    }
}
