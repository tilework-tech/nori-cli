// Forbid accidental stdout/stderr writes in the *library* portion of the TUI.
// The standalone `nori-tui` binary prints a short help message before the
// alternate‑screen mode starts; that file opts‑out locally via `allow`.
#![deny(clippy::print_stdout, clippy::print_stderr)]
#![deny(clippy::disallowed_methods)]
use additional_dirs::add_dir_warning_message;
use app::App;
pub use app::AppExitInfo;
pub use app::RESUME_HINT_LEAD;
pub use app::resume_command_for_conversation;
use codex_login::AuthManager;
#[cfg(target_os = "windows")]
use codex_sandbox::get_platform_sandbox;
use nori_config::AskForApproval;
use nori_config::NoriConfig;
use nori_config::NoriConfigOverrides;
use nori_config::SandboxMode;
use nori_harness::transcript::SessionMetadata;
use nori_harness::transcript::TranscriptLoader;
use std::fs::OpenOptions;
use std::path::PathBuf;
use tracing::error;
use tracing_appender::non_blocking;
use tracing_subscriber::EnvFilter;
#[allow(unused_imports)]
use tracing_subscriber::filter::Targets;
use tracing_subscriber::prelude::*;
pub use ui_types::TokenUsage;

mod additional_dirs;
mod app;
mod app_backtrack;
mod app_event;
mod app_event_sender;
mod bottom_pane;
mod chatwidget;
mod cli;
mod client_event_format;
mod client_tool_cell;
mod clipboard_paste;
mod color;
pub mod custom_terminal;
mod diff_render;
mod editor;
mod effective_cwd_tracker;
mod exec_cell;
mod exec_command;
mod file_search;
mod frames;
mod get_git_diff;
mod git_marker;
mod history_cell;
pub mod insert_history;
mod key_hint;
pub mod live_wrap;
mod login_handler;
mod markdown;
mod markdown_render;
mod markdown_stream;
mod nori;
mod overlay_menu;
mod pager_overlay;
mod pinned_plan_drawer;
mod presentation;
pub mod public_widgets;
mod render;
mod resume_picker;
mod session_log;
pub mod session_stats;
mod shimmer;
mod slash_command;
mod status;
mod status_indicator_widget;
mod streaming;
mod style;
mod system_info;
mod terminal_palette;
mod terminal_title;
mod text_formatting;
mod transcript_reflow;
mod tui;
mod ui_consts;
mod ui_types;
mod viewonly_transcript;

// Nori-specific update modules
// Re-export as pub mod for external access to UpdateAction type
pub mod update_action {
    pub use super::nori::update_action::*;
}
// Re-export Nori updates module (release builds only)
#[cfg(not(debug_assertions))]
mod updates {
    pub use super::nori::updates::*;
}

// Re-export update prompt functions (release builds only)
#[cfg(not(debug_assertions))]
pub(crate) use nori::update_prompt::UpdatePromptOutcome;
#[cfg(not(debug_assertions))]
pub(crate) use nori::update_prompt::run_update_prompt_if_needed;

mod version;

mod wrapping;

#[cfg(test)]
pub mod test_backend;

use crate::nori::onboarding::NoriOnboardingScreenArgs;
use crate::nori::onboarding::run_nori_onboarding_app;
use crate::tui::Tui;
pub use cli::Cli;
pub use markdown_render::render_markdown_text;
pub use public_widgets::composer_input::ComposerAction;
pub use public_widgets::composer_input::ComposerInput;
use std::io::Write as _;

// (tests access modules directly within the crate)

