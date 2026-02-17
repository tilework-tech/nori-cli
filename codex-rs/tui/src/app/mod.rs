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

impl App {
    async fn shutdown_current_conversation(&mut self) {
        if let Some(conversation_id) = self.chat_widget.conversation_id() {
            self.suppress_shutdown_complete = true;
            self.chat_widget.submit_op(Op::Shutdown);
            self.server.remove_conversation(&conversation_id).await;
        }
    }

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

    pub(crate) async fn handle_tui_event(
        &mut self,
        tui: &mut tui::Tui,
        event: TuiEvent,
    ) -> Result<bool> {
        if self.overlay.is_some() {
            let _ = self.handle_backtrack_overlay_event(tui, event).await?;
        } else {
            match event {
                TuiEvent::Key(key_event) => {
                    self.handle_key_event(tui, key_event).await;
                }
                TuiEvent::Paste(pasted) => {
                    // Many terminals convert newlines to \r when pasting (e.g., iTerm2),
                    // but tui-textarea expects \n. Normalize CR to LF.
                    // [tui-textarea]: https://github.com/rhysd/tui-textarea/blob/4d18622eeac13b309e0ff6a55a46ac6706da68cf/src/textarea.rs#L782-L783
                    // [iTerm2]: https://github.com/gnachman/iTerm2/blob/5d0c0d9f68523cbd0494dad5422998964a2ecd8d/sources/iTermPasteHelper.m#L206-L216
                    let pasted = pasted.replace("\r", "\n");
                    self.chat_widget.handle_paste(pasted);
                }
                TuiEvent::Draw => {
                    self.chat_widget.maybe_post_pending_notification(tui);
                    if self
                        .chat_widget
                        .handle_paste_burst_tick(tui.frame_requester())
                    {
                        return Ok(true);
                    }
                    tui.draw(
                        self.chat_widget.desired_height(tui.terminal.size()?.width),
                        |frame| {
                            self.chat_widget.render(frame.area(), frame.buffer);
                            if let Some((x, y)) = self.chat_widget.cursor_pos(frame.area()) {
                                frame.set_cursor_position((x, y));
                            }
                        },
                    )?;
                }
            }
        }
        Ok(true)
    }

