use crate::app_backtrack::BacktrackState;
use crate::app_event::AppEvent;
use crate::app_event_sender::AppEventSender;
use crate::bottom_pane::ApprovalRequest;
use crate::chatwidget::ChatWidget;
use crate::client_event_format;
use crate::client_tool_cell;
use crate::diff_render::DiffSummary;
use crate::exec_command::strip_bash_lc_and_escape;
use crate::file_search::FileSearchManager;
use crate::history_cell::HistoryCell;
use crate::nori::agent_picker::PendingAgentSelection;
use crate::pager_overlay::Overlay;
use crate::render::highlight::highlight_bash_to_lines;
use crate::render::renderable::Renderable;
use crate::resume_picker::ResumeSelection;
use crate::tui;
use crate::tui::TuiEvent;
use crate::update_action::UpdateAction;
use codex_ansi_escape::ansi_escape_line;
use codex_login::AuthManager;
use codex_protocol::ConversationId;
use codex_protocol::protocol::AskForApproval;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::FinalOutput;
use codex_protocol::protocol::Op;
use codex_protocol::protocol::SandboxPolicy;
use codex_protocol::protocol::TokenUsage;
use color_eyre::eyre::Result;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use nori_config::NoriConfig;
use nori_config::NoriConfigEdits as ConfigEditsBuilder;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Wrap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;
use tokio::select;
use tokio::sync::mpsc::unbounded_channel;

#[cfg(not(debug_assertions))]
use crate::history_cell::UpdateAvailableHistoryCell;

pub const RESUME_HINT_LEAD: &str = "To continue this session, run:";

#[derive(Debug, Clone)]
pub struct AppExitInfo {
    pub token_usage: TokenUsage,
    pub conversation_id: Option<ConversationId>,
    pub conversation_has_activity: bool,
    pub update_action: Option<UpdateAction>,
}

fn session_summary(
    token_usage: TokenUsage,
    conversation_id: Option<ConversationId>,
    conversation_has_activity: bool,
) -> Option<SessionSummary> {
    let usage_line = (!token_usage.is_zero()).then(|| FinalOutput::from(token_usage).to_string());
    let resume_command = conversation_id
        .filter(|_| conversation_has_activity)
        .map(|conversation_id| resume_command_for_conversation(&conversation_id));

    if usage_line.is_none() && resume_command.is_none() {
        return None;
    }

    Some(SessionSummary {
        usage_line,
        resume_command,
    })
}