pub async fn run_main(
    cli: Cli,
    _codex_linux_sandbox_exe: Option<PathBuf>,
) -> std::io::Result<AppExitInfo> {
    // Pre-warm the ACP agent installation cache in a background thread.
    // This runs `which` commands early so the agent picker opens quickly.
    std::thread::spawn(|| {
        nori_harness::prewarm_installation_cache();
    });

    // Track install/session in background (non-blocking, fire-and-forget)
    // This updates ~/.nori/cli/.nori-install.json with launch metadata
    if let Ok(nori_home) = nori_config::find_nori_home() {
        nori_installed::track_launch(&nori_home);
    }

    // Note: Rolling file tracing is initialized in nori-cli main.rs before run_main() is called.
    // This ensures a single point of file-based tracing initialization.

    let (sandbox_mode, approval_policy): (Option<SandboxMode>, Option<AskForApproval>) =
        if cli.dangerously_bypass_approvals_and_sandbox {
            (
                Some(SandboxMode::DangerFullAccess),
                Some(AskForApproval::Never),
            )
        } else {
            (None, None)
        };

    let raw_overrides = cli.config_overrides.raw_overrides.clone();
    let overrides_cli = codex_common::CliConfigOverrides { raw_overrides };
    let cli_kv_overrides = match overrides_cli.parse_overrides() {
        // Parse `-c` overrides from the CLI.
        Ok(v) => v,
        #[allow(clippy::print_stderr)]
        Err(e) => {
            eprintln!("Error parsing -c overrides: {e}");
            std::process::exit(1);
        }
    };

    // canonicalize the cwd
    let mut cwd = cli.cwd.clone().map(|p| p.canonicalize().unwrap_or(p));
    let additional_dirs = cli.add_dir.clone();

    let mut overrides = NoriConfigOverrides {
        agent: cli.agent.clone(),
        approval_policy,
        sandbox_mode,
        cwd: cwd.clone(),
        additional_writable_roots: additional_dirs,
        raw_overrides: cli_kv_overrides,
    };
    let mut config = load_config_or_exit(overrides.clone());

    // Initialize the agent registry with custom agents from config plus any
    // caller-injected entries (e.g. `nori cloud`'s pinned handroll agent).
    let mut registry_agents = config.agents.clone();
    // Caller-injected entries win slug collisions (a user-defined agent with
    // the same slug would otherwise fail registry init with a duplicate-slug
    // error and break cloud mode with a confusing "unknown agent").
    registry_agents.retain(|agent| {
        !cli.extra_agents
            .iter()
            .any(|extra| extra.slug == agent.slug)
    });
    registry_agents.extend(cli.extra_agents.clone());
    if let Err(e) = nori_harness::initialize_registry(registry_agents) {
        tracing::warn!("Failed to initialize agent registry with custom agents: {e}");
    }

    let (pending_worktree_ask, worktree_blocked_reason) = {
        use nori_config::AutoWorktree;
        let auto_worktree = config.auto_worktree;

        if !auto_worktree.is_enabled() {
            (false, None)
        } else {
            match cwd.clone().or_else(|| std::env::current_dir().ok()) {
                Some(ref effective_cwd) => {
                    match nori_harness::auto_worktree::can_create_worktree(effective_cwd) {
                        Err(reason) => {
                            tracing::debug!("Worktree creation blocked: {reason}");
                            (false, Some(reason.to_string()))
                        }
                        Ok(()) => match auto_worktree {
                            AutoWorktree::Automatic => {
                                match nori_harness::auto_worktree::setup_auto_worktree(
                                    effective_cwd,
                                ) {
                                    Ok(worktree_path) => {
                                        tracing::info!(
                                            "Auto-worktree created at {}",
                                            worktree_path.display()
                                        );
                                        cwd = Some(worktree_path);
                                    }
                                    Err(e) => {
                                        tracing::warn!("Auto-worktree setup skipped: {e}");
                                    }
                                }
                                (false, None)
                            }
                            AutoWorktree::Ask => (true, None),
                            AutoWorktree::Off => (false, None),
                        },
                    }
                }
                None => {
                    tracing::warn!(
                        "Auto-worktree setup skipped: could not determine working directory"
                    );
                    (false, None)
                }
            }
        }
    };
    if cwd != overrides.cwd {
        overrides.cwd = cwd;
        config = load_config_or_exit(overrides.clone());
    }

    #[cfg(target_os = "windows")]
    {
        codex_sandbox::set_windows_sandbox_enabled(config.windows_sandbox_enabled);
        config.apply_windows_sandbox_availability(get_platform_sandbox().is_some());
    }

    if let Some(warning) = add_dir_warning_message(&cli.add_dir, &config.sandbox_policy) {
        #[allow(clippy::print_stderr)]
        {
            eprintln!("Error adding directories: {warning}");
            std::process::exit(1);
        }
    }

    let log_dir = config.nori_home.join("log");
    std::fs::create_dir_all(&log_dir)?;
    // Open (or create) your log file, appending to it.
    let mut log_file_opts = OpenOptions::new();
    log_file_opts.create(true).append(true);

    // Ensure the file is only readable and writable by the current user.
    // Doing the equivalent to `chmod 600` on Windows is quite a bit more code
    // and requires the Windows API crates, so we can reconsider that when
    // Codex CLI is officially supported on Windows.
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        log_file_opts.mode(0o600);
    }

    let log_file = log_file_opts.open(log_dir.join("nori-tui.log"))?;

    // Wrap file in non‑blocking writer.
    let (non_blocking, _guard) = non_blocking(log_file);

    // use RUST_LOG env var, default to info for codex crates.
    let env_filter = || {
        EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| EnvFilter::new("nori_tui=info,codex_rmcp_client=info"))
    };

    let file_layer = tracing_subscriber::fmt::layer()
        .with_writer(non_blocking)
        .with_target(false)
        .with_span_events(tracing_subscriber::fmt::format::FmtSpan::CLOSE)
        .with_filter(env_filter());

    let _ = tracing_subscriber::registry().with(file_layer).try_init();

    // Remote ACP transport (docs/specs/remote-acp-transport.md): bind the
    // listener before the app runs so a controller can connect as soon as
    // the session launches. The server lives for the whole app run.
    let _remote_server = match cli.remote.as_deref() {
        Some(spec) => {
            let addr =
                nori_harness::remote_agent::parse_bind_addr(spec, cli.remote_allow_nonloopback)
                    .map_err(|error| std::io::Error::other(error.to_string()))?;
            let host = std::sync::Arc::new(nori_harness::remote_agent::HarnessRemoteHost::new());
            let server = nori_harness::remote_agent::RemoteAcpServer::bind(addr, host.clone())
                .await
                .map_err(|error| std::io::Error::other(error.to_string()))?;
            nori_harness::remote_agent::set_active_host(host);
            tracing::info!(
                "remote ACP transport listening on ws://{}/acp",
                server.local_addr()
            );
            Some(server)
        }
        None => None,
    };

    run_ratatui_app(
        cli,
        config,
        overrides,
        pending_worktree_ask,
        worktree_blocked_reason,
    )
    .await
    .map_err(|err| std::io::Error::other(err.to_string()))
}