    async fn handle_event(&mut self, tui: &mut tui::Tui, event: AppEvent) -> Result<bool> {
        match event {
            AppEvent::NewSession => {
                let summary = session_summary(
                    self.chat_widget.token_usage(),
                    self.chat_widget.conversation_id(),
                );
                self.shutdown_current_conversation().await;
                let init = crate::chatwidget::ChatWidgetInit {
                    config: self.config.clone(),
                    frame_requester: tui.frame_requester(),
                    app_event_tx: self.app_event_tx.clone(),
                    initial_prompt: None,
                    initial_images: Vec::new(),
                    enhanced_keys_supported: self.enhanced_keys_supported,
                    auth_manager: self.auth_manager.clone(),
                    vertical_footer: self.vertical_footer,
                    expected_agent: None, // No filtering for /new command
                };
                self.chat_widget = ChatWidget::new(init, self.server.clone());
                self.chat_widget
                    .set_hotkey_config(self.hotkey_config.clone());
                self.chat_widget.set_vim_mode_enabled(self.vim_mode_enabled);
                if let Some(summary) = summary {
                    let mut lines: Vec<Line<'static>> = vec![summary.usage_line.clone().into()];
                    if let Some(command) = summary.resume_command {
                        let spans = vec!["To continue this session, run ".into(), command.cyan()];
                        lines.push(spans.into());
                    }
                    self.chat_widget.add_plain_history_lines(lines);
                }
                tui.frame_requester().schedule_frame();
            }
            AppEvent::InsertHistoryCell(cell) => {
                let cell: Arc<dyn HistoryCell> = cell.into();
                if let Some(Overlay::Transcript(t)) = &mut self.overlay {
                    t.insert_cell(cell.clone());
                    tui.frame_requester().schedule_frame();
                }
                self.transcript_cells.push(cell.clone());
                let mut display = cell.display_lines(tui.terminal.last_known_screen_size.width);
                if !display.is_empty() {
                    // Only insert a separating blank line for new cells that are not
                    // part of an ongoing stream. Streaming continuations should not
                    // accrue extra blank lines between chunks.
                    if !cell.is_stream_continuation() {
                        if self.has_emitted_history_lines {
                            display.insert(0, Line::from(""));
                        } else {
                            self.has_emitted_history_lines = true;
                        }
                    }
                    if self.overlay.is_some() {
                        self.deferred_history_lines.extend(display);
                    } else {
                        tui.insert_history_lines(display);
                    }
                }
            }
            AppEvent::StartCommitAnimation => {
                if self
                    .commit_anim_running
                    .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
                    .is_ok()
                {
                    let tx = self.app_event_tx.clone();
                    let running = self.commit_anim_running.clone();
                    thread::spawn(move || {
                        while running.load(Ordering::Relaxed) {
                            thread::sleep(Duration::from_millis(50));
                            tx.send(AppEvent::CommitTick);
                        }
                    });
                }
            }
            AppEvent::StopCommitAnimation => {
                self.commit_anim_running.store(false, Ordering::Release);
            }
            AppEvent::CommitTick => {
                self.chat_widget.on_commit_tick();
            }
            AppEvent::CodexEvent(event) => {
                if self.suppress_shutdown_complete
                    && matches!(event.msg, EventMsg::ShutdownComplete)
                {
                    self.suppress_shutdown_complete = false;
                    return Ok(true);
                }
                self.chat_widget.handle_codex_event(event);
            }
            AppEvent::ConversationHistory(ev) => {
                self.on_conversation_history_for_backtrack(tui, ev).await?;
            }
            AppEvent::ExitRequest => {
                // Create and insert exit message cell before exiting
                let exit_cell = self.chat_widget.create_exit_message_cell();

                // Insert the cell directly (inline the InsertHistoryCell logic to avoid recursion)
                let cell: Arc<dyn HistoryCell> = exit_cell.into();
                if let Some(Overlay::Transcript(t)) = &mut self.overlay {
                    t.insert_cell(cell.clone());
                }
                self.transcript_cells.push(cell.clone());
                let mut display = cell.display_lines(tui.terminal.last_known_screen_size.width);
                if !display.is_empty() {
                    if !cell.is_stream_continuation() {
                        if self.has_emitted_history_lines {
                            display.insert(0, Line::from(""));
                        } else {
                            self.has_emitted_history_lines = true;
                        }
                    }
                    if self.overlay.is_some() {
                        self.deferred_history_lines.extend(display);
                    } else {
                        tui.insert_history_lines(display);
                    }
                }

                // Force immediate synchronous draw to flush all history lines to scrollback
                // This will temporarily show the bottom pane in the viewport
                tui.draw(
                    self.chat_widget.desired_height(tui.terminal.size()?.width),
                    |frame| {
                        self.chat_widget.render(frame.area(), frame.buffer);
                        if let Some((x, y)) = self.chat_widget.cursor_pos(frame.area()) {
                            frame.set_cursor_position((x, y));
                        }
                    },
                )?;

                // Clear the viewport to remove the bottom pane, but keep scrollback intact
                tui.terminal.clear()?;

                // Exit the application
                return Ok(false);
            }
            AppEvent::CodexOp(op) => self.chat_widget.submit_op(op),
            AppEvent::DiffResult(text) => {
                // Clear the in-progress state in the bottom pane
                self.chat_widget.on_diff_complete();
                // Enter alternate screen using TUI helper and build pager lines
                let _ = tui.enter_alt_screen();
                let pager_lines: Vec<ratatui::text::Line<'static>> = if text.trim().is_empty() {
                    vec!["No changes detected.".italic().into()]
                } else {
                    text.lines().map(ansi_escape_line).collect()
                };
                self.overlay = Some(Overlay::new_static_with_lines(
                    pager_lines,
                    "D I F F".to_string(),
                ));
                tui.frame_requester().schedule_frame();
            }
            AppEvent::StartFileSearch(query) => {
                if !query.is_empty() {
                    self.file_search.on_user_query(query);
                }
            }
            AppEvent::FileSearchResult { query, matches } => {
                self.chat_widget.apply_file_search_result(query, matches);
            }
            AppEvent::SystemInfoRefreshed(info) => {
                if !self.worktree_warning_shown
                    && let Some(warning) = &info.worktree_cleanup_warning
                {
                    let free = warning.free_percent;
                    let count = warning.worktree_count;
                    let message = format!(
                        "Low disk space: {free}% free. You have {count} git worktree(s) that may be consuming disk space. \
                         Consider running `git worktree remove <path>` to clean up unused worktrees.",
                    );
                    self.chat_widget.add_warning_message(message);
                    self.worktree_warning_shown = true;
                }
                self.chat_widget.apply_system_info_refresh(info);
            }
            AppEvent::RefreshSystemInfoForDirectory { dir, agent } => {
                self.request_system_info_refresh(dir, agent, self.chat_widget.first_prompt_text());
            }
            AppEvent::RateLimitSnapshotFetched(snapshot) => {
                self.chat_widget.on_rate_limit_snapshot(Some(snapshot));
            }
            AppEvent::UpdateReasoningEffort(effort) => {
                self.on_update_reasoning_effort(effort);
            }
            AppEvent::UpdateAgent(model) => {
                self.chat_widget.set_agent(&model);
                self.config.model = model.clone();
                if let Some(family) = find_family_for_model(&model) {
                    self.config.model_family = family;
                }
            }
            AppEvent::OpenFullAccessConfirmation { preset } => {
                self.chat_widget.open_full_access_confirmation(preset);
            }
            AppEvent::OpenWorldWritableWarningConfirmation {
                preset,
                sample_paths,
                extra_count,
                failed_scan,
            } => {
                self.chat_widget.open_world_writable_warning_confirmation(
                    preset,
                    sample_paths,
                    extra_count,
                    failed_scan,
                );
            }
            AppEvent::OpenWindowsSandboxEnablePrompt { preset } => {
                self.chat_widget.open_windows_sandbox_enable_prompt(preset);
            }
            AppEvent::EnableWindowsSandboxForAgentMode { preset } => {
                #[cfg(target_os = "windows")]
                {
                    let profile = self.active_profile.as_deref();
                    let feature_key = Feature::WindowsSandbox.key();
                    match ConfigEditsBuilder::new(&self.config.codex_home)
                        .with_profile(profile)
                        .set_feature_enabled(feature_key, true)
                        .apply()
                        .await
                    {
                        Ok(()) => {
                            self.config.set_windows_sandbox_globally(true);
                            self.chat_widget.clear_forced_auto_mode_downgrade();
                            if let Some((sample_paths, extra_count, failed_scan)) =
                                self.chat_widget.world_writable_warning_details()
                            {
                                self.app_event_tx.send(
                                    AppEvent::OpenWorldWritableWarningConfirmation {
                                        preset: Some(preset.clone()),
                                        sample_paths,
                                        extra_count,
                                        failed_scan,
                                    },
                                );
                            } else {
                                self.app_event_tx.send(AppEvent::CodexOp(
                                    Op::OverrideTurnContext {
                                        cwd: None,
                                        approval_policy: Some(preset.approval),
                                        sandbox_policy: Some(preset.sandbox.clone()),
                                        model: None,
                                        effort: None,
                                        summary: None,
                                    },
                                ));
                                self.app_event_tx
                                    .send(AppEvent::UpdateAskForApprovalPolicy(preset.approval));
                                self.app_event_tx
                                    .send(AppEvent::UpdateSandboxPolicy(preset.sandbox.clone()));
                                self.chat_widget.add_info_message(
                                    "Enabled experimental Windows sandbox.".to_string(),
                                    None,
                                );
                            }
                        }
                        Err(err) => {
                            tracing::error!(
                                error = %err,
                                "failed to enable Windows sandbox feature"
                            );
                            self.chat_widget.add_error_message(format!(
                                "Failed to enable the Windows sandbox feature: {err}"
                            ));
                        }
                    }
                }
                #[cfg(not(target_os = "windows"))]
                {
                    let _ = preset;
                }
            }
            AppEvent::PersistAgentSelection {
                agent: model,
                effort,
            } => {
                let profile = self.active_profile.as_deref();
                match ConfigEditsBuilder::new(&self.config.codex_home)
                    .with_profile(profile)
                    .set_model(Some(model.as_str()), effort)
                    .apply()
                    .await
                {
                    Ok(()) => {
                        let reasoning_label = Self::reasoning_label(effort);
                        if let Some(profile) = profile {
                            self.chat_widget.add_info_message(
                                format!(
                                    "Model changed to {model} {reasoning_label} for {profile} profile"
                                ),
                                None,
                            );
                        } else {
                            self.chat_widget.add_info_message(
                                format!("Model changed to {model} {reasoning_label}"),
                                None,
                            );
                        }
                    }
                    Err(err) => {
                        tracing::error!(
                            error = %err,
                            "failed to persist model selection"
                        );
                        if let Some(profile) = profile {
                            self.chat_widget.add_error_message(format!(
                                "Failed to save model for profile `{profile}`: {err}"
                            ));
                        } else {
                            self.chat_widget
                                .add_error_message(format!("Failed to save default model: {err}"));
                        }
                    }
                }
            }
            AppEvent::UpdateAskForApprovalPolicy(policy) => {
                self.chat_widget.set_approval_policy(policy);
            }
            AppEvent::UpdateSandboxPolicy(policy) => {
                #[cfg(target_os = "windows")]
                let policy_is_workspace_write_or_ro = matches!(
                    policy,
                    codex_core::protocol::SandboxPolicy::WorkspaceWrite { .. }
                        | codex_core::protocol::SandboxPolicy::ReadOnly
                );

                self.config.sandbox_policy = policy.clone();
                #[cfg(target_os = "windows")]
                if !matches!(policy, codex_core::protocol::SandboxPolicy::ReadOnly)
                    || codex_core::get_platform_sandbox().is_some()
                {
                    self.config.forced_auto_mode_downgraded_on_windows = false;
                }
                self.chat_widget.set_sandbox_policy(policy);

                // If sandbox policy becomes workspace-write or read-only, run the Windows world-writable scan.
                #[cfg(target_os = "windows")]
                {
                    // One-shot suppression if the user just confirmed continue.
                    if self.skip_world_writable_scan_once {
                        self.skip_world_writable_scan_once = false;
                        return Ok(true);
                    }

                    let should_check = codex_core::get_platform_sandbox().is_some()
                        && policy_is_workspace_write_or_ro
                        && !self.chat_widget.world_writable_warning_hidden();
                    if should_check {
                        let cwd = self.config.cwd.clone();
                        let env_map: std::collections::HashMap<String, String> =
                            std::env::vars().collect();
                        let tx = self.app_event_tx.clone();
                        let logs_base_dir = self.config.codex_home.clone();
                        let sandbox_policy = self.config.sandbox_policy.clone();
                        Self::spawn_world_writable_scan(
                            cwd,
                            env_map,
                            logs_base_dir,
                            sandbox_policy,
                            tx,
                        );
                    }
                }
            }
            AppEvent::SkipNextWorldWritableScan => {
                self.skip_world_writable_scan_once = true;
            }
            AppEvent::UpdateFullAccessWarningAcknowledged(ack) => {
                self.chat_widget.set_full_access_warning_acknowledged(ack);
            }
            AppEvent::UpdateWorldWritableWarningAcknowledged(ack) => {
                self.chat_widget
                    .set_world_writable_warning_acknowledged(ack);
            }
            AppEvent::PersistFullAccessWarningAcknowledged => {
                if let Err(err) = ConfigEditsBuilder::new(&self.config.codex_home)
                    .set_hide_full_access_warning(true)
                    .apply()
                    .await
                {
                    tracing::error!(
                        error = %err,
                        "failed to persist full access warning acknowledgement"
                    );
                    self.chat_widget.add_error_message(format!(
                        "Failed to save full access confirmation preference: {err}"
                    ));
                }
            }
            AppEvent::PersistWorldWritableWarningAcknowledged => {
                if let Err(err) = ConfigEditsBuilder::new(&self.config.codex_home)
                    .set_hide_world_writable_warning(true)
                    .apply()
                    .await
                {
                    tracing::error!(
                        error = %err,
                        "failed to persist world-writable warning acknowledgement"
                    );
                    self.chat_widget.add_error_message(format!(
                        "Failed to save Agent mode warning preference: {err}"
                    ));
                }
            }
            AppEvent::PersistModelMigrationPromptAcknowledged { migration_config } => {
                if let Err(err) = ConfigEditsBuilder::new(&self.config.codex_home)
                    .set_hide_model_migration_prompt(&migration_config, true)
                    .apply()
                    .await
                {
                    tracing::error!(error = %err, "failed to persist model migration prompt acknowledgement");
                    self.chat_widget.add_error_message(format!(
                        "Failed to save model migration prompt preference: {err}"
                    ));
                }
            }
            AppEvent::OpenApprovalsPopup => {
                self.chat_widget.open_approvals_popup();
            }
            AppEvent::FullScreenApprovalRequest(request) => match request {
                ApprovalRequest::ApplyPatch { cwd, changes, .. } => {
                    let _ = tui.enter_alt_screen();
                    let diff_summary = DiffSummary::new(changes, cwd);
                    self.overlay = Some(Overlay::new_static_with_renderables(
                        vec![diff_summary.into()],
                        "P A T C H".to_string(),
                    ));
                }
                ApprovalRequest::Exec { command, .. } => {
                    let _ = tui.enter_alt_screen();
                    let full_cmd = strip_bash_lc_and_escape(&command);
                    let full_cmd_lines = highlight_bash_to_lines(&full_cmd);
                    self.overlay = Some(Overlay::new_static_with_lines(
                        full_cmd_lines,
                        "E X E C".to_string(),
                    ));
                }
                ApprovalRequest::McpElicitation {
                    server_name,
                    message,
                    ..
                } => {
                    let _ = tui.enter_alt_screen();
                    let paragraph = Paragraph::new(vec![
                        Line::from(vec!["Server: ".into(), server_name.bold()]),
                        Line::from(""),
                        Line::from(message),
                    ])
                    .wrap(Wrap { trim: false });
                    self.overlay = Some(Overlay::new_static_with_renderables(
                        vec![Box::new(paragraph)],
                        "E L I C I T A T I O N".to_string(),
                    ));
                }
            },
            AppEvent::SetPendingAgent {
                agent_name,
                display_name,
            } => {
                // Store the pending agent selection in both App and ChatWidget
                self.pending_agent = Some(PendingAgentSelection {
                    agent_name: agent_name.clone(),
                    display_name: display_name.clone(),
                });
                // Also set on ChatWidget so it can trigger the switch on prompt submission
                self.chat_widget
                    .set_pending_agent(agent_name.clone(), display_name.clone());
                tracing::info!(
                    "Pending agent set: {} ({}). Will switch on next prompt.",
                    display_name,
                    agent_name
                );
                self.chat_widget.add_info_message(
                    format!(
                        "Agent '{display_name}' selected. On next prompt, will start a new conversation with this agent (current history will not be transferred)."
                    ),
                    None,
                );
            }
            AppEvent::SubmitWithAgentSwitch {
                agent_name,
                display_name,
                message_text,
                image_paths,
            } => {
                tracing::info!(
                    "Switching agent to {} ({}) and submitting message",
                    display_name,
                    agent_name
                );

                // Clear the pending agent since we're applying it now
                self.pending_agent = None;

                // Update the model in config
                self.config.model = agent_name.clone();

                // Persist the agent selection to config.toml for next TUI startup
                if let Err(err) = ConfigEditsBuilder::new(&self.config.codex_home)
                    .set_agent(Some(&agent_name))
                    .apply()
                    .await
                {
                    tracing::error!(error = %err, "failed to persist agent selection");
                    // Non-fatal: continue with the switch even if persistence fails
                }

                // Shutdown current conversation
                self.shutdown_current_conversation().await;

                // Create the new chat widget with the new config and the message as initial prompt
                // Set expected_agent to filter events from the OLD agent until SessionConfigured
                let init = crate::chatwidget::ChatWidgetInit {
                    config: self.config.clone(),
                    frame_requester: tui.frame_requester(),
                    app_event_tx: self.app_event_tx.clone(),
                    initial_prompt: Some(message_text),
                    initial_images: image_paths,
                    enhanced_keys_supported: self.enhanced_keys_supported,
                    auth_manager: self.auth_manager.clone(),
                    vertical_footer: self.vertical_footer,
                    expected_agent: Some(agent_name.clone()),
                };
                self.chat_widget = ChatWidget::new(init, self.server.clone());
                self.chat_widget
                    .set_hotkey_config(self.hotkey_config.clone());
                self.chat_widget.set_vim_mode_enabled(self.vim_mode_enabled);

                self.chat_widget.add_info_message(
                    format!("Started new conversation with agent: {display_name}"),
                    None,
                );
            }
            AppEvent::AgentSpawnFailed { agent_name, error } => {
                tracing::warn!(
                    agent = %agent_name,
                    error = %error,
                    "Agent failed to spawn, opening agent picker"
                );

                self.chat_widget.on_agent_spawn_failed(&agent_name, &error);
            }
            AppEvent::AgentConnecting { display_name } => {
                tracing::info!(
                    display_name = %display_name,
                    "Agent connecting, showing status indicator"
                );
                self.chat_widget.show_connecting_status(&display_name);
            }
            #[cfg(feature = "unstable")]
            AppEvent::OpenAcpModelPicker {
                models,
                current_model_id,
            } => {
                self.chat_widget
                    .open_acp_model_picker(models, current_model_id);
            }
            #[cfg(feature = "unstable")]
            AppEvent::SetAcpModel {
                model_id,
                display_name,
            } => {
                self.chat_widget.set_acp_model(model_id, display_name);
            }
            #[cfg(feature = "unstable")]
            AppEvent::AcpModelSetResult {
                success,
                model_id,
                display_name,
                error,
            } => {
                if success {
                    // Update the approval dialog display name to reflect the new model
                    self.chat_widget
                        .update_agent_display_name(display_name.clone());
                    self.chat_widget
                        .add_info_message(format!("Model switched to: {display_name}"), None);

                    // Persist the model selection to [default_models] in config.toml
                    let agent = self.config.model.clone();
                    if let Err(err) = ConfigEditsBuilder::new(&self.config.codex_home)
                        .set_default_model(&agent, &model_id)
                        .apply()
                        .await
                    {
                        tracing::error!(
                            error = %err,
                            "failed to persist default model selection"
                        );
                    }
                } else {
                    let error_msg = error.unwrap_or_else(|| "Unknown error".to_string());
                    self.chat_widget
                        .add_info_message(format!("Failed to switch model: {error_msg}"), None);
                }
            }
            AppEvent::LoginComplete { success } => {
                self.chat_widget.handle_login_complete(success);
            }
            AppEvent::ExternalCliLoginOutput { data } => {
                self.chat_widget.handle_external_cli_login_output(data);
            }
            AppEvent::ExternalCliLoginComplete {
                success,
                agent_name,
            } => {
                self.chat_widget
                    .handle_external_cli_login_complete(success, agent_name);
            }
            AppEvent::SetConfigVerticalFooter(enabled) => {
                self.persist_config_setting("vertical_footer", enabled)
                    .await;
            }
            AppEvent::SetConfigTerminalNotifications(enabled) => {
                self.persist_notification_setting("terminal_notifications", enabled)
                    .await;
            }
            AppEvent::SetConfigOsNotifications(enabled) => {
                self.persist_notification_setting("os_notifications", enabled)
                    .await;
            }
            AppEvent::SetConfigHotkey { action, binding } => {
                self.persist_hotkey_setting(action, binding).await;
            }
            AppEvent::OpenHotkeyPicker => {
                self.chat_widget
                    .open_hotkey_picker(self.hotkey_config.clone());
            }
            #[cfg(feature = "nori-config")]
            AppEvent::OpenNotifyAfterIdlePicker => {
                let nori_config = codex_acp::config::NoriConfig::load().unwrap_or_default();
                self.chat_widget
                    .open_notify_after_idle_picker(nori_config.notify_after_idle);
            }
            #[cfg(feature = "nori-config")]
            AppEvent::SetConfigNotifyAfterIdle(value) => {
                self.persist_notify_after_idle_setting(value).await;
            }
            #[cfg(feature = "nori-config")]
            AppEvent::OpenScriptTimeoutPicker => {
                let nori_config = codex_acp::config::NoriConfig::load().unwrap_or_default();
                self.chat_widget
                    .open_script_timeout_picker(nori_config.script_timeout);
            }
            #[cfg(feature = "nori-config")]
            AppEvent::SetConfigScriptTimeout(value) => {
                self.persist_script_timeout_setting(value).await;
            }
            #[cfg(feature = "nori-config")]
            AppEvent::OpenLoopCountPicker => {
                let nori_config = codex_acp::config::NoriConfig::load().unwrap_or_default();
                self.chat_widget
                    .open_loop_count_picker(nori_config.loop_count);
            }
            #[cfg(feature = "nori-config")]
            AppEvent::SetConfigLoopCount(value) => {
                self.persist_loop_count_setting(value).await;
            }
            #[cfg(feature = "nori-config")]
            AppEvent::SetConfigAutoWorktree(enabled) => {
                self.persist_auto_worktree_setting(enabled).await;
            }
            #[cfg(feature = "nori-config")]
            AppEvent::OpenFooterSegmentsPicker => {
                let nori_config = codex_acp::config::NoriConfig::load().unwrap_or_default();
                self.chat_widget
                    .open_footer_segments_picker(&nori_config.footer_segment_config);
            }
            #[cfg(feature = "nori-config")]
            AppEvent::SetConfigFooterSegment(segment, enabled) => {
                self.persist_footer_segment_setting(segment, enabled).await;
            }
            #[cfg(feature = "nori-config")]
            AppEvent::LoopIteration {
                prompt,
                remaining,
                total,
            } => {
                let iteration = total - remaining;
                tracing::info!("Loop iteration {iteration}/{total} (remaining: {remaining})");

                self.shutdown_current_conversation().await;

                let init = crate::chatwidget::ChatWidgetInit {
                    config: self.config.clone(),
                    frame_requester: tui.frame_requester(),
                    app_event_tx: self.app_event_tx.clone(),
                    initial_prompt: Some(prompt),
                    initial_images: Vec::new(),
                    enhanced_keys_supported: self.enhanced_keys_supported,
                    auth_manager: self.auth_manager.clone(),
                    vertical_footer: self.vertical_footer,
                    expected_agent: None,
                };
                self.chat_widget = ChatWidget::new(init, self.server.clone());
                self.chat_widget
                    .set_hotkey_config(self.hotkey_config.clone());
                self.chat_widget.set_vim_mode_enabled(self.vim_mode_enabled);
                self.chat_widget.set_loop_state(remaining, total);

                self.chat_widget
                    .add_info_message(format!("Loop iteration {iteration} of {total}"), None);
            }
            AppEvent::SetConfigVimMode(value) => {
                self.persist_vim_mode_setting(value).await;
            }
            AppEvent::SkillsetListResult { names, error } => {
                self.chat_widget.on_skillset_list_result(names, error);
            }
            AppEvent::InstallSkillset { name } => {
                self.chat_widget.on_install_skillset_request(&name);
            }
            AppEvent::SkillsetInstallResult {
                name,
                success,
                message,
            } => {
                self.chat_widget
                    .on_skillset_install_result(&name, success, &message);
            }
            AppEvent::ExecuteScript { prompt, args } => {
                let tx = self.app_event_tx.clone();
                let nori_config = codex_acp::config::NoriConfig::load().unwrap_or_default();
                let timeout = nori_config.script_timeout.as_duration();
                let name = prompt.name.clone();
                self.chat_widget
                    .add_info_message(format!("Running script '{name}'..."), None);
                tokio::spawn(async move {
                    let result =
                        codex_core::custom_prompts::execute_script(&prompt, &args, timeout).await;
                    tx.send(AppEvent::ScriptExecutionComplete {
                        name: prompt.name.clone(),
                        result,
                    });
                });
            }
            AppEvent::ScriptExecutionComplete { name, result } => match result {
                Ok(stdout) => {
                    if stdout.trim().is_empty() {
                        self.chat_widget.add_info_message(
                            format!("Script '{name}' completed with no output."),
                            None,
                        );
                    } else {
                        let message = format!("Output from script '{name}':\n{stdout}");
                        self.chat_widget.queue_text_as_user_message(message);
                    }
                }
                Err(err) => {
                    self.chat_widget
                        .add_error_message(format!("Script '{name}' failed: {err}"));
                    let error_context =
                        format!("Script '{name}' failed with the following error:\n{err}");
                    self.chat_widget.queue_text_as_user_message(error_context);
                }
            },
            AppEvent::ShowViewonlySessionPicker {
                sessions,
                nori_home,
            } => {
                let params = crate::nori::viewonly_session_picker::viewonly_session_picker_params(
                    sessions,
                    nori_home,
                    self.app_event_tx.clone(),
                );
                self.chat_widget.show_selection_view(params);
            }
            AppEvent::LoadViewonlyTranscript {
                nori_home,
                project_id,
                session_id,
            } => {
                let tx = self.app_event_tx.clone();
                tokio::spawn(async move {
                    let loader = codex_acp::transcript::TranscriptLoader::new(nori_home);
                    match loader.load_transcript(&project_id, &session_id).await {
                        Ok(transcript) => {
                            let entries =
                                crate::viewonly_transcript::transcript_to_entries(&transcript);
                            tx.send(AppEvent::DisplayViewonlyTranscript { entries });
                        }
                        Err(e) => {
                            tx.send(AppEvent::InsertHistoryCell(Box::new(
                                crate::history_cell::new_error_event(format!(
                                    "Failed to load transcript: {e}"
                                )),
                            )));
                        }
                    }
                });
            }
            AppEvent::DisplayViewonlyTranscript { entries } => {
                self.display_viewonly_transcript(entries);
            }
            AppEvent::ShowResumeSessionPicker {
                sessions,
                nori_home,
            } => {
                let params = crate::nori::resume_session_picker::resume_session_picker_params(
                    sessions,
                    nori_home,
                    self.app_event_tx.clone(),
                );
                self.chat_widget.show_selection_view(params);
            }
            AppEvent::ResumeSession {
                nori_home,
                project_id,
                session_id,
            } => {
                let loader = codex_acp::transcript::TranscriptLoader::new(nori_home);
                match loader.load_transcript(&project_id, &session_id).await {
                    Ok(transcript) => {
                        let acp_session_id = transcript.meta.acp_session_id.clone();
                        let display_name =
                            crate::nori::agent_picker::get_agent_info(&self.config.model)
                                .map(|info| info.display_name)
                                .unwrap_or_else(|| self.config.model.clone());

                        self.shutdown_current_conversation().await;

                        let init = crate::chatwidget::ChatWidgetInit {
                            config: self.config.clone(),
                            frame_requester: tui.frame_requester(),
                            app_event_tx: self.app_event_tx.clone(),
                            initial_prompt: None,
                            initial_images: Vec::new(),
                            enhanced_keys_supported: self.enhanced_keys_supported,
                            auth_manager: self.auth_manager.clone(),
                            vertical_footer: self.vertical_footer,
                            expected_agent: None,
                        };
                        self.chat_widget =
                            ChatWidget::new_resumed_acp(init, acp_session_id, transcript);
                        self.chat_widget
                            .set_hotkey_config(self.hotkey_config.clone());
                        self.chat_widget.set_vim_mode_enabled(self.vim_mode_enabled);

                        self.chat_widget.add_info_message(
                            format!("Resuming session with {display_name}..."),
                            None,
                        );
                        tui.frame_requester().schedule_frame();
                    }
                    Err(e) => {
                        self.chat_widget
                            .add_error_message(format!("Failed to load session transcript: {e}"));
                    }
                }
            }
        }
        Ok(true)
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

