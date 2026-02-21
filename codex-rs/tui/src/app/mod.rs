use crate::app_backtrack::BacktrackState;
use crate::app_event::AppEvent;
use crate::app_event_sender::AppEventSender;
use crate::bottom_pane::ApprovalRequest;
use crate::chatwidget::ChatWidget;
use crate::diff_render::DiffSummary;
use crate::exec_command::strip_bash_lc_and_escape;
use crate::file_search::FileSearchManager;
use crate::history_cell::HistoryCell;
use crate::model_migration::ModelMigrationOutcome;
use crate::model_migration::migration_copy_for_config;
use crate::model_migration::run_model_migration_prompt;
use crate::nori::agent_picker::PendingAgentSelection;
use crate::pager_overlay::Overlay;
use crate::render::highlight::highlight_bash_to_lines;
use crate::render::renderable::Renderable;
use crate::resume_picker::ResumeSelection;
use crate::tui;
use crate::tui::TuiEvent;
use crate::update_action::UpdateAction;
use codex_ansi_escape::ansi_escape_line;
use codex_app_server_protocol::AuthMode;
use codex_common::model_presets::HIDE_GPT_5_1_CODEX_MAX_MIGRATION_PROMPT_CONFIG;
use codex_common::model_presets::HIDE_GPT5_1_MIGRATION_PROMPT_CONFIG;
use codex_common::model_presets::ModelUpgrade;
use codex_common::model_presets::all_model_presets;
use codex_core::AuthManager;
use codex_core::ConversationManager;
use codex_core::config::Config;
use codex_core::config::edit::ConfigEditsBuilder;
use codex_core::config::edit::toml_value;
#[cfg(target_os = "windows")]
use codex_core::features::Feature;
use codex_core::model_family::find_family_for_model;
use codex_core::protocol::EventMsg;
use codex_core::protocol::FinalOutput;
use codex_core::protocol::Op;
use codex_core::protocol::SessionSource;
use codex_core::protocol::TokenUsage;
use codex_core::protocol_config_types::ReasoningEffort as ReasoningEffortConfig;
use codex_protocol::ConversationId;
use color_eyre::eyre::Result;
use color_eyre::eyre::WrapErr;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
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

const GPT_5_1_MIGRATION_AUTH_MODES: [AuthMode; 2] = [AuthMode::ChatGPT, AuthMode::ApiKey];
const GPT_5_1_CODEX_MIGRATION_AUTH_MODES: [AuthMode; 1] = [AuthMode::ChatGPT];

#[derive(Debug, Clone)]
pub struct AppExitInfo {
    pub token_usage: TokenUsage,
    pub conversation_id: Option<ConversationId>,
    pub update_action: Option<UpdateAction>,
}