#[allow(clippy::too_many_arguments)]
async fn run_ratatui_app(
    cli: Cli,
    initial_config: NoriConfig,
    overrides: NoriConfigOverrides,
    pending_worktree_ask: bool,
    worktree_blocked_reason: Option<String>,
) -> color_eyre::Result<AppExitInfo> {
    color_eyre::install()?;

    // Forward panic reports through tracing so they appear in the UI status
    // line, but do not swallow the default/color-eyre panic handler.
    // Chain to the previous hook so users still get a rich panic report
    // (including backtraces) after we restore the terminal.
    let prev_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        tracing::error!("panic: {info}");
        prev_hook(info);
    }));
    let mut terminal = tui::init()?;
    terminal.clear()?;

    let mut tui = Tui::new(terminal);

    #[cfg(not(debug_assertions))]
    {
        let skip_update_prompt = cli.prompt.as_ref().is_some_and(|prompt| !prompt.is_empty());
        if !skip_update_prompt {
            match run_update_prompt_if_needed(&mut tui, &initial_config).await? {
                UpdatePromptOutcome::Continue => {}
                UpdatePromptOutcome::RunUpdate(action) => {
                    crate::tui::restore()?;
                    return Ok(AppExitInfo {
                        token_usage: crate::ui_types::TokenUsage::default(),
                        conversation_id: None,
                        conversation_has_activity: false,
                        update_action: Some(action),
                    });
                }
            }
        }
    }

    // Initialize high-fidelity session event logging if enabled.
    session_log::maybe_init(&initial_config);

    let auth_manager = AuthManager::shared(
        initial_config.nori_home.clone(),
        false,
        codex_login::AuthCredentialsStoreMode::File,
    );
    let should_show_trust_screen = should_show_trust_screen(&initial_config);
    let should_show_onboarding = should_show_trust_screen
        || (!cli.skip_welcome && nori::onboarding::is_first_launch(&initial_config.nori_home));

    let (config, overrides) = if should_show_onboarding {
        // Use Nori-branded onboarding flow
        let onboarding_result = run_nori_onboarding_app(
            NoriOnboardingScreenArgs {
                show_trust_screen: should_show_trust_screen,
                skip_welcome: cli.skip_welcome,
                skip_trust_directory: cli.skip_trust_directory,
                config: initial_config.clone(),
            },
            &mut tui,
        )
        .await?;
        if onboarding_result.should_exit {
            restore();
            session_log::log_session_end();
            let _ = tui.terminal.clear();
            return Ok(AppExitInfo {
                token_usage: crate::ui_types::TokenUsage::default(),
                conversation_id: None,
                conversation_has_activity: false,
                update_action: None,
            });
        }
        // Trust decisions are persisted by onboarding; reload explicitly so
        // the resolved policy and active project reflect that decision.
        if onboarding_result.directory_trust_decision.is_some() {
            let config = load_config_or_exit(overrides.clone());
            (config, overrides)
        } else {
            (initial_config, overrides)
        }
    } else {
        (initial_config, overrides)
    };

    // Auto-worktree: show a popup if worktree creation is blocked, or ask the
    // user whether to create one.
    let mut config = if let Some(reason) = worktree_blocked_reason {
        nori::worktree_ask::run_worktree_blocked_popup(&mut tui, &reason).await?;
        config
    } else if pending_worktree_ask {
        let effective_cwd = config.cwd.clone();
        let user_wants_worktree = nori::worktree_ask::run_worktree_ask_popup(&mut tui).await?;
        if user_wants_worktree {
            match nori_harness::auto_worktree::setup_auto_worktree(&effective_cwd) {
                Ok(worktree_path) => {
                    tracing::info!("Auto-worktree created at {}", worktree_path.display());
                    let mut new_overrides = overrides;
                    new_overrides.cwd = Some(worktree_path);
                    load_config_or_exit(new_overrides)
                }
                Err(e) => {
                    tracing::warn!("Auto-worktree setup skipped: {e}");
                    config
                }
            }
        } else {
            config
        }
    } else {
        config
    };

    let resume_agent_filter = cli.agent.as_deref();
    let transcript_loader = TranscriptLoader::new(config.nori_home.clone());

    // Determine resume behavior: explicit id, then resume last, then picker.
    let resume_selection = if let Some(id_str) = cli.resume_session_id.as_deref() {
        match transcript_loader
            .find_session_metadata_by_id(id_str)
            .await?
        {
            Some(metadata) => match resume_target_from_metadata(
                config.nori_home.clone(),
                metadata,
                resume_agent_filter,
            ) {
                Ok(target) => resume_picker::ResumeSelection::Resume(target),
                Err(message) => {
                    return resume_startup_error(&mut tui, message);
                }
            },
            None => {
                return resume_startup_error(
                    &mut tui,
                    format!(
                        "No saved session found with ID {id_str}. Run `nori resume` without an ID to choose from existing sessions."
                    ),
                );
            }
        }
    } else if cli.resume_last {
        let filter_cwd = if cli.resume_show_all {
            None
        } else {
            Some(config.cwd.as_path())
        };
        match transcript_loader
            .list_resumable_session_metadata(filter_cwd, resume_agent_filter)
            .await
        {
            Ok(sessions) => match sessions.into_iter().next() {
                Some(metadata) => match resume_target_from_metadata(
                    config.nori_home.clone(),
                    metadata,
                    resume_agent_filter,
                ) {
                    Ok(target) => resume_picker::ResumeSelection::Resume(target),
                    Err(message) => {
                        return resume_startup_error(&mut tui, message);
                    }
                },
                None => resume_picker::ResumeSelection::StartFresh,
            },
            Err(_) => resume_picker::ResumeSelection::StartFresh,
        }
    } else if cli.resume_picker {
        match resume_picker::run_resume_picker(
            &mut tui,
            &config.nori_home,
            resume_agent_filter,
            cli.resume_show_all,
        )
        .await?
        {
            resume_picker::ResumeSelection::Exit => {
                restore();
                session_log::log_session_end();
                return Ok(AppExitInfo {
                    token_usage: crate::ui_types::TokenUsage::default(),
                    conversation_id: None,
                    conversation_has_activity: false,
                    update_action: None,
                });
            }
            other => other,
        }
    } else {
        resume_picker::ResumeSelection::StartFresh
    };

    if let resume_picker::ResumeSelection::Resume(target) = &resume_selection
        && let Some(agent) = target.agent.as_ref()
    {
        config.active_agent = agent.clone();
    }

    let Cli {
        prompt,
        images,
        cloud_mode,
        cloud_onboard,
        ..
    } = cli;
    let vertical_footer = config.vertical_footer;

    let app_result = App::run(
        &mut tui,
        auth_manager,
        config,
        prompt,
        images,
        resume_selection,
        vertical_footer,
        cloud_mode,
        cloud_onboard,
    )
    .await;

    restore();
    // Mark the end of the recorded session.
    session_log::log_session_end();
    // ignore error when collecting usage – report underlying error instead
    app_result
}