    /// Display a loaded transcript in the history view.
    fn display_viewonly_transcript(
        &mut self,
        entries: Vec<crate::viewonly_transcript::ViewonlyEntry>,
    ) {
        use crate::history_cell::AgentMessageCell;
        use crate::markdown::append_markdown;
        use crate::viewonly_transcript::ViewonlyEntry;

        // Add a header
        self.chat_widget.add_info_message(
            "────────── Viewing Previous Session ──────────".to_string(),
            None,
        );

        let mut is_first_entry = true;
        for entry in entries {
            // Add a blank line separator between entries (except before the first)
            if !is_first_entry {
                self.chat_widget
                    .add_plain_history_lines(vec![Line::from("")]);
            }
            is_first_entry = false;

            match entry {
                ViewonlyEntry::User { content } => {
                    // Add user messages with a user prefix to distinguish them
                    self.chat_widget.add_boxed_history(Box::new(
                        crate::history_cell::UserHistoryCell { message: content },
                    ));
                }
                ViewonlyEntry::Assistant { content } => {
                    // Add assistant response with markdown rendering
                    let mut lines = Vec::new();
                    append_markdown(&content, None, &mut lines);
                    let cell = AgentMessageCell::new(lines, true);
                    self.chat_widget.add_boxed_history(Box::new(cell));
                }
                ViewonlyEntry::Thinking { content } => {
                    // Add thinking block with dimmed style (same pattern as reasoning display)
                    let mut lines = Vec::new();
                    append_markdown(&content, None, &mut lines);
                    // Dim all spans in the lines to indicate this is thinking content
                    let dimmed_lines: Vec<Line<'static>> = lines
                        .into_iter()
                        .map(|line| {
                            Line::from(
                                line.spans
                                    .into_iter()
                                    .map(ratatui::prelude::Stylize::dim)
                                    .collect::<Vec<_>>(),
                            )
                        })
                        .collect();
                    let cell = AgentMessageCell::new(dimmed_lines, true);
                    self.chat_widget.add_boxed_history(Box::new(cell));
                }
                ViewonlyEntry::Info { content } => {
                    // Add as an info message
                    self.chat_widget
                        .add_info_message(content, Some("transcript".to_string()));
                }
            }
        }

        self.chat_widget
            .add_info_message("────────── End of Transcript ──────────".to_string(), None);
    }