fn session_summary(
    token_usage: TokenUsage,
    conversation_id: Option<ConversationId>,
) -> Option<SessionSummary> {
    if token_usage.is_zero() {
        return None;
    }

    let usage_line = FinalOutput::from(token_usage).to_string();
    let resume_command =
        conversation_id.map(|conversation_id| format!("codex resume {conversation_id}"));
    Some(SessionSummary {
        usage_line,
        resume_command,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SessionSummary {
    usage_line: String,
    resume_command: Option<String>,
}

fn should_show_model_migration_prompt(
    current_model: &str,
    target_model: &str,
    hide_prompt_flag: Option<bool>,
) -> bool {
    if target_model == current_model || hide_prompt_flag.unwrap_or(false) {
        return false;
    }

    all_model_presets()
        .iter()
        .filter(|preset| preset.upgrade.is_some())
        .any(|preset| preset.model == current_model)
}

fn migration_prompt_hidden(config: &Config, migration_config_key: &str) -> Option<bool> {
    match migration_config_key {
        HIDE_GPT_5_1_CODEX_MAX_MIGRATION_PROMPT_CONFIG => {
            config.notices.hide_gpt_5_1_codex_max_migration_prompt
        }
        HIDE_GPT5_1_MIGRATION_PROMPT_CONFIG => config.notices.hide_gpt5_1_migration_prompt,
        _ => None,
    }
}

async fn handle_model_migration_prompt_if_needed(
    tui: &mut tui::Tui,
    config: &mut Config,
    app_event_tx: &AppEventSender,
    auth_mode: Option<AuthMode>,
) -> Option<AppExitInfo> {
    let upgrade = all_model_presets()
        .iter()
        .find(|preset| preset.model == config.model)
        .and_then(|preset| preset.upgrade.as_ref());

    if let Some(ModelUpgrade {
        id: target_model,
        reasoning_effort_mapping,
        migration_config_key,
    }) = upgrade
    {
        if !migration_prompt_allows_auth_mode(auth_mode, migration_config_key) {
            return None;
        }

        let target_model = target_model.to_string();
        let hide_prompt_flag = migration_prompt_hidden(config, migration_config_key);
        if !should_show_model_migration_prompt(&config.model, &target_model, hide_prompt_flag) {
            return None;
        }

        let prompt_copy = migration_copy_for_config(migration_config_key);
        match run_model_migration_prompt(tui, prompt_copy).await {
            ModelMigrationOutcome::Accepted => {
                app_event_tx.send(AppEvent::PersistModelMigrationPromptAcknowledged {
                    migration_config: migration_config_key.to_string(),
                });
                config.model = target_model.to_string();
                if let Some(family) = find_family_for_model(&target_model) {
                    config.model_family = family;
                }

                let mapped_effort = if let Some(reasoning_effort_mapping) = reasoning_effort_mapping
                    && let Some(reasoning_effort) = config.model_reasoning_effort
                {
                    reasoning_effort_mapping
                        .get(&reasoning_effort)
                        .cloned()
                        .or(config.model_reasoning_effort)
                } else {
                    config.model_reasoning_effort
                };

                config.model_reasoning_effort = mapped_effort;

                app_event_tx.send(AppEvent::UpdateAgent(target_model.clone()));
                app_event_tx.send(AppEvent::UpdateReasoningEffort(mapped_effort));
                app_event_tx.send(AppEvent::PersistAgentSelection {
                    agent: target_model.clone(),
                    effort: mapped_effort,
                });
            }
            ModelMigrationOutcome::Rejected => {
                app_event_tx.send(AppEvent::PersistModelMigrationPromptAcknowledged {
                    migration_config: migration_config_key.to_string(),
                });
            }
            ModelMigrationOutcome::Exit => {
                return Some(AppExitInfo {
                    token_usage: TokenUsage::default(),
                    conversation_id: None,
                    update_action: None,
                });
            }
        }
    }

    None
}

pub(crate) struct App {
    pub(crate) server: Arc<ConversationManager>,
    pub(crate) app_event_tx: AppEventSender,
    pub(crate) chat_widget: ChatWidget,
    pub(crate) auth_manager: Arc<AuthManager>,

    /// Config is stored here so we can recreate ChatWidgets as needed.
    pub(crate) config: Config,
    pub(crate) vertical_footer: bool,
    pub(crate) active_profile: Option<String>,

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

    /// Ephemeral per-session loop count override (set via /config menu).
    /// Outer Option: whether overridden; inner Option<i32>: the value.
    #[cfg(feature = "nori-config")]
    loop_count_override: Option<Option<i32>>,

    /// Configurable hotkey bindings loaded from NoriConfig.
    pub(crate) hotkey_config: codex_acp::config::HotkeyConfig,

    /// Vim mode enabled setting loaded from NoriConfig.
    vim_mode_enabled: bool,

    system_info_tx: mpsc::Sender<SystemInfoRefreshRequest>,

    /// Guard to prevent showing the worktree cleanup warning more than once per session.
    worktree_warning_shown: bool,
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
        mut config: Config,
        active_profile: Option<String>,
        initial_prompt: Option<String>,
        initial_images: Vec<PathBuf>,
        resume_selection: ResumeSelection,
        vertical_footer: bool,
    ) -> Result<AppExitInfo> {
        use tokio_stream::StreamExt;

        // Early check: if ACP-only mode is enabled (allow_http_fallback=false) and
        // the model is not registered in the ACP registry, fail immediately.
        // This prevents showing model migration prompts or other UI for HTTP models
        // that will ultimately fail.
        if !config.acp_allow_http_fallback && codex_acp::get_agent_config(&config.model).is_err() {
            return Err(color_eyre::eyre::eyre!(
                "Model '{}' is not registered as an ACP agent. \
                 Set acp.allow_http_fallback = true to allow HTTP providers. \
                 Known ACP models: mock-model, claude, claude-acp, gemini-2.5-flash, gemini-acp",
                config.model
            ));
        }

        let (app_event_tx, mut app_event_rx) = unbounded_channel();
        let app_event_tx = AppEventSender::new(app_event_tx);

        let auth_mode = auth_manager.auth().map(|auth| auth.mode);
        let exit_info =
            handle_model_migration_prompt_if_needed(tui, &mut config, &app_event_tx, auth_mode)
                .await;
        if let Some(exit_info) = exit_info {
            return Ok(exit_info);
        }

        let conversation_manager = Arc::new(ConversationManager::new(
            auth_manager.clone(),
            SessionSource::Cli,
        ));

        let enhanced_keys_supported = tui.enhanced_keys_supported();

        let mut chat_widget = match resume_selection {
            ResumeSelection::StartFresh | ResumeSelection::Exit => {
                let init = crate::chatwidget::ChatWidgetInit {
                    config: config.clone(),
                    frame_requester: tui.frame_requester(),
                    app_event_tx: app_event_tx.clone(),
                    initial_prompt: initial_prompt.clone(),
                    initial_images: initial_images.clone(),
                    enhanced_keys_supported,
                    auth_manager: auth_manager.clone(),
                    vertical_footer,
                    expected_agent: None, // No filtering for fresh sessions
                };
                ChatWidget::new(init, conversation_manager.clone())
            }
            ResumeSelection::Resume(path) => {
                let resumed = conversation_manager
                    .resume_conversation_from_rollout(
                        config.clone(),
                        path.clone(),
                        auth_manager.clone(),
                    )
                    .await
                    .wrap_err_with(|| {
                        format!("Failed to resume session from {}", path.display())
                    })?;
                let init = crate::chatwidget::ChatWidgetInit {
                    config: config.clone(),
                    frame_requester: tui.frame_requester(),
                    app_event_tx: app_event_tx.clone(),
                    initial_prompt: initial_prompt.clone(),
                    initial_images: initial_images.clone(),
                    enhanced_keys_supported,
                    auth_manager: auth_manager.clone(),
                    vertical_footer,
                    expected_agent: None, // No filtering for resumed sessions
                };
                ChatWidget::new_from_existing(
                    init,
                    resumed.conversation,
                    resumed.session_configured,
                )
            }
        };

        chat_widget.maybe_prompt_windows_sandbox_enable();

        let file_search = FileSearchManager::new(config.cwd.clone(), app_event_tx.clone());
        #[cfg(not(debug_assertions))]
        let upgrade_version = crate::updates::get_upgrade_version(&config);

        let (system_info_tx, system_info_rx) = mpsc::channel();
        let _system_info_worker = Self::spawn_system_info_worker(
            system_info_rx,
            app_event_tx.clone(),
            config.cwd.clone(),
            config.model.clone(),
            initial_prompt.clone(),
        );

        let mut app = Self {
            server: conversation_manager,
            app_event_tx,
            chat_widget,
            auth_manager: auth_manager.clone(),
            config,
            vertical_footer,
            active_profile,
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
            #[cfg(feature = "nori-config")]
            loop_count_override: None,
            hotkey_config: codex_acp::config::HotkeyConfig::default(),
            vim_mode_enabled: false,
            system_info_tx,
            worktree_warning_shown: false,
        };

        // Load NoriConfig and propagate settings to the textarea.
        let nori_config = codex_acp::config::NoriConfig::load().unwrap_or_default();
        app.hotkey_config = nori_config.hotkeys;
        app.vim_mode_enabled = nori_config.vim_mode;

        // Propagate initial hotkey config to the textarea so editing bindings
        // (ctrl+a, ctrl+e, etc.) respect user overrides from config.toml.
        app.chat_widget.set_hotkey_config(app.hotkey_config.clone());
        // Propagate initial vim mode setting.
        app.chat_widget.set_vim_mode_enabled(app.vim_mode_enabled);
        // Propagate initial footer segment config.
        for segment in codex_acp::config::FooterSegment::all_variants() {
            app.chat_widget.set_footer_segment_enabled(
                *segment,
                nori_config.footer_segment_config.is_enabled(*segment),
            );
        }

        // If skillset_per_session is enabled and we're in a worktree, check if a
        // skillset is already active. If so, load it; otherwise show the picker.
        #[cfg(feature = "nori-config")]
        if nori_config.skillset_per_session {
            let is_in_worktree =
                crate::system_info::extract_worktree_name(&app.config.cwd).is_some();
            if is_in_worktree {
                // Check if .nori-config.json already has an activeSkillset
                let existing_skillset = app
                    .config
                    .cwd
                    .join(".nori-config.json")
                    .exists()
                    .then(|| {
                        std::fs::read_to_string(app.config.cwd.join(".nori-config.json"))
                            .ok()
                            .and_then(|c| serde_json::from_str::<serde_json::Value>(&c).ok())
                            .and_then(|j| {
                                j.get("activeSkillset")
                                    .and_then(|v| v.as_str())
                                    .map(String::from)
                            })
                    })
                    .flatten();

                if let Some(name) = existing_skillset {
                    app.chat_widget.set_session_skillset_name(Some(name));
                } else {
                    app.chat_widget.handle_switch_skillset_command();
                }
            }
        }

        // On startup, if Agent mode (workspace-write) or ReadOnly is active, warn about world-writable dirs on Windows.
        #[cfg(target_os = "windows")]
        {
            let should_check = codex_core::get_platform_sandbox().is_some()
                && matches!(
                    app.config.sandbox_policy,
                    codex_core::protocol::SandboxPolicy::WorkspaceWrite { .. }
                        | codex_core::protocol::SandboxPolicy::ReadOnly
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
                let logs_base_dir = app.config.codex_home.clone();
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
            Some(app.config.model.clone()),
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
            update_action: app.pending_update_action,
        })
    }

    fn reasoning_label(reasoning_effort: Option<ReasoningEffortConfig>) -> &'static str {
        match reasoning_effort {
            Some(ReasoningEffortConfig::Minimal) => "minimal",
            Some(ReasoningEffortConfig::Low) => "low",
            Some(ReasoningEffortConfig::Medium) => "medium",
            Some(ReasoningEffortConfig::High) => "high",
            Some(ReasoningEffortConfig::XHigh) => "xhigh",
            None | Some(ReasoningEffortConfig::None) => "default",
        }
    }

    pub(crate) fn token_usage(&self) -> codex_core::protocol::TokenUsage {
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
        initial_dir: PathBuf,
        initial_model: String,
        initial_first_message: Option<String>,
    ) -> thread::JoinHandle<()> {
        thread::spawn(move || {
            let mut last_request = SystemInfoRefreshRequest {
                dir: initial_dir,
                model: Some(initial_model),
                first_message: initial_first_message,
            };
            loop {
                match system_info_rx.recv_timeout(Duration::from_secs(5)) {
                    Ok(request) => last_request = request,
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }

                let agent_kind = last_request
                    .model
                    .as_ref()
                    .and_then(|model| codex_acp::AgentKind::from_slug(model));
                let info = crate::system_info::SystemInfo::collect_for_directory_with_message(
                    &last_request.dir,
                    agent_kind,
                    last_request.first_message.as_deref(),
                );
                app_event_tx.send(AppEvent::SystemInfoRefreshed(info));
            }
        })
    }

    fn on_update_reasoning_effort(&mut self, effort: Option<ReasoningEffortConfig>) {
        self.chat_widget.set_reasoning_effort(effort);
        self.config.model_reasoning_effort = effort;
    }
}

fn migration_prompt_allowed_auth_modes(migration_config_key: &str) -> Option<&'static [AuthMode]> {
    match migration_config_key {
        HIDE_GPT5_1_MIGRATION_PROMPT_CONFIG => Some(&GPT_5_1_MIGRATION_AUTH_MODES),
        HIDE_GPT_5_1_CODEX_MAX_MIGRATION_PROMPT_CONFIG => Some(&GPT_5_1_CODEX_MIGRATION_AUTH_MODES),
        _ => None,
    }
}

fn migration_prompt_allows_auth_mode(
    auth_mode: Option<AuthMode>,
    migration_config_key: &str,
) -> bool {
    if let Some(allowed_modes) = migration_prompt_allowed_auth_modes(migration_config_key) {
        match auth_mode {
            None => true,
            Some(mode) => allowed_modes.contains(&mode),
        }
    } else {
        auth_mode != Some(AuthMode::ApiKey)
    }
}

#[cfg(test)]
mod tests;