fn resume_target_from_metadata(
    nori_home: PathBuf,
    metadata: SessionMetadata,
    requested_agent: Option<&str>,
) -> std::result::Result<resume_picker::ResumeTarget, String> {
    if let (Some(requested_agent), Some(recorded_agent)) =
        (requested_agent, metadata.agent.as_deref())
        && requested_agent != recorded_agent
    {
        return Err(format!(
            "Session {} was recorded with agent `{recorded_agent}`, but `--agent {requested_agent}` was requested.",
            metadata.session_id
        ));
    }

    Ok(resume_picker::ResumeTarget {
        nori_home,
        project_id: metadata.project_id,
        session_id: metadata.session_id,
        agent: metadata.agent,
    })
}

fn resume_startup_error(tui: &mut Tui, message: String) -> color_eyre::Result<AppExitInfo> {
    error!("{message}");
    restore();
    session_log::log_session_end();
    let _ = tui.terminal.clear();
    if let Err(err) = writeln!(std::io::stdout(), "{message}") {
        error!("Failed to write resume error message: {err}");
    }
    Ok(AppExitInfo {
        token_usage: crate::ui_types::TokenUsage::default(),
        conversation_id: None,
        conversation_has_activity: false,
        update_action: None,
    })
}

#[expect(
    clippy::print_stderr,
    reason = "TUI should no longer be displayed, so we can write to stderr."
)]
fn restore() {
    if let Err(err) = tui::restore() {
        eprintln!(
            "failed to restore terminal. Run `reset` or restart your terminal to recover: {err}"
        );
    }
}