    fn open_external_editor(&mut self, tui: &mut tui::Tui) {
        use crate::editor;

        let current_text = self.chat_widget.composer_text();
        let editor_cmd = editor::resolve_editor();

        let temp_path = match editor::write_temp_file(&current_text) {
            Ok(path) => path,
            Err(err) => {
                self.chat_widget
                    .add_error_message(format!("Failed to create temp file: {err}"));
                return;
            }
        };

        // Restore terminal to normal mode so the editor can take over
        let _ = tui::restore();

        let status = editor::spawn_editor(&editor_cmd, &temp_path);

        // Re-enable TUI mode
        let _ = tui::set_modes();
        tui.frame_requester().schedule_frame();

        match status {
            Ok(exit_status) if exit_status.success() => {
                match editor::read_and_cleanup_temp_file(&temp_path) {
                    Ok(content) => {
                        let trimmed = content.trim_end().to_string();
                        self.chat_widget.set_composer_text(trimmed);
                    }
                    Err(err) => {
                        self.chat_widget
                            .add_error_message(format!("Failed to read editor output: {err}"));
                    }
                }
            }
            Ok(_) => {
                // Editor exited with non-zero status; discard changes, clean up temp file
                let _ = std::fs::remove_file(&temp_path);
            }
            Err(err) => {
                let _ = std::fs::remove_file(&temp_path);
                self.chat_widget
                    .add_error_message(format!("Failed to launch editor '{editor_cmd}': {err}"));
            }
        }
    }

