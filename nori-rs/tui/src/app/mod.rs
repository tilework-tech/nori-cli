use crate::app_backtrack::BacktrackState;
use crate::app_event::AppEvent;
use crate::app_event_sender::AppEventSender;
use crate::bottom_pane::ApprovalRequest;
use crate::chatwidget::ChatWidget;
use crate::client_event_format;
use crate::client_tool_cell;
use crate::diff_render::DiffSummary;
use crate::file_search::FileSearchManager;
use crate::history_cell::HistoryCell;
use crate::pager_overlay::Overlay;
use crate::render::renderable::Renderable;
use crate::resume_picker::ResumeSelection;
use crate::tui;
use crate::tui::TuiEvent;
use crate::ui_types::TokenUsage;
use crate::update_action::UpdateAction;
use codex_ansi_escape::ansi_escape_line;
use codex_login::AuthManager;
use color_eyre::eyre::Result;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use nori_config::AskForApproval;
use nori_config::NoriConfig;
use nori_config::NoriConfigEdits as ConfigEditsBuilder;
use nori_config::SandboxPolicy;
use nori_harness::ConversationId;
use ratatui::style::Stylize;
use ratatui::text::Line;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;
use tokio::sync::mpsc::unbounded_channel;

#[cfg(not(debug_assertions))]
use crate::history_cell::UpdateAvailableHistoryCell;

#[derive(Debug, Clone)]
pub struct AppExitInfo {
    pub token_usage: TokenUsage,
    pub conversation_id: Option<ConversationId>,
    pub conversation_has_activity: bool,
    pub update_action: Option<UpdateAction>,
}

pub fn resume_command_for_conversation(conversation_id: &ConversationId) -> String {
    format!("nori resume {conversation_id}")
}

pub(crate) struct App {
    pub(crate) app_event_tx: AppEventSender,
    pub(crate) chat_widget: ChatWidget,
    pub(crate) auth_manager: Arc<AuthManager>,
    /// Registry of every session this process runs. The active widget's
    /// session and a hidden `/agent` switch candidate are both entries.
    session_manager: Option<nori_harness::session_manager::SessionManager>,
    /// Where the host this terminal owns reads the terminal's current config.
    /// A joined host has none.
    foreground_config: Option<nori_host_runtime::ForegroundConfig>,
    /// The sessions of a joined host this terminal shows. None for a host
    /// this terminal owns.
    joined: Option<joined_sessions::JoinedSessions>,
    /// While `/new` waits for the host, the session to bring back if the
    /// host refuses.
    new_session_return: Option<crate::app_event::SessionGeneration>,

    /// Config is stored here so we can recreate ChatWidgets as needed.
    pub(crate) config: NoriConfig,
    pub(crate) vertical_footer: bool,
    pub(crate) cloud_mode: bool,
    pub(crate) analytics: Option<nori_installed::AnalyticsReporter>,
    pub(crate) footer_layout_config: nori_config::FooterLayoutConfig,

    pub(crate) file_search: FileSearchManager,
    file_mentions: file_mentions::FileMentions,

    pub(crate) transcript_cells: transcript_cells::TranscriptCells,

    /// Every session this TUI holds, in navigation order. Sessions that are
    /// not on screen are parked here with their own widget and transcript.
    session_deck: session_deck::SessionDeck<Box<ChatWidget>>,
    adopted_delegations: std::collections::HashSet<ConversationId>,
    /// The active session's transcript, taken when that session is about to be
    /// parked behind a new one.
    retiring_transcript: Option<Vec<Arc<dyn HistoryCell>>>,
    /// The active session changed and the terminal must be reprinted on the
    /// next draw. Several jumps before a draw reprint once.
    session_reprint_pending: bool,
    /// Sessions that finished a turn while off screen and were not viewed.
    unseen_finishes: crate::session_status::UnseenFinishes,
    /// What each session is doing, for the session list.
    session_previews: crate::session_preview::SessionPreviews,
    /// Agent display names for the session list, looked up once per agent.
    agent_display_names: std::cell::RefCell<std::collections::HashMap<String, String>>,

    // Pager overlay state (Transcript or Static like Diff)
    pub(crate) overlay: Option<Overlay>,
    /// Inline or full-screen view, and the history held while the screen is
    /// covered by the full-screen view or an overlay.
    pub(crate) screen: screen_mode::ScreenMode,
    has_emitted_history_lines: bool,
    pub(crate) transcript_reflow: crate::transcript_reflow::TranscriptReflowState,

    pub(crate) enhanced_keys_supported: bool,