fn load_config_or_exit(overrides: NoriConfigOverrides) -> NoriConfig {
    #[allow(clippy::print_stderr)]
    match NoriConfig::load_with_overrides(overrides) {
        Ok(config) => config,
        Err(err) => {
            eprintln!("Error loading configuration: {err}");
            std::process::exit(1);
        }
    }
}

/// Determine if user has configured a sandbox / approval policy,
/// or if the current cwd project is already trusted. If not, we need to
/// show the trust screen.
fn should_show_trust_screen(config: &NoriConfig) -> bool {
    if cfg!(target_os = "windows")
        && (!config.windows_sandbox_enabled || config.forced_auto_mode_downgraded_on_windows)
    {
        // Native Windows cannot enforce sandboxed write access, so skip the
        // trust prompt when the configured sandbox is disabled or unavailable.
        return false;
    }
    if config.has_explicit_approval_or_sandbox_policy {
        // Respect explicit approval/sandbox overrides made by the user.
        return false;
    }
    // otherwise, show only if no trust decision has been made
    config.active_project.trust_level.is_none()
}

#[cfg(test)]
mod tests {
    use super::*;
    use nori_config::ProjectConfig;
    use serial_test::serial;
    use tempfile::TempDir;

    fn session_metadata(agent: Option<&str>) -> SessionMetadata {
        SessionMetadata {
            session_id: "session-123".to_string(),
            project_id: "project-123".to_string(),
            started_at: "2025-01-01T00:00:00Z".to_string(),
            cwd: PathBuf::from("/tmp/project"),
            agent: agent.map(str::to_string),
        }
    }