    /// Persist a TUI config setting to config.toml and apply it immediately.
    async fn persist_config_setting(&mut self, setting_name: &str, enabled: bool) {
        // Apply immediately to the running TUI
        match setting_name {
            "vertical_footer" => {
                self.vertical_footer = enabled;
                self.chat_widget.set_vertical_footer(enabled);
            }
            _ => {
                tracing::warn!("Unknown config setting: {setting_name}");
                return;
            }
        }

        // Persist to config.toml
        if let Err(err) = ConfigEditsBuilder::new(&self.config.codex_home)
            .set_path(&["tui", setting_name], toml_value(enabled))
            .apply()
            .await
        {
            tracing::error!(
                error = %err,
                setting = %setting_name,
                "failed to persist TUI config setting"
            );
            self.chat_widget
                .add_error_message(format!("Failed to save {setting_name} setting: {err}"));
            return;
        }

        let status = if enabled { "enabled" } else { "disabled" };
        self.chat_widget
            .add_info_message(format!("{setting_name} {status}"), None);
    }

    #[cfg(feature = "nori-config")]
    async fn persist_notify_after_idle_setting(
        &mut self,
        value: codex_acp::config::NotifyAfterIdle,
    ) {
        let toml_str = value.toml_value();

        if let Err(err) = ConfigEditsBuilder::new(&self.config.codex_home)
            .set_path(&["tui", "notify_after_idle"], toml_value(toml_str))
            .apply()
            .await
        {
            tracing::error!(
                error = %err,
                "failed to persist notify_after_idle setting"
            );
            self.chat_widget
                .add_error_message(format!("Failed to save notify_after_idle setting: {err}"));
            return;
        }

        self.chat_widget.add_info_message(
            format!(
                "Notify after idle set to {}. Changes will take effect after restart.",
                value.display_name()
            ),
            None,
        );
    }