    /// Controls the animation thread that sends CommitTick events.
    pub(crate) commit_anim_running: Arc<AtomicBool>,

    // Esc-backtracking state grouped
    pub(crate) backtrack: crate::app_backtrack::BacktrackState,
    /// Set when the user confirms an update; propagated on exit.
    pub(crate) pending_update_action: Option<UpdateAction>,

    // One-shot suppression of the next world-writable scan after user confirmation.
    skip_world_writable_scan_once: bool,

    /// Prepared startup/post-close connection waiting for an explicit session choice.
    prepared_agent: Option<nori_harness::runtime::PreparedAgent>,
    /// Fork/backtrack context captured with the prepared connection and reused
    /// when refreshing session-time configuration before activation.
    prepared_agent_initial_context: Option<String>,

    /// Owned startup/post-close preparation. Its generation prevents a late
    /// result from replacing a newer session decision.
    primary_agent_preparation: Option<PrimaryAgentPreparation>,

    /// A session directive chosen while the startup connection is still being
    /// prepared. The directive consumes that exact connection when ready.
    pub(crate) pending_session_activation: Option<PendingSessionActivation>,

    /// Live agent-switch candidate. The active widget remains usable until an
    /// activating candidate publishes `SessionStarted`.
    candidate_agent: Option<CandidateAgent>,
    /// Whether the welcome card for the current conversation has already been
    /// written, ahead of the session that conversation will run on.
    welcome_card_written: bool,
    /// The welcome card while the default session starts; see `startup_header`.
    startup_header: Option<startup_header::StartupHeader>,

    /// Ephemeral per-session loop count override (set via /settings menu).
    /// Outer Option: whether overridden; inner Option<i32>: the value.
    loop_count_override: Option<Option<i32>>,

    /// Configurable hotkey bindings loaded from NoriConfig.
    pub(crate) hotkey_config: nori_config::HotkeyConfig,

    /// Vim mode and Enter key behavior loaded from NoriConfig.
    vim_mode: nori_config::VimEnterBehavior,

    /// Current footer segment visibility loaded from NoriConfig.
    footer_segment_config: nori_config::FooterSegmentConfig,

    /// Writes the foreground session's status to the terminal when
    /// `[tui] program_status` is on.
    program_status: Option<crate::program_status::ProgramStatusReporter>,

    /// Plan drawer visibility mode.
    plan_drawer_mode: crate::chatwidget::PlanDrawerMode,

    system_info_tx: mpsc::Sender<SystemInfoRefreshRequest>,

    /// Guard to prevent showing the worktree cleanup warning more than once per session.
    worktree_warning_shown: bool,

    /// True when the initial agent spawn was deferred (waiting for a skillset
    /// switch). Cleared on the first successful skillset switch or picker
    /// dismissal. Guards against re-spawning the agent on later switches.
    deferred_spawn_pending: bool,
    /// Cancel sender for an in-progress MCP OAuth login flow.
    mcp_oauth_cancel_tx: Option<tokio::sync::oneshot::Sender<()>>,
}

struct PrimaryAgentPreparation {
    generation: crate::app_event::SessionGeneration,
    abort: tokio::task::AbortHandle,
    intent: crate::app_event::AgentPrepareIntent,
    initial_context: Option<String>,
}

/// Where a session about to be prepared comes from when it forks an earlier
/// point of another session.
#[derive(Clone, Default)]
pub(crate) struct ForkOrigin {
    /// Conversation history up to the fork point, injected into the first
    /// prompt.
    pub(crate) context: Option<String>,
    /// The session being forked.
    pub(crate) parent: Option<nori_harness::ConversationId>,
}

pub(crate) enum PendingSessionActivation {
    New,
    Resume {
        acp_session_id: Option<String>,
        title: Option<String>,
        transcript: Option<Box<nori_harness::transcript::Transcript>>,
    },
}

enum CandidateAgent {
    Preparing {
        generation: crate::app_event::SessionGeneration,
        agent_name: String,
        purpose: host_candidate::PickerPurpose,
    },
    Prepared {
        agent_name: String,
        display_name: String,
        selection: host_candidate::HostSelection,
        purpose: host_candidate::PickerPurpose,
    },
    /// The host is starting the picked session; no widget exists until it
    /// answers with the live ID.
    Requested {
        generation: crate::app_event::SessionGeneration,
        agent_name: String,
        display_name: String,
        saved: Option<host_candidate::SavedSession>,
        purpose: host_candidate::PickerPurpose,
    },
    Activating {
        agent_name: String,
        display_name: String,
        widget: Box<ChatWidget>,
        activation_error: Option<String>,
        /// The candidate resumes an existing session and replays its history.
        resumes: bool,
        purpose: host_candidate::PickerPurpose,
    },
}

