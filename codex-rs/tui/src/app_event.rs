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
    RefreshSystemInfoForDirectory(PathBuf),

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

    /// Open the branch picker option from the review popup.
    OpenReviewBranchPicker(PathBuf),

    /// Open the commit picker option from the review popup.
    OpenReviewCommitPicker(PathBuf),

    /// Open the custom prompt option from the review popup.
    OpenReviewCustomPrompt,

    /// Open the approval popup.
    FullScreenApprovalRequest(ApprovalRequest),

    /// Open the feedback note entry overlay after the user selects a category.
    #[cfg(feature = "feedback")]
    OpenFeedbackNote {
        category: FeedbackCategory,
        include_logs: bool,
    },

    /// Open the upload consent popup for feedback after selecting a category.
    #[cfg(feature = "feedback")]
    OpenFeedbackConsent {
        category: FeedbackCategory,
    },

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
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FeedbackCategory {
    BadResult,
    GoodResult,
    Bug,
    Other,
}
