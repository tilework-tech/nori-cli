use std::path::PathBuf;

use codex_common::approval_presets::ApprovalPreset;
use codex_common::model_presets::ModelPreset;
use codex_core::protocol::ConversationPathResponseEvent;
use codex_core::protocol::Event;
use codex_core::protocol::RateLimitSnapshot;
use codex_file_search::FileMatch;

use crate::bottom_pane::ApprovalRequest;
use crate::history_cell::HistoryCell;
use crate::system_info::SystemInfo;

use codex_core::protocol::AskForApproval;
use codex_core::protocol::SandboxPolicy;
use codex_core::protocol_config_types::ReasoningEffort;

/// Information about an available ACP model.
#[cfg(feature = "unstable")]
#[derive(Debug, Clone)]
pub(crate) struct AcpModelInfo {
    /// The model ID (used for switching)
    pub model_id: String,
    /// Human-readable display name
    pub display_name: String,
    /// Optional description
    pub description: Option<String>,
}

#[allow(clippy::large_enum_variant)]
#[derive(Debug)]
pub(crate) enum AppEvent {
    CodexEvent(Event),

    /// Start a new session.
    NewSession,

    /// Request to exit the application gracefully.
    ExitRequest,

    /// Forward an `Op` to the Agent. Using an `AppEvent` for this avoids
    /// bubbling channels through layers of widgets.
    CodexOp(codex_core::protocol::Op),

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
    /// The optional model name is used to determine which agent's transcripts to search for.
    RefreshSystemInfoForDirectory {
        /// The directory to collect system info for
        dir: PathBuf,
        /// Optional model name (e.g., "claude-code", "gemini") to determine agent kind
        model: Option<String>,
    },

    /// Result of refreshing rate limits
    #[allow(dead_code)]
    RateLimitSnapshotFetched(RateLimitSnapshot),

    /// Result of computing a `/diff` command.
    DiffResult(String),

    InsertHistoryCell(Box<dyn HistoryCell>),

    StartCommitAnimation,
    StopCommitAnimation,
    CommitTick,

    /// Update the current reasoning effort in the running app and widget.
    UpdateReasoningEffort(Option<ReasoningEffort>),

    /// Update the current model slug in the running app and widget.
    UpdateModel(String),

    /// Persist the selected model and reasoning effort to the appropriate config.
    PersistModelSelection {
        model: String,
        effort: Option<ReasoningEffort>,
    },

    /// Open the reasoning selection popup after picking a model.
    OpenReasoningPopup {
        model: ModelPreset,
    },

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

    /// Update the current approval policy in the running app and widget.
    UpdateAskForApprovalPolicy(AskForApproval),

    /// Update the current sandbox policy in the running app and widget.
    UpdateSandboxPolicy(SandboxPolicy),

    /// Update whether the full access warning prompt has been acknowledged.
    UpdateFullAccessWarningAcknowledged(bool),

    /// Update whether the world-writable directories warning has been acknowledged.
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    UpdateWorldWritableWarningAcknowledged(bool),

    /// Update whether the rate limit switch prompt has been acknowledged for the session.
    UpdateRateLimitSwitchPromptHidden(bool),

    /// Persist the acknowledgement flag for the full access warning prompt.
    PersistFullAccessWarningAcknowledged,

    /// Persist the acknowledgement flag for the world-writable directories warning.
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    PersistWorldWritableWarningAcknowledged,

    /// Persist the acknowledgement flag for the rate limit switch prompt.
    PersistRateLimitSwitchPromptHidden,

    /// Persist the acknowledgement flag for the model migration prompt.
    PersistModelMigrationPromptAcknowledged {
        migration_config: String,
    },

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
        /// The model name of the selected agent (e.g., "mock-model", "gemini-2.5-flash")
        model_name: String,
        /// The display name for the status indicator
        display_name: String,
    },

    /// Submit a message with a pending agent switch. The agent will be switched
    /// first, then the message will be submitted to the new agent.
    SubmitWithAgentSwitch {
        /// The model name of the agent to switch to
        model_name: String,
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
        /// The model name of the agent that failed to spawn
        model_name: String,
        /// The error message describing the failure
        error: String,
    },

    /// Agent is connecting (spawning subprocess). Show "Connecting to [Agent]" status.
    /// Sent before AcpBackend::spawn() and cleared when SessionConfigured is received.
    AgentConnecting {
        /// The display name of the agent being connected to
        display_name: String,
    },

    /// Open the ACP model picker popup with available models from the agent.
    #[cfg(feature = "unstable")]
    OpenAcpModelPicker {
        /// Available models from the ACP agent
        models: Vec<AcpModelInfo>,
        /// Currently selected model ID
        current_model_id: Option<String>,
    },

    /// Set the active model in the ACP agent.
    #[cfg(feature = "unstable")]
    SetAcpModel {
        /// The model ID to switch to
        model_id: String,
        /// The display name for UI feedback
        display_name: String,
    },

    /// Result of setting the ACP model.
    #[cfg(feature = "unstable")]
    AcpModelSetResult {
        /// Whether the model was set successfully
        success: bool,
        /// The model that was set (on success) or attempted (on failure).
        /// Kept for logging/debugging even though not currently used in UI.
        #[allow(dead_code)]
        model_id: String,
        /// The display name for UI feedback
        display_name: String,
        /// Error message on failure
        error: Option<String>,
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
        action: codex_acp::config::HotkeyAction,
        binding: codex_acp::config::HotkeyBinding,
    },

    /// Set the TUI OS notifications config setting.
    SetConfigOsNotifications(bool),

    /// Set the TUI vim mode config setting.
    SetConfigVimMode(bool),

    /// Open the notify-after-idle sub-picker.
    #[cfg(feature = "nori-config")]
    OpenNotifyAfterIdlePicker,

    /// Open the script timeout sub-picker.
    #[cfg(feature = "nori-config")]
    OpenScriptTimeoutPicker,

    /// Open the hotkey picker sub-view.
    OpenHotkeyPicker,

    /// Set the TUI notify-after-idle config setting.
    #[cfg(feature = "nori-config")]
    SetConfigNotifyAfterIdle(codex_acp::config::NotifyAfterIdle),

    /// Set the TUI script timeout config setting.
    #[cfg(feature = "nori-config")]
    SetConfigScriptTimeout(codex_acp::config::ScriptTimeout),

    /// Open the loop count sub-picker.
    #[cfg(feature = "nori-config")]
    OpenLoopCountPicker,

    /// Set the loop count config setting. `None` means disabled.
    #[cfg(feature = "nori-config")]
    SetConfigLoopCount(Option<i32>),

    /// Start the next loop iteration with a fresh conversation.
    /// Sent by ChatWidget::on_task_complete when loop mode is active.
    #[cfg(feature = "nori-config")]
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
    },

    /// Request to install a skillset by name.
    InstallSkillset {
        /// The name of the skillset to install.
        name: String,
    },

    /// Result of installing a skillset.
    SkillsetInstallResult {
        /// The name of the skillset that was installed.
        name: String,
        /// Whether the installation succeeded (exit code 0).
        success: bool,
        /// First line of stdout on success, or error message on failure.
        message: String,
    },

    /// Execute a custom prompt script asynchronously.
    ExecuteScript {
        /// The custom prompt to execute.
        prompt: codex_protocol::custom_prompts::CustomPrompt,
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
}