#[derive(Clone, Debug)]
struct SystemInfoRefreshRequest {
    dir: PathBuf,
    model: Option<String>,
    first_message: Option<String>,
    /// `[features] skillsets_v2`.
    skillsets_v2: bool,
}

mod auth_preference;
mod config_persistence;
mod delegation;
mod event_handling;
mod file_mentions;
mod full_screen_frame;
mod host_candidate;
mod host_link;
pub(crate) use host_link::ConnectionLost;
#[cfg(test)]
mod host_link_tests;
mod host_updates;
mod joined_feed;
#[cfg(test)]
mod joined_feed_tests;
mod joined_pager;
mod joined_sessions;
#[cfg(test)]
mod joined_sessions_tests;
mod new_session_folder;
mod program_status;
mod resize_reflow;
mod run_loop;
mod screen_mode;
mod session_deck;
mod session_keys;
mod session_lifecycle;
mod session_overview;
mod session_resume;
mod session_setup;
mod session_view;
mod startup_header;
mod thought_setting;
mod transcript_cells;
pub(crate) use host_candidate::PreparedCatalog;
#[cfg(test)]
pub(crate) use session_view::route_session_history_event;

impl App {
    /// Runs the TUI on `host`, which this terminal owns: the host stops when
    /// the TUI returns, on every path.
    #[allow(clippy::too_many_arguments)]
    pub async fn run(
        tui: &mut tui::Tui,
        auth_manager: Arc<AuthManager>,
        config: NoriConfig,
        initial_prompt: Option<String>,
        initial_images: Vec<PathBuf>,
        resume_selection: ResumeSelection,
        vertical_footer: bool,
        cloud_mode: bool,
        cloud_onboard: bool,
        analytics: Option<nori_installed::AnalyticsReporter>,
        host: crate::OwnedHost,
    ) -> Result<AppExitInfo> {
        // Sessions belong to the host the CLI built for this run.
        let (session_manager, foreground_config, mut host) = host_link::HostLink::start(host);
        let ran = match host.initialize() {
            Ok(()) => {
                Self::run_on_host(
                    tui,
                    auth_manager,
                    config,
                    initial_prompt,
                    initial_images,
                    resume_selection,
                    vertical_footer,
                    cloud_mode,
                    cloud_onboard,
                    analytics,
                    session_manager,
                    foreground_config,
                    &mut host,
                )
                .await
            }
            Err(error) => Err(color_eyre::eyre::eyre!(error)),
        };
        let stopped = host.stop_or_leave().await;
        let exit = ran?;
        stopped.map_err(|error| color_eyre::eyre::eyre!(error))?;
        Ok(exit)
    }