    #[cfg(feature = "nori-config")]
    async fn persist_script_timeout_setting(&mut self, value: codex_acp::config::ScriptTimeout) {
        let toml_str = value.toml_value();

        if let Err(err) = ConfigEditsBuilder::new(&self.config.codex_home)
            .set_path(&["tui", "script_timeout"], toml_value(toml_str))
            .apply()
            .await
        {
            tracing::error!(
                error = %err,
                "failed to persist script_timeout setting"
            );
            self.chat_widget
                .add_error_message(format!("Failed to save script_timeout setting: {err}"));
            return;
        }

        self.chat_widget.add_info_message(
            format!("Script timeout set to {}.", value.display_name()),
            None,
        );
    }

    #[cfg(feature = "nori-config")]
    async fn persist_loop_count_setting(&mut self, value: Option<i32>) {
        // Store 0 for disabled (None), which deserializes as Some(0) and is
        // treated the same as None by the loop orchestration code.
        let stored = value.unwrap_or(0) as i64;

        if let Err(err) = ConfigEditsBuilder::new(&self.config.codex_home)
            .set_path(&["tui", "loop_count"], toml_value(stored))
            .apply()
            .await
        {
            tracing::error!(
                error = %err,
                "failed to persist loop_count setting"
            );
            self.chat_widget
                .add_error_message(format!("Failed to save loop_count setting: {err}"));
            return;
        }

        let display = match value {
            Some(n) => format!("{n}"),
            None => "Disabled".to_string(),
        };
        self.chat_widget
            .add_info_message(format!("Loop count set to {display}."), None);
    }