pub fn resume_command_for_conversation(conversation_id: &ConversationId) -> String {
    format!("nori resume {conversation_id}")
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SessionSummary {
    usage_line: Option<String>,
    resume_command: Option<String>,
}

pub(crate) struct App {
    pub(crate) app_event_tx: AppEventSender,
    pub(crate) chat_widget: ChatWidget,
    pub(crate) auth_manager: Arc<AuthManager>,

    /// Config is stored here so we can recreate ChatWidgets as needed.
    pub(crate) config: NoriConfig,
    pub(crate) vertical_footer: bool,
    pub(crate) footer_layout_config: nori_config::FooterLayoutConfig,

    pub(crate) file_search: FileSearchManager,

    pub(crate) transcript_cells: Vec<Arc<dyn HistoryCell>>,

    // Pager overlay state (Transcript or Static like Diff)
    pub(crate) overlay: Option<Overlay>,
    pub(crate) deferred_history_lines: Vec<Line<'static>>,
    has_emitted_history_lines: bool,

    pub(crate) enhanced_keys_supported: bool,

    /// Controls the animation thread that sends CommitTick events.
    pub(crate) commit_anim_running: Arc<AtomicBool>,

    // Esc-backtracking state grouped
    pub(crate) backtrack: crate::app_backtrack::BacktrackState,
    /// Set when the user confirms an update; propagated on exit.
    pub(crate) pending_update_action: Option<UpdateAction>,

    /// Ignore the next ShutdownComplete event when we're intentionally
    /// stopping a conversation (e.g., before starting a new one).
    suppress_shutdown_complete: bool,

    // One-shot suppression of the next world-writable scan after user confirmation.
    skip_world_writable_scan_once: bool,

    /// Pending agent selection. When set, the agent will switch on the next
    /// prompt submission. This avoids disrupting active prompt turns.
    pending_agent: Option<PendingAgentSelection>,

    /// Ephemeral per-session loop count override (set via /settings menu).
    /// Outer Option: whether overridden; inner Option<i32>: the value.
    loop_count_override: Option<Option<i32>>,

    /// Configurable hotkey bindings loaded from NoriConfig.
    pub(crate) hotkey_config: nori_config::HotkeyConfig,

    /// Vim mode and Enter key behavior loaded from NoriConfig.
    vim_mode: nori_config::VimEnterBehavior,

    /// Current footer segment visibility loaded from NoriConfig.
    footer_segment_config: nori_config::FooterSegmentConfig,

    /// Plan drawer visibility mode.
    plan_drawer_mode: crate::chatwidget::PlanDrawerMode,

    system_info_tx: mpsc::Sender<SystemInfoRefreshRequest>,

    /// Guard to prevent showing the worktree cleanup warning more than once per session.
    worktree_warning_shown: bool,

    /// True when the initial agent spawn was deferred (waiting for a skillset
    /// switch). Cleared on the first successful skillset switch or picker
    /// dismissal. Guards against re-spawning the agent on later switches.
    deferred_spawn_pending: bool,
    /// True while the pre-session agent probe (session/list) is running —
    /// guards against concurrent probes and against skillset events
    /// resolving the deferred spawn out from under the picker flow.
    agent_session_probe_in_flight: bool,

    /// Cancel sender for an in-progress MCP OAuth login flow.
    mcp_oauth_cancel_tx: Option<tokio::sync::oneshot::Sender<()>>,
}

#[derive(Clone, Debug)]
struct SystemInfoRefreshRequest {
    dir: PathBuf,
    model: Option<String>,
    first_message: Option<String>,
}

mod config_persistence;
mod event_handling;
mod session_setup;

impl App {
    #[allow(clippy::too_many_arguments)]
    pub async fn run(
        tui: &mut tui::Tui,
        auth_manager: Arc<AuthManager>,
        config: NoriConfig,
        initial_prompt: Option<String>,
        initial_images: Vec<PathBuf>,
        resume_selection: ResumeSelection,
        vertical_footer: bool,
        cloud_session_picker: bool,
    ) -> Result<AppExitInfo> {
        use tokio_stream::StreamExt;

        let (app_event_tx, mut app_event_rx) = unbounded_channel();
        let app_event_tx = AppEventSender::new(app_event_tx);

        let enhanced_keys_supported = tui.enhanced_keys_supported();

        // When skillset_per_session is enabled, defer spawning the agent until
        // after the user picks a skillset and the switch writes
        // `.claude/CLAUDE.md` to disk. If the user dismisses the picker, the
        // agent spawns without a skillset.
        let needs_deferred_spawn = cloud_session_picker || config.skillset_per_session;
        let mut chat_widget = {
            let init = crate::chatwidget::ChatWidgetInit {
                config: config.clone(),
                frame_requester: tui.frame_requester(),
                app_event_tx: app_event_tx.clone(),
                initial_prompt: initial_prompt.clone(),
                initial_images: initial_images.clone(),
                enhanced_keys_supported,
                auth_manager: auth_manager.clone(),
                vertical_footer,
                footer_segment_config: config.footer_segment_config.clone(),
                footer_layout_config: config.footer_layout_config.clone(),
                expected_agent: None,
                deferred_spawn: needs_deferred_spawn,
                fork_context: None,
            };
            match resume_selection {
                ResumeSelection::Resume(target) => {
                    let loader = nori_harness::transcript::TranscriptLoader::new(target.nori_home);
                    let transcript = loader
                        .load_transcript(&target.project_id, &target.session_id)
                        .await?;
                    let acp_session_id = transcript.meta.acp_session_id.clone();
                    ChatWidget::new_resumed_acp(init, acp_session_id, None, Some(transcript))
                }
                ResumeSelection::StartFresh | ResumeSelection::Exit => ChatWidget::new(init),
            }
        };

        chat_widget.maybe_prompt_windows_sandbox_enable();

        let file_search = FileSearchManager::new(config.cwd.clone(), app_event_tx.clone());
        #[cfg(not(debug_assertions))]
        let upgrade_version = crate::updates::get_upgrade_version(&config);

        let (system_info_tx, system_info_rx) = mpsc::channel();
        let _system_info_worker =
            Self::spawn_system_info_worker(system_info_rx, app_event_tx.clone());
        let footer_segment_config = config.footer_segment_config.clone();
        let footer_layout_config = config.footer_layout_config.clone();

        let mut app = Self {
            app_event_tx,
            chat_widget,
            auth_manager: auth_manager.clone(),
            config,
            vertical_footer,
            file_search,
            enhanced_keys_supported,
            transcript_cells: Vec::new(),
            overlay: None,
            deferred_history_lines: Vec::new(),
            has_emitted_history_lines: false,
            commit_anim_running: Arc::new(AtomicBool::new(false)),
            backtrack: BacktrackState::default(),
            pending_update_action: None,
            suppress_shutdown_complete: false,
            skip_world_writable_scan_once: false,
            pending_agent: None,
            loop_count_override: None,
            hotkey_config: nori_config::HotkeyConfig::default(),
            vim_mode: nori_config::VimEnterBehavior::Off,
            footer_segment_config,
            footer_layout_config,
            plan_drawer_mode: crate::chatwidget::PlanDrawerMode::Off,
            system_info_tx,
            worktree_warning_shown: false,
            deferred_spawn_pending: needs_deferred_spawn,
            agent_session_probe_in_flight: false,
            mcp_oauth_cancel_tx: None,
        };

        // Propagate NoriConfig settings to the textarea.
        app.hotkey_config = app.config.hotkeys.clone();
        app.vim_mode = app.config.vim_mode;

        // Propagate initial hotkey config to the textarea so editing bindings
        // (ctrl+a, ctrl+e, etc.) respect user overrides from config.toml.
        app.chat_widget.set_hotkey_config(app.hotkey_config.clone());
        // Propagate initial vim mode setting.
        app.chat_widget.set_vim_mode(app.vim_mode);
        // Propagate initial pinned plan drawer setting.
        let plan_mode = if app.config.pinned_plan_drawer {
            crate::chatwidget::PlanDrawerMode::Expanded
        } else {
            crate::chatwidget::PlanDrawerMode::Off
        };
        app.plan_drawer_mode = plan_mode;
        app.chat_widget.set_plan_drawer_mode(plan_mode);

        // If skillset_per_session is enabled, show the skillset picker. The
        // agent spawn was deferred so that `nori-skillsets switch` can write
        // `.claude/CLAUDE.md` before the agent reads it. Once the user picks a
        // skillset and the switch completes, `event_handling.rs` triggers
        // `spawn_deferred_agent()`. If the user dismisses the picker, the
        // `SkillsetPickerDismissed` event triggers the deferred spawn without a
        // skillset.
        if cloud_session_picker {
            // Picker-first entry: list live sessions before anything can
            // claim one; "start new" is an explicit row in the picker.
            app.begin_agent_session_picker(true);
        } else if app.config.skillset_per_session {
            app.chat_widget.handle_switch_skillset_command();
        }

        // On startup, if Agent mode (workspace-write) or ReadOnly is active, warn about world-writable dirs on Windows.
        #[cfg(target_os = "windows")]
        {
            let should_check = codex_sandbox::get_platform_sandbox().is_some()
                && matches!(
                    app.config.sandbox_policy,
                    codex_protocol::protocol::SandboxPolicy::WorkspaceWrite { .. }
                        | codex_protocol::protocol::SandboxPolicy::ReadOnly
                )
                && !app
                    .config
                    .notices
                    .hide_world_writable_warning
                    .unwrap_or(false);
            if should_check {
                let cwd = app.config.cwd.clone();
                let env_map: std::collections::HashMap<String, String> = std::env::vars().collect();
                let tx = app.app_event_tx.clone();
                let logs_base_dir = app.config.nori_home.clone();
                let sandbox_policy = app.config.sandbox_policy.clone();
                Self::spawn_world_writable_scan(cwd, env_map, logs_base_dir, sandbox_policy, tx);
            }
        }

        #[cfg(not(debug_assertions))]
        if let Some(latest_version) = upgrade_version {
            app.handle_event(
                tui,
                AppEvent::InsertHistoryCell(Box::new(UpdateAvailableHistoryCell::new(
                    latest_version,
                    crate::update_action::get_update_action(),
                ))),
            )
            .await?;
        }

        let tui_events = tui.event_stream();
        tokio::pin!(tui_events);

        app.request_system_info_refresh(
            app.config.cwd.clone(),
            Some(app.config.active_agent.clone()),
            app.chat_widget.first_prompt_text(),
        );

        tui.frame_requester().schedule_frame();

        while select! {
            Some(event) = app_event_rx.recv() => {
                app.handle_event(tui, event).await?
            }
            Some(event) = tui_events.next() => {
                app.handle_tui_event(tui, event).await?
            }
        } {}

        // Don't clear terminal to allow exit message to remain visible
        // tui.terminal.clear()?;

        Ok(AppExitInfo {
            token_usage: app.token_usage(),
            conversation_id: app.chat_widget.conversation_id(),
            conversation_has_activity: app.chat_widget.session_stats().has_activity(),
            update_action: app.pending_update_action,
        })
    }

    pub(super) fn chat_widget_init(
        &self,
        frame_requester: crate::tui::FrameRequester,
        initial_prompt: Option<String>,
        initial_images: Vec<PathBuf>,
        expected_agent: Option<String>,
        deferred_spawn: bool,
        fork_context: Option<String>,
    ) -> crate::chatwidget::ChatWidgetInit {
        crate::chatwidget::ChatWidgetInit {
            config: self.config.clone(),
            frame_requester,
            app_event_tx: self.app_event_tx.clone(),
            initial_prompt,
            initial_images,
            enhanced_keys_supported: self.enhanced_keys_supported,
            auth_manager: self.auth_manager.clone(),
            vertical_footer: self.vertical_footer,
            footer_segment_config: self.footer_segment_config.clone(),
            footer_layout_config: self.footer_layout_config.clone(),
            expected_agent,
            deferred_spawn,
            fork_context,
        }
    }

    pub(super) fn configure_new_chat_widget(&mut self) {
        self.chat_widget
            .set_hotkey_config(self.hotkey_config.clone());
        self.chat_widget.set_vim_mode(self.vim_mode);
        self.chat_widget.set_plan_drawer_mode(self.plan_drawer_mode);
        self.chat_widget
            .set_loop_count_override(self.loop_count_override);
    }

    pub(crate) fn token_usage(&self) -> codex_protocol::protocol::TokenUsage {
        self.chat_widget.token_usage()
    }

    fn request_system_info_refresh(
        &self,
        dir: PathBuf,
        model: Option<String>,
        first_message: Option<String>,
    ) {
        let request = SystemInfoRefreshRequest {
            dir,
            model,
            first_message,
        };
        if self.system_info_tx.send(request).is_err() {
            tracing::error!("system info refresh channel is closed");
        }
    }

    fn spawn_system_info_worker(
        system_info_rx: mpsc::Receiver<SystemInfoRefreshRequest>,
        app_event_tx: AppEventSender,
    ) -> thread::JoinHandle<()> {
        thread::spawn(move || {
            // Refresh only when a request arrives. The initial refresh is
            // scheduled explicitly via `request_system_info_refresh` at
            // startup, and subsequent refreshes are driven by user actions
            // (message submit, task completion, tool-call cwd changes,
            // skillset install/switch). No periodic polling.
            while let Ok(request) = system_info_rx.recv() {
                let agent_kind = request
                    .model
                    .as_ref()
                    .and_then(|model| nori_harness::AgentKind::from_slug(model));
                let info = crate::system_info::SystemInfo::collect_for_directory_with_message(
                    &request.dir,
                    agent_kind,
                    request.first_message.as_deref(),
                );
                app_event_tx.send(AppEvent::SystemInfoRefreshed(info));
            }
        })
    }
}

#[cfg(test)]
mod tests;