    #[allow(clippy::too_many_arguments)]
    async fn run_on_host(
        tui: &mut tui::Tui,
        auth_manager: Arc<AuthManager>,
        config: NoriConfig,
        initial_prompt: Option<String>,
        initial_images: Vec<PathBuf>,
        resume_selection: ResumeSelection,
        vertical_footer: bool,
        cloud_mode: bool,
        cloud_onboard: bool,
        analytics: Option<nori_installed::AnalyticsReporter>,
        session_manager: nori_harness::session_manager::SessionManager,
        foreground_config: nori_host_runtime::ForegroundConfig,
        host: &mut host_link::HostLink,
    ) -> Result<AppExitInfo> {
        let (app_event_tx, app_event_rx) = unbounded_channel();
        let app_event_tx = AppEventSender::new(app_event_tx);

        let enhanced_keys_supported = tui.enhanced_keys_supported();

        // Every subprocess starts in the prepared state. Session activation is
        // a later user/session decision, after any per-session skillset setup.
        let needs_deferred_spawn = true;
        let mut pending_session_activation = None;
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
                cloud_mode,
                deferred_spawn: needs_deferred_spawn,
                fork_context: None,
                prepared_agent: None,
                analytics: analytics.clone(),
                session_manager: Some(session_manager.clone()),
                clock: crate::answer_guard::Clock::system(),
            };
            match resume_selection {
                ResumeSelection::Resume(target) => {
                    let loader = nori_harness::transcript::TranscriptLoader::new(target.nori_home);
                    let transcript = loader
                        .load_transcript(&target.project_id, &target.session_id)
                        .await?;
                    let acp_session_id = transcript.meta.acp_session_id.clone();
                    pending_session_activation = Some(PendingSessionActivation::Resume {
                        acp_session_id,
                        title: None,
                        transcript: Some(Box::new(transcript)),
                    });
                    ChatWidget::new(init)
                }
                ResumeSelection::StartFresh | ResumeSelection::Exit => ChatWidget::new(init),
            }
        };

        chat_widget.maybe_prompt_windows_sandbox_enable();

        #[cfg(not(debug_assertions))]
        let upgrade_version = crate::updates::get_upgrade_version(&config);
        let skillset_per_session = !cloud_mode && config.skillset_per_session;
        let mut app = Self::assemble(
            tui.enhanced_keys_supported(),
            app_event_tx,
            chat_widget,
            auth_manager,
            config,
            Some(session_manager),
            Some(foreground_config),
            vertical_footer,
            cloud_mode,
            analytics,
        );
        app.pending_session_activation = pending_session_activation;
        app.deferred_spawn_pending = skillset_per_session;

        // Write the welcome card before the agent session exists, so a session
        // that has not been prompted yet still says where it is running and
        // which agent it will run. Two entries opt out. A cloud session's card
        // names the broker session, which it only learns once attached. And a
        // pending resume replaces this widget before the user can read
        // anything, so its card belongs to the widget that takes over.
        if !cloud_mode && app.pending_session_activation.is_none() {
            let face = if app.config.animations {
                use rand::Rng;
                crate::nori::session_header::StartupFace::new(
                    std::time::Instant::now(),
                    rand::rng().random_range(1..=3),
                    crate::version::CODEX_CLI_VERSION,
                )
            } else {
                crate::nori::session_header::StartupFace::settled(crate::version::CODEX_CLI_VERSION)
            };
            if let Some(card) = app.chat_widget.take_welcome_card(face.clone()) {
                app.startup_header = Some(startup_header::StartupHeader::new(Arc::new(card), face));
            }
            app.welcome_card_written = true;
        }

        // If skillset_per_session is enabled, show the skillset picker. The
        // agent preparation was deferred so that `nori-skillsets switch` can
        // write `.claude/CLAUDE.md` before the agent reads it. Applying or
        // dismissing the picker starts preparation without activating a
        // session.
        if skillset_per_session {
            app.chat_widget.handle_switch_skillset_command();
        } else if !cloud_mode && app.pending_session_activation.is_none() {
            // Plain local startup activates the session on its own so
            // session-scoped features (model/config pickers, prompt history)
            // work before the first prompt. The host this terminal owns
            // prepares the agent and starts the session; a positional prompt
            // waits in the composer's held input for it. The welcome card
            // above already ran, so it still shows meanwhile.
            app.begin_host_startup();
        } else if app.pending_session_activation.is_some() {
            app.begin_agent_preparation(crate::app_event::AgentPrepareIntent::Idle);
        } else {
            debug_assert!(cloud_mode);
            if cloud_onboard {
                app.begin_agent_preparation(crate::app_event::AgentPrepareIntent::Onboarding);
            } else {
                // Picker-first entry: list live sessions before anything can
                // claim one; "start new" is an explicit row in the picker.
                app.begin_agent_preparation(crate::app_event::AgentPrepareIntent::Picker {
                    fallback_to_spawn: true,
                });
            }
        }

        // On startup, if Agent mode (workspace-write) or ReadOnly is active, warn about world-writable dirs on Windows.
        #[cfg(target_os = "windows")]
        {
            let should_check = codex_sandbox::get_platform_sandbox().is_some()
                && matches!(
                    app.config.sandbox_policy,
                    nori_config::SandboxPolicy::WorkspaceWrite { .. }
                        | nori_config::SandboxPolicy::ReadOnly
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
                host,
            )
            .await?;
        }

        app.drive(tui, app_event_rx, host).await
    }

    /// The App before it runs, with the settings every entry shares.
    #[allow(clippy::too_many_arguments)]
    fn assemble(
        enhanced_keys_supported: bool,
        app_event_tx: AppEventSender,
        chat_widget: ChatWidget,
        auth_manager: Arc<AuthManager>,
        config: NoriConfig,
        session_manager: Option<nori_harness::session_manager::SessionManager>,
        foreground_config: Option<nori_host_runtime::ForegroundConfig>,
        vertical_footer: bool,
        cloud_mode: bool,
        analytics: Option<nori_installed::AnalyticsReporter>,
    ) -> Self {
        let file_search = FileSearchManager::new(config.cwd.clone(), app_event_tx.clone());
        let (system_info_tx, system_info_rx) = mpsc::channel();
        let _system_info_worker =
            Self::spawn_system_info_worker(system_info_rx, app_event_tx.clone());
        let footer_segment_config = config.footer_segment_config.clone();
        let footer_layout_config = config.footer_layout_config.clone();
        let program_status = crate::program_status::ProgramStatusReporter::from_config(&config);

        let mut app = Self {
            app_event_tx,
            chat_widget,
            auth_manager,
            session_manager,
            foreground_config,
            joined: None,
            new_session_return: None,
            config,
            vertical_footer,
            cloud_mode,
            analytics,
            file_search,
            file_mentions: Default::default(),
            enhanced_keys_supported,
            transcript_cells: Default::default(),
            session_deck: Default::default(),
            adopted_delegations: Default::default(),
            retiring_transcript: None,
            session_reprint_pending: false,
            unseen_finishes: Default::default(),
            session_previews: Default::default(),
            agent_display_names: Default::default(),
            overlay: None,
            screen: Default::default(),
            has_emitted_history_lines: false,
            transcript_reflow: Default::default(),
            commit_anim_running: Arc::new(AtomicBool::new(false)),
            backtrack: BacktrackState::default(),
            pending_update_action: None,
            skip_world_writable_scan_once: false,
            prepared_agent: None,
            prepared_agent_initial_context: None,
            primary_agent_preparation: None,
            pending_session_activation: None,
            candidate_agent: None,
            welcome_card_written: false,
            startup_header: None,
            loop_count_override: None,
            hotkey_config: nori_config::HotkeyConfig::default(),
            vim_mode: nori_config::VimEnterBehavior::Off,
            footer_segment_config,
            program_status,
            footer_layout_config,
            plan_drawer_mode: crate::chatwidget::PlanDrawerMode::Off,
            system_info_tx,
            worktree_warning_shown: false,
            deferred_spawn_pending: false,
            mcp_oauth_cancel_tx: None,
        };

        app.session_deck.add(app.chat_widget.session_generation());

        // Propagate NoriConfig settings to the textarea.
        app.hotkey_config = app.config.hotkeys.clone();
        app.vim_mode = app.config.vim_mode;

        // Propagate initial hotkey config to the textarea so editing bindings
        // (ctrl+a, ctrl+e, etc.) respect user overrides from config.toml.
        app.chat_widget.set_hotkey_config(app.hotkey_config.clone());
        app.chat_widget.update_approval_mode_label();
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

        // Fill the footer with the git facts we can read in a few milliseconds.
        // The background refresh below replaces this with the full snapshot,
        // but it spawns Node for the skillset version and walks the filesystem
        // for transcripts, so without a seed every footer segment stays blank
        // for as long as that takes.
        app.chat_widget.seed_system_info();
        app
    }

    pub(super) fn chat_widget_init(
        &self,
        frame_requester: crate::tui::FrameRequester,
        initial_prompt: Option<String>,
        initial_images: Vec<PathBuf>,
        _expected_agent: Option<String>,
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
            cloud_mode: self.cloud_mode,
            deferred_spawn,
            fork_context,
            prepared_agent: None,
            analytics: self.analytics.clone(),
            session_manager: self.session_manager.clone(),
            clock: crate::answer_guard::Clock::system(),
        }
    }

    pub(super) fn configure_new_chat_widget(&mut self) {
        // Lazy activation swaps in a fresh widget the first time the user
        // submits a prompt. That widget continues the conversation the startup
        // card already described, so it must not write a second one. Taking the
        // flag scopes this to that one handover: a later `/new` is a different
        // conversation and writes its own card when its session starts.
        if std::mem::take(&mut self.welcome_card_written) {
            self.chat_widget.suppress_welcome_card();
        }
        // The replacement carries its own empty footer, so re-seed it rather
        // than leaving every segment blank until the next background refresh.
        self.chat_widget.seed_system_info();
        self.chat_widget.update_approval_mode_label();
        self.chat_widget
            .set_hotkey_config(self.hotkey_config.clone());
        self.chat_widget.set_vim_mode(self.vim_mode);
        self.chat_widget.set_plan_drawer_mode(self.plan_drawer_mode);
        self.chat_widget
            .set_loop_count_override(self.loop_count_override);
    }

    pub(crate) fn token_usage(&self) -> TokenUsage {
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
            skillsets_v2: self.config.skillsets_v2,
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
                    request.skillsets_v2,
                );
                app_event_tx.send(AppEvent::SystemInfoRefreshed(info));
            }
        })
    }
}