    async fn persist_vim_mode_setting(&mut self, enabled: bool) {
        if let Err(err) = ConfigEditsBuilder::new(&self.config.codex_home)
            .set_path(&["tui", "vim_mode"], toml_value(enabled))
            .apply()
            .await
        {
            tracing::error!(
                error = %err,
                "failed to persist vim_mode setting"
            );
            self.chat_widget
                .add_error_message(format!("Failed to save vim_mode setting: {err}"));
            return;
        }

        // Update in-memory state and propagate to the chat widget
        self.vim_mode_enabled = enabled;
        self.chat_widget.set_vim_mode_enabled(enabled);

        let status = if enabled { "enabled" } else { "disabled" };
        self.chat_widget
            .add_info_message(format!("Vim mode {status}."), None);
    }

    #[cfg(feature = "nori-config")]
    async fn persist_auto_worktree_setting(&mut self, enabled: bool) {
        if let Err(err) = ConfigEditsBuilder::new(&self.config.codex_home)
            .set_path(&["tui", "auto_worktree"], toml_value(enabled))
            .apply()
            .await
        {
            tracing::error!(
                error = %err,
                "failed to persist auto_worktree setting"
            );
            self.chat_widget
                .add_error_message(format!("Failed to save auto_worktree setting: {err}"));
            return;
        }

        let status = if enabled { "enabled" } else { "disabled" };
        self.chat_widget.add_info_message(
            format!("Auto worktree {status}. Changes will take effect on next session."),
            None,
        );
    }