    #[test]
    fn resume_target_uses_recorded_agent_when_agent_not_requested() {
        let target = resume_target_from_metadata(
            PathBuf::from("/tmp/nori-home"),
            session_metadata(Some("codex")),
            None,
        )
        .expect("recorded agent should be accepted");

        assert_eq!(target.agent.as_deref(), Some("codex"));
    }

    #[test]
    fn resume_target_rejects_requested_agent_mismatch() {
        let error = resume_target_from_metadata(
            PathBuf::from("/tmp/nori-home"),
            session_metadata(Some("codex")),
            Some("claude-code"),
        )
        .expect_err("mismatched agent should be rejected");

        assert!(error.contains("recorded with agent `codex`"));
        assert!(error.contains("--agent claude-code"));
    }

    #[test]
    #[serial]
    fn windows_skips_trust_prompt_without_sandbox() -> std::io::Result<()> {
        let temp_dir = TempDir::new()?;
        let mut config = NoriConfig {
            cwd: temp_dir.path().to_path_buf(),
            ..NoriConfig::default()
        };
        config.has_explicit_approval_or_sandbox_policy = false;
        config.active_project = ProjectConfig { trust_level: None };
        config.windows_sandbox_enabled = false;

        let should_show = should_show_trust_screen(&config);
        if cfg!(target_os = "windows") {
            assert!(
                !should_show,
                "Windows trust prompt should always be skipped on native Windows"
            );
        } else {
            assert!(
                should_show,
                "Non-Windows should still show trust prompt when project is untrusted"
            );
        }
        Ok(())
    }
    #[test]
    #[serial]
    fn windows_shows_trust_prompt_with_sandbox() -> std::io::Result<()> {
        let temp_dir = TempDir::new()?;
        let mut config = NoriConfig {
            cwd: temp_dir.path().to_path_buf(),
            ..NoriConfig::default()
        };
        config.has_explicit_approval_or_sandbox_policy = false;
        config.active_project = ProjectConfig { trust_level: None };
        config.windows_sandbox_enabled = true;

        let should_show = should_show_trust_screen(&config);
        if cfg!(target_os = "windows") {
            assert!(
                should_show,
                "Windows trust prompt should be shown on native Windows with sandbox enabled"
            );
        } else {
            assert!(
                should_show,
                "Non-Windows should still show trust prompt when project is untrusted"
            );
        }
        Ok(())
    }
    #[test]
    fn untrusted_project_skips_trust_prompt() -> std::io::Result<()> {
        use nori_config::TrustLevel;
        let temp_dir = TempDir::new()?;
        let mut config = NoriConfig {
            cwd: temp_dir.path().to_path_buf(),
            ..NoriConfig::default()
        };
        config.has_explicit_approval_or_sandbox_policy = false;
        config.active_project = ProjectConfig {
            trust_level: Some(TrustLevel::Untrusted),
        };

        let should_show = should_show_trust_screen(&config);
        assert!(
            !should_show,
            "Trust prompt should not be shown for projects explicitly marked as untrusted"
        );
        Ok(())
    }
}