    #[cfg(feature = "nori-config")]
    async fn persist_footer_segment_setting(
        &mut self,
        segment: codex_acp::config::FooterSegment,
        enabled: bool,
    ) {
        if let Err(err) = ConfigEditsBuilder::new(&self.config.codex_home)
            .set_path(
                &["tui", "footer_segments", segment.toml_key()],
                toml_value(enabled),
            )
            .apply()
            .await
        {
            tracing::error!(
                error = %err,
                "failed to persist footer_segment setting"
            );
            self.chat_widget
                .add_error_message(format!("Failed to save footer segment setting: {err}"));
            return;
        }

        // Update the local config and apply to the widget
        self.chat_widget
            .set_footer_segment_enabled(segment, enabled);

        let status = if enabled { "enabled" } else { "disabled" };
        self.chat_widget
            .add_info_message(format!("{} {status}.", segment.display_name()), None);

        // Refresh the picker to show updated state without stacking a new view.
        let nori_config = codex_acp::config::NoriConfig::load().unwrap_or_default();
        self.chat_widget
            .replace_footer_segments_picker(&nori_config.footer_segment_config);
    }

    async fn persist_notification_setting(&mut self, setting_name: &str, enabled: bool) {
        let enum_value = if enabled { "enabled" } else { "disabled" };

        // Persist to config.toml as a string enum value
        if let Err(err) = ConfigEditsBuilder::new(&self.config.codex_home)
            .set_path(&["tui", setting_name], toml_value(enum_value))
            .apply()
            .await
        {
            tracing::error!(
                error = %err,
                setting = %setting_name,
                "failed to persist TUI notification setting"
            );
            self.chat_widget
                .add_error_message(format!("Failed to save {setting_name} setting: {err}"));
            return;
        }

        let status = if enabled { "enabled" } else { "disabled" };
        self.chat_widget
            .add_info_message(format!("{setting_name} {status}"), None);
    }

    async fn persist_hotkey_setting(
        &mut self,
        action: codex_acp::config::HotkeyAction,
        binding: codex_acp::config::HotkeyBinding,
    ) {
        let toml_key = action.toml_key();
        let toml_val = binding.toml_value();

        if let Err(err) = ConfigEditsBuilder::new(&self.config.codex_home)
            .set_path(&["tui", "hotkeys", toml_key], toml_value(&toml_val))
            .apply()
            .await
        {
            tracing::error!(
                error = %err,
                action = %action.display_name(),
                "failed to persist hotkey setting"
            );
            self.chat_widget.add_error_message(format!(
                "Failed to save hotkey for {}: {err}",
                action.display_name()
            ));
            return;
        }

        self.hotkey_config.set_binding(action, binding.clone());
        self.chat_widget
            .set_hotkey_config(self.hotkey_config.clone());
        self.chat_widget.add_info_message(
            format!(
                "{} bound to {}",
                action.display_name(),
                binding.display_name()
            ),
            None,
        );
    }

    async fn handle_key_event(&mut self, tui: &mut tui::Tui, key_event: KeyEvent) {
        use crate::nori::hotkey_match::matches_binding;
        use codex_acp::config::HotkeyAction;

        // Check configurable hotkeys first (before the structural match),
        // but only when no popup/view is active — otherwise the popup should
        // capture the key (e.g. the hotkey picker in rebinding mode).
        if key_event.kind == KeyEventKind::Press && !self.chat_widget.has_active_popup() {
            let transcript_binding = self.hotkey_config.binding_for(HotkeyAction::OpenTranscript);
            if matches_binding(transcript_binding, &key_event) {
                let _ = tui.enter_alt_screen();
                self.overlay = Some(Overlay::new_transcript(self.transcript_cells.clone()));
                tui.frame_requester().schedule_frame();
                return;
            }

            let editor_binding = self.hotkey_config.binding_for(HotkeyAction::OpenEditor);
            if matches_binding(editor_binding, &key_event) {
                self.open_external_editor(tui);
                return;
            }
        }

        match key_event {
            // Esc primes/advances backtracking only in normal (not working) mode
            // with the composer focused and empty. In any other state, forward
            // Esc so the active UI (e.g. status indicator, modals, popups)
            // handles it.
            KeyEvent {
                code: KeyCode::Esc,
                kind: KeyEventKind::Press | KeyEventKind::Repeat,
                ..
            } => {
                if self.chat_widget.is_normal_backtrack_mode()
                    && self.chat_widget.composer_is_empty()
                {
                    self.handle_backtrack_esc_key(tui);
                } else {
                    self.chat_widget.handle_key_event(key_event);
                }
            }
            // Enter confirms backtrack when primed + count > 0. Otherwise pass to widget.
            KeyEvent {
                code: KeyCode::Enter,
                kind: KeyEventKind::Press,
                ..
            } if self.backtrack.primed
                && self.backtrack.nth_user_message != usize::MAX
                && self.chat_widget.composer_is_empty() =>
            {
                // Delegate to helper for clarity; preserves behavior.
                self.confirm_backtrack_from_main();
            }
            KeyEvent {
                kind: KeyEventKind::Press | KeyEventKind::Repeat,
                ..
            } => {
                // Any non-Esc key press should cancel a primed backtrack.
                // This avoids stale "Esc-primed" state after the user starts typing
                // (even if they later backspace to empty).
                if key_event.code != KeyCode::Esc && self.backtrack.primed {
                    self.reset_backtrack_state();
                }
                self.chat_widget.handle_key_event(key_event);
            }
            _ => {
                // Ignore Release key events.
            }
        };
    }

    #[cfg(target_os = "windows")]
    fn spawn_world_writable_scan(
        cwd: PathBuf,
        env_map: std::collections::HashMap<String, String>,
        logs_base_dir: PathBuf,
        sandbox_policy: codex_core::protocol::SandboxPolicy,
        tx: AppEventSender,
    ) {
        tokio::task::spawn_blocking(move || {
            let result = codex_windows_sandbox::apply_world_writable_scan_and_denies(
                &logs_base_dir,
                &cwd,
                &env_map,
                &sandbox_policy,
                Some(logs_base_dir.as_path()),
            );
            if result.is_err() {
                // Scan failed: warn without examples.
                tx.send(AppEvent::OpenWorldWritableWarningConfirmation {
                    preset: None,
                    sample_paths: Vec::new(),
                    extra_count: 0usize,
                    failed_scan: true,
                });
            }
        });
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
