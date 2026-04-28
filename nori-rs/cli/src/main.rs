use clap::CommandFactory;
use clap::Parser;
use codex_arg0::arg0_dispatch_or_else;
use codex_common::CliConfigOverrides;
use codex_execpolicy::ExecPolicyCheckCommand;
use nori_acp::find_nori_home;
use nori_acp::init_rolling_file_tracing;
use nori_cli::LandlockCommand;
use nori_cli::SeatbeltCommand;
use nori_cli::WindowsCommand;
#[cfg(feature = "login")]
use nori_cli::login::read_api_key_from_stdin;
#[cfg(feature = "login")]
use nori_cli::login::run_login_status;
#[cfg(feature = "login")]
use nori_cli::login::run_login_with_api_key;
#[cfg(feature = "login")]
use nori_cli::login::run_login_with_chatgpt;
#[cfg(feature = "login")]
use nori_cli::login::run_login_with_device_code;
#[cfg(feature = "login")]
use nori_cli::login::run_logout;

use nori_tui::AppExitInfo;
use nori_tui::Cli as TuiCli;
use nori_tui::update_action::UpdateAction;
use owo_colors::OwoColorize;
use std::path::PathBuf;
use supports_color::Stream;

#[cfg(not(windows))]
mod wsl_paths;

/// Nori CLI
///
/// If no subcommand is specified, options will be forwarded to the interactive CLI.
#[derive(Debug, Parser)]
#[clap(
    name = "nori-ai-cli",
    author,
    version,
    // If a sub‑command is given, ignore requirements of the default args.
    subcommand_negates_reqs = true,
    // The executable is sometimes invoked via a platform‑specific name like
    // `nori-x86_64-unknown-linux-musl`, but the help output should always use
    // the generic `nori` command name that users run.
    bin_name = "nori",
    override_usage = "nori [OPTIONS] [PROMPT]\n       nori [OPTIONS] <COMMAND> [ARGS]"
)]
struct MultitoolCli {
    #[clap(flatten)]
    pub config_overrides: CliConfigOverrides,

    #[clap(flatten)]
    interactive: TuiCli,

    #[clap(subcommand)]
    subcommand: Option<Subcommand>,
}

#[derive(Debug, clap::Subcommand)]
enum Subcommand {
    /// Manage login.
    #[cfg(feature = "login")]
    Login(LoginCommand),

    /// Remove stored authentication credentials.
    #[cfg(feature = "login")]
    Logout(LogoutCommand),

    /// Run commands within a Nori-provided sandbox.
    #[clap(visible_alias = "debug")]
    Sandbox(SandboxArgs),

    /// Manage skillsets. An alias for `npx nori-skillsets` or `bunx nori-skillsets`.
    Skillsets(SkillsetsCommand),

    /// Execpolicy tooling.
    #[clap(hide = true)]
    Execpolicy(ExecpolicyCommand),

    /// Internal: relay stdio to a Unix domain socket.
    #[clap(hide = true, name = "stdio-to-uds")]
    StdioToUds(StdioToUdsCommand),

    /// Generate shell completion scripts.
    Completions(CompletionsCommand),

    /// Resume a previous interactive session (picker by default; use --last to continue the most recent).
    Resume(ResumeCommand),
}

#[derive(Debug, Parser)]
struct CompletionsCommand {
    /// The shell to generate completions for.
    shell: clap_complete::Shell,
}

#[derive(Debug, Parser)]
struct ResumeCommand {
    /// Session id (UUID). If omitted, use --last or choose from the picker.
    #[arg(value_name = "SESSION_ID")]
    session_id: Option<String>,

    /// Continue the most recent session without showing the picker.
    #[arg(long = "last", default_value_t = false)]
    last: bool,

    /// Show all sessions instead of filtering to the current working directory.
    #[arg(long = "all", default_value_t = false)]
    all: bool,

    #[clap(flatten)]
    config_overrides: TuiCli,
}

#[derive(Debug, Parser)]
struct SandboxArgs {
    #[command(subcommand)]
    cmd: SandboxCommand,
}

#[derive(Debug, clap::Subcommand)]
enum SandboxCommand {
    /// Run a command under Seatbelt (macOS only).
    #[clap(visible_alias = "seatbelt")]
    Macos(SeatbeltCommand),

    /// Run a command under Landlock+seccomp (Linux only).
    #[clap(visible_alias = "landlock")]
    Linux(LandlockCommand),

    /// Run a command under Windows restricted token (Windows only).
    Windows(WindowsCommand),
}

#[derive(Debug, Parser)]
struct ExecpolicyCommand {
    #[command(subcommand)]
    sub: ExecpolicySubcommand,
}

#[derive(Debug, clap::Subcommand)]
enum ExecpolicySubcommand {
    /// Check execpolicy files against a command.
    #[clap(name = "check")]
    Check(ExecPolicyCheckCommand),
}

#[cfg(feature = "login")]
#[derive(Debug, Parser)]
struct LoginCommand {
    #[clap(skip)]
    config_overrides: CliConfigOverrides,

    #[arg(
        long = "with-api-key",
        help = "Read the API key from stdin (e.g. `printenv OPENAI_API_KEY | nori login --with-api-key`)"
    )]
    with_api_key: bool,

    #[arg(
        long = "api-key",
        value_name = "API_KEY",
        help = "(deprecated) Previously accepted the API key directly; now exits with guidance to use --with-api-key",
        hide = true
    )]
    api_key: Option<String>,

    #[arg(long = "device-auth")]
    use_device_code: bool,

    /// EXPERIMENTAL: Use custom OAuth issuer base URL (advanced)
    /// Override the OAuth issuer base URL (advanced)
    #[arg(long = "experimental_issuer", value_name = "URL", hide = true)]
    issuer_base_url: Option<String>,

    /// EXPERIMENTAL: Use custom OAuth client ID (advanced)
    #[arg(long = "experimental_client-id", value_name = "CLIENT_ID", hide = true)]
    client_id: Option<String>,

    #[command(subcommand)]
    action: Option<LoginSubcommand>,
}

#[cfg(feature = "login")]
#[derive(Debug, clap::Subcommand)]
enum LoginSubcommand {
    /// Show login status.
    Status,
}

#[cfg(feature = "login")]
#[derive(Debug, Parser)]
struct LogoutCommand {
    #[clap(skip)]
    config_overrides: CliConfigOverrides,
}

#[derive(Debug, Parser)]
struct StdioToUdsCommand {
    /// Path to the Unix domain socket to connect to.
    #[arg(value_name = "SOCKET_PATH")]
    socket_path: PathBuf,
}

#[derive(Debug, Parser)]
#[clap(disable_help_flag = true)]
struct SkillsetsCommand {
    /// Arguments to pass to nori-skillsets.
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    args: Vec<String>,
}

fn format_exit_messages(exit_info: AppExitInfo, color_enabled: bool) -> Vec<String> {
    let AppExitInfo {
        token_usage,
        conversation_id,
        ..
    } = exit_info;

    if token_usage.is_zero() {
        return Vec::new();
    }

    let mut lines = vec![format!(
        "{}",
        codex_core::protocol::FinalOutput::from(token_usage)
    )];

    if let Some(session_id) = conversation_id {
        let resume_cmd = format!("nori resume {session_id}");
        let command = if color_enabled {
            resume_cmd.cyan().to_string()
        } else {
            resume_cmd
        };
        lines.push(format!("To continue this session, run {command}"));
    }

    lines
}

/// Handle the app exit and print the results. Optionally run the update action.
fn handle_app_exit(exit_info: AppExitInfo) -> anyhow::Result<()> {
    let update_action = exit_info.update_action;
    let color_enabled = supports_color::on(Stream::Stdout).is_some();
    for line in format_exit_messages(exit_info, color_enabled) {
        println!("{line}");
    }
    if let Some(action) = update_action {
        run_update_action(action)?;
    }
    Ok(())
}

/// Run the update action and print the result.
fn run_update_action(action: UpdateAction) -> anyhow::Result<()> {
    println!();
    let cmd_str = action.command_str();
    println!("Updating Nori via `{cmd_str}`...");

    let status = {
        #[cfg(windows)]
        {
            // On Windows, run via cmd.exe so .CMD/.BAT are correctly resolved (PATHEXT semantics).
            std::process::Command::new("cmd")
                .args(["/C", &cmd_str])
                .status()?
        }
        #[cfg(not(windows))]
        {
            let (cmd, args) = action.command_args();
            let command_path = crate::wsl_paths::normalize_for_wsl(cmd);
            let normalized_args: Vec<String> = args
                .iter()
                .map(crate::wsl_paths::normalize_for_wsl)
                .collect();
            std::process::Command::new(&command_path)
                .args(&normalized_args)
                .status()?
        }
    };
    if !status.success() {
        anyhow::bail!("`{cmd_str}` failed with status {status}");
    }
    println!();
    println!("Update ran successfully! Please restart Nori.");
    Ok(())
}

fn run_execpolicycheck(cmd: ExecPolicyCheckCommand) -> anyhow::Result<()> {
    cmd.run()
}

fn run_skillsets_command(cmd: SkillsetsCommand) -> anyhow::Result<()> {
    const NORI_SKILLSETS_CMD: &str = "nori-skillsets";

    // First, check if nori-skillsets is available directly in PATH
    let status = if let Ok(skillsets_path) = which::which(NORI_SKILLSETS_CMD) {
        #[cfg(windows)]
        {
            // On Windows, run via cmd.exe so .CMD/.BAT are correctly resolved (PATHEXT semantics).
            let mut cmd_args = vec!["/C".to_string(), skillsets_path.display().to_string()];
            cmd_args.extend(cmd.args.iter().cloned());
            std::process::Command::new("cmd").args(&cmd_args).status()?
        }
        #[cfg(not(windows))]
        {
            std::process::Command::new(&skillsets_path)
                .args(&cmd.args)
                .status()?
        }
    } else {
        // Fall back to npx/bunx if not in PATH
        use nori_acp::registry::detect_preferred_package_manager;

        let package_manager = detect_preferred_package_manager();
        let runner = package_manager.command(); // "npx" or "bunx"

        #[cfg(windows)]
        {
            let mut cmd_args = vec!["/C", runner, NORI_SKILLSETS_CMD];
            cmd_args.extend(cmd.args.iter().map(String::as_str));
            std::process::Command::new("cmd").args(&cmd_args).status()?
        }
        #[cfg(not(windows))]
        {
            let command_path = crate::wsl_paths::normalize_for_wsl(runner);
            std::process::Command::new(&command_path)
                .arg(NORI_SKILLSETS_CMD)
                .args(&cmd.args)
                .status()?
        }
    };

    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }
    Ok(())
}

/// As early as possible in the process lifecycle, apply hardening measures. We
/// skip this in debug builds to avoid interfering with debugging.
#[ctor::ctor]
#[cfg(not(debug_assertions))]
fn pre_main_hardening() {
    codex_process_hardening::pre_main_hardening();
}

fn main() -> anyhow::Result<()> {
    arg0_dispatch_or_else(|codex_linux_sandbox_exe| async move {
        cli_main(codex_linux_sandbox_exe).await?;
        Ok(())
    })
}

async fn cli_main(codex_linux_sandbox_exe: Option<PathBuf>) -> anyhow::Result<()> {
    let MultitoolCli {
        config_overrides: root_config_overrides,
        mut interactive,
        subcommand,
    } = MultitoolCli::parse();

    // Set up CODEX_HOME to point to NORI_HOME so all codex-core config loading
    // uses ~/.nori/cli/ instead of ~/.codex. This must happen early, before any
    // subcommand dispatch or config loading. Only set if not already defined,
    // to allow tests and users to override via environment variable.
    if std::env::var("CODEX_HOME").is_err()
        && let Ok(nori_home) = find_nori_home()
    {
        // Create the directory if it doesn't exist
        if let Err(e) = std::fs::create_dir_all(&nori_home) {
            eprintln!(
                "Warning: Failed to create Nori config directory '{}': {e}",
                nori_home.display()
            );
        }
        // SAFETY: Called early in main before spawning threads
        unsafe {
            std::env::set_var("CODEX_HOME", &nori_home);
        }
    }

    // Initialize ACP rolling file tracing in $NORI_HOME/log/ (non-critical, log warning on failure)
    // Logs are stored as daily rolling files like: ~/.nori/cli/log/nori-acp.2024-01-15.log
    if let Ok(nori_home) = find_nori_home() {
        let log_dir = nori_home.join("log");
        if let Err(e) = init_rolling_file_tracing(&log_dir, "nori-acp") {
            eprintln!("Warning: Failed to initialize ACP file tracing: {e}");
        }
    }

    match subcommand {
        None => {
            prepend_config_flags(
                &mut interactive.config_overrides,
                root_config_overrides.clone(),
            );
            let exit_info = nori_tui::run_main(interactive, codex_linux_sandbox_exe).await?;
            handle_app_exit(exit_info)?;
        }
        #[cfg(feature = "login")]
        Some(Subcommand::Login(mut login_cli)) => {
            prepend_config_flags(
                &mut login_cli.config_overrides,
                root_config_overrides.clone(),
            );
            match login_cli.action {
                Some(LoginSubcommand::Status) => {
                    run_login_status(login_cli.config_overrides).await;
                }
                None => {
                    if login_cli.use_device_code {
                        run_login_with_device_code(
                            login_cli.config_overrides,
                            login_cli.issuer_base_url,
                            login_cli.client_id,
                        )
                        .await;
                    } else if login_cli.api_key.is_some() {
                        eprintln!(
                            "The --api-key flag is no longer supported. Pipe the key instead, e.g. `printenv OPENAI_API_KEY | nori login --with-api-key`."
                        );
                        std::process::exit(1);
                    } else if login_cli.with_api_key {
                        let api_key = read_api_key_from_stdin();
                        run_login_with_api_key(login_cli.config_overrides, api_key).await;
                    } else {
                        run_login_with_chatgpt(login_cli.config_overrides).await;
                    }
                }
            }
        }
        #[cfg(feature = "login")]
        Some(Subcommand::Logout(mut logout_cli)) => {
            prepend_config_flags(
                &mut logout_cli.config_overrides,
                root_config_overrides.clone(),
            );
            run_logout(logout_cli.config_overrides).await;
        }
        Some(Subcommand::Sandbox(sandbox_args)) => match sandbox_args.cmd {
            SandboxCommand::Macos(mut seatbelt_cli) => {
                prepend_config_flags(
                    &mut seatbelt_cli.config_overrides,
                    root_config_overrides.clone(),
                );
                nori_cli::debug_sandbox::run_command_under_seatbelt(
                    seatbelt_cli,
                    codex_linux_sandbox_exe,
                )
                .await?;
            }
            SandboxCommand::Linux(mut landlock_cli) => {
                prepend_config_flags(
                    &mut landlock_cli.config_overrides,
                    root_config_overrides.clone(),
                );
                nori_cli::debug_sandbox::run_command_under_landlock(
                    landlock_cli,
                    codex_linux_sandbox_exe,
                )
                .await?;
            }
            SandboxCommand::Windows(mut windows_cli) => {
                prepend_config_flags(
                    &mut windows_cli.config_overrides,
                    root_config_overrides.clone(),
                );
                nori_cli::debug_sandbox::run_command_under_windows(
                    windows_cli,
                    codex_linux_sandbox_exe,
                )
                .await?;
            }
        },
        Some(Subcommand::Skillsets(cmd)) => {
            run_skillsets_command(cmd)?;
        }
        Some(Subcommand::Resume(ResumeCommand {
            session_id,
            last,
            all,
            config_overrides,
        })) => {
            interactive = finalize_resume_interactive(
                interactive,
                root_config_overrides.clone(),
                session_id,
                last,
                all,
                config_overrides,
            );
            let exit_info = nori_tui::run_main(interactive, codex_linux_sandbox_exe).await?;
            handle_app_exit(exit_info)?;
        }
        Some(Subcommand::Execpolicy(ExecpolicyCommand { sub })) => match sub {
            ExecpolicySubcommand::Check(cmd) => run_execpolicycheck(cmd)?,
        },
        Some(Subcommand::StdioToUds(cmd)) => {
            let socket_path = cmd.socket_path;
            tokio::task::spawn_blocking(move || codex_stdio_to_uds::run(socket_path.as_path()))
                .await??;
        }
        Some(Subcommand::Completions(cmd)) => {
            clap_complete::generate(
                cmd.shell,
                &mut MultitoolCli::command(),
                "nori",
                &mut std::io::stdout(),
            );
        }
    }

    Ok(())
}

/// Prepend root-level overrides so they have lower precedence than
/// CLI-specific ones specified after the subcommand (if any).
fn prepend_config_flags(
    subcommand_config_overrides: &mut CliConfigOverrides,
    cli_config_overrides: CliConfigOverrides,
) {
    subcommand_config_overrides
        .raw_overrides
        .splice(0..0, cli_config_overrides.raw_overrides);
}

fn finalize_resume_interactive(
    mut interactive: TuiCli,
    root_config_overrides: CliConfigOverrides,
    session_id: Option<String>,
    last: bool,
    show_all: bool,
    resume_cli: TuiCli,
) -> TuiCli {
    interactive.resume_picker = session_id.is_none() && !last;
    interactive.resume_last = last;
    interactive.resume_session_id = session_id;
    interactive.resume_show_all = show_all;
    merge_interactive_cli_flags(&mut interactive, resume_cli);
    prepend_config_flags(&mut interactive.config_overrides, root_config_overrides);
    interactive
}

fn merge_interactive_cli_flags(interactive: &mut TuiCli, subcommand_cli: TuiCli) {
    if let Some(prompt) = subcommand_cli.prompt {
        interactive.prompt = Some(prompt.replace("\r\n", "\n").replace('\r', "\n"));
    }
    if !subcommand_cli.images.is_empty() {
        interactive.images = subcommand_cli.images;
    }
    if let Some(agent) = subcommand_cli.agent {
        interactive.agent = Some(agent);
    }
    if let Some(profile) = subcommand_cli.config_profile {
        interactive.config_profile = Some(profile);
    }
    if subcommand_cli.dangerously_bypass_approvals_and_sandbox {
        interactive.dangerously_bypass_approvals_and_sandbox = true;
    }
    if let Some(cwd) = subcommand_cli.cwd {
        interactive.cwd = Some(cwd);
    }
    if !subcommand_cli.add_dir.is_empty() {
        interactive.add_dir.extend(subcommand_cli.add_dir);
    }
    if subcommand_cli.skip_welcome {
        interactive.skip_welcome = true;
    }
    if subcommand_cli.skip_trust_directory {
        interactive.skip_trust_directory = true;
    }
    interactive
        .config_overrides
        .raw_overrides
        .extend(subcommand_cli.config_overrides.raw_overrides);
}

#[cfg(test)]
mod tests {
    use super::*;
    use codex_core::protocol::TokenUsage;
    use codex_protocol::ConversationId;
    use pretty_assertions::assert_eq;

    fn finalize_resume_from_args(args: &[&str]) -> TuiCli {
        let cli = MultitoolCli::try_parse_from(args).expect("parse");
        let MultitoolCli {
            interactive,
            config_overrides: root_overrides,
            subcommand,
        } = cli;

        let Subcommand::Resume(ResumeCommand {
            session_id,
            last,
            all,
            config_overrides: resume_cli,
        }) = subcommand.expect("resume present")
        else {
            unreachable!()
        };

        finalize_resume_interactive(
            interactive,
            root_overrides,
            session_id,
            last,
            all,
            resume_cli,
        )
    }

    fn sample_exit_info(conversation: Option<&str>) -> AppExitInfo {
        let token_usage = TokenUsage {
            output_tokens: 2,
            total_tokens: 2,
            ..Default::default()
        };
        AppExitInfo {
            token_usage,
            conversation_id: conversation
                .map(ConversationId::from_string)
                .map(Result::unwrap),
            update_action: None,
        }
    }

    #[test]
    fn format_exit_messages_skips_zero_usage() {
        let exit_info = AppExitInfo {
            token_usage: TokenUsage::default(),
            conversation_id: None,
            update_action: None,
        };
        let lines = format_exit_messages(exit_info, false);
        assert!(lines.is_empty());
    }

    #[test]
    fn format_exit_messages_includes_resume_hint_without_color() {
        let exit_info = sample_exit_info(Some("123e4567-e89b-12d3-a456-426614174000"));
        let lines = format_exit_messages(exit_info, false);
        assert_eq!(
            lines,
            vec![
                "Token usage: total=2 input=0 output=2".to_string(),
                "To continue this session, run nori resume 123e4567-e89b-12d3-a456-426614174000"
                    .to_string(),
            ]
        );
    }

    #[test]
    fn format_exit_messages_applies_color_when_enabled() {
        let exit_info = sample_exit_info(Some("123e4567-e89b-12d3-a456-426614174000"));
        let lines = format_exit_messages(exit_info, true);
        assert_eq!(lines.len(), 2);
        assert!(lines[1].contains("\u{1b}[36m"));
    }

    #[test]
    fn resume_picker_logic_none_and_not_last() {
        let interactive = finalize_resume_from_args(["nori", "resume"].as_ref());
        assert!(interactive.resume_picker);
        assert!(!interactive.resume_last);
        assert_eq!(interactive.resume_session_id, None);
        assert!(!interactive.resume_show_all);
    }

    #[test]
    fn resume_picker_logic_last() {
        let interactive = finalize_resume_from_args(["nori", "resume", "--last"].as_ref());
        assert!(!interactive.resume_picker);
        assert!(interactive.resume_last);
        assert_eq!(interactive.resume_session_id, None);
    }

    #[test]
    fn resume_picker_logic_with_session_id() {
        let interactive = finalize_resume_from_args(["nori", "resume", "session-123"].as_ref());
        assert!(!interactive.resume_picker);
        assert!(!interactive.resume_last);
        assert_eq!(
            interactive.resume_session_id.as_deref(),
            Some("session-123")
        );
    }

    #[test]
    fn resume_all_flag_sets_show_all() {
        let interactive = finalize_resume_from_args(["nori", "resume", "--all"].as_ref());
        assert!(interactive.resume_picker);
        assert!(interactive.resume_show_all);
    }

    #[test]
    fn resume_merges_resume_scoped_interactive_flags() {
        let interactive = finalize_resume_from_args(
            [
                "nori",
                "resume",
                "session-123",
                "--agent",
                "codex",
                "--dangerously-bypass-approvals-and-sandbox",
                "-C",
                "/tmp",
                "-i",
                "/tmp/a.png,/tmp/b.png",
                "--skip-welcome",
                "--skip-trust-directory",
            ]
            .as_ref(),
        );

        assert_eq!(interactive.agent.as_deref(), Some("codex"));
        assert!(interactive.dangerously_bypass_approvals_and_sandbox);
        assert_eq!(
            interactive.cwd.as_deref(),
            Some(std::path::Path::new("/tmp"))
        );
        assert!(
            interactive
                .images
                .iter()
                .any(|path| path == std::path::Path::new("/tmp/a.png"))
        );
        assert!(
            interactive
                .images
                .iter()
                .any(|path| path == std::path::Path::new("/tmp/b.png"))
        );
        assert!(interactive.skip_welcome);
        assert!(interactive.skip_trust_directory);
    }

    /// Binary name should be "nori" in help output
    #[test]
    fn binary_name_is_nori() {
        let help = MultitoolCli::command().render_help().to_string();
        assert!(
            help.contains("nori [OPTIONS]"),
            "Help should show 'nori' as binary name, got: {help}"
        );
        assert!(
            !help.contains("codex [OPTIONS]"),
            "Help should not show 'codex' as binary name"
        );
    }

    /// Config path example should reference ~/.nori/cli/ not ~/.codex/
    #[test]
    fn config_help_references_nori_path() {
        let help = MultitoolCli::command().render_help().to_string();
        assert!(
            help.contains("~/.nori/cli/config.toml"),
            "Help should reference ~/.nori/cli/config.toml, got: {help}"
        );
        assert!(
            !help.contains("~/.codex/config.toml"),
            "Help should not reference ~/.codex/config.toml"
        );
    }

    /// Config example should show agent="claude-code" not model="o3"
    #[test]
    fn config_example_shows_agent_claude_code() {
        let help = MultitoolCli::command().render_long_help().to_string();
        assert!(
            help.contains("agent=\"claude-code\""),
            "Help should show agent=\"claude-code\" example, got: {help}"
        );
        assert!(
            !help.contains("model=\"o3\""),
            "Help should not show model=\"o3\" example"
        );
    }

    /// The completions subcommand should appear in help output
    #[test]
    fn completions_subcommand_in_help() {
        let help = MultitoolCli::command().render_help().to_string();
        assert!(
            help.contains("completions"),
            "Help should show 'completions' subcommand, got: {help}"
        );
    }

    /// "completions bash" should be parsed as the Completions subcommand, not a prompt
    #[test]
    fn completions_parsed_as_subcommand() {
        let cli =
            MultitoolCli::try_parse_from(["nori", "completions", "bash"]).expect("should parse");
        assert!(
            matches!(cli.subcommand, Some(Subcommand::Completions(_))),
            "completions should be parsed as subcommand, got: {:?}",
            cli.subcommand
        );
        assert!(
            cli.interactive.prompt.is_none(),
            "prompt should be None when completions subcommand is used"
        );
    }

    /// "completions" with no shell argument should produce an error
    #[test]
    fn completions_requires_shell_argument() {
        let result = MultitoolCli::try_parse_from(["nori", "completions"]);
        assert!(
            result.is_err(),
            "completions without a shell argument should fail"
        );
    }

    /// completions generates non-empty output containing "nori" for each supported shell
    #[test]
    fn completions_generates_valid_output_for_all_shells() {
        use clap_complete::Shell;

        let shells = [
            Shell::Bash,
            Shell::Zsh,
            Shell::Fish,
            Shell::PowerShell,
            Shell::Elvish,
        ];

        for shell in shells {
            let mut buf = Vec::new();
            clap_complete::generate(shell, &mut MultitoolCli::command(), "nori", &mut buf);
            let output = String::from_utf8(buf).expect("completion output should be valid UTF-8");
            assert!(
                !output.is_empty(),
                "completion output for {shell:?} should not be empty"
            );
            assert!(
                output.contains("nori"),
                "completion output for {shell:?} should contain 'nori', got: {output}"
            );
        }
    }

    /// "completion" (singular) should still be treated as a prompt, not a subcommand
    #[test]
    fn completion_singular_treated_as_prompt() {
        let cli = MultitoolCli::try_parse_from(["nori", "completion"]).expect("should parse");
        assert!(
            cli.subcommand.is_none(),
            "singular 'completion' should not be parsed as subcommand"
        );
        assert_eq!(
            cli.interactive.prompt.as_deref(),
            Some("completion"),
            "singular 'completion' should be parsed as prompt"
        );
    }

    /// "skillsets" should be recognized as a subcommand, not a prompt
    #[test]
    fn skillsets_subcommand_is_recognized() {
        let cli = MultitoolCli::try_parse_from(["nori", "skillsets"]).expect("should parse");
        assert!(
            matches!(cli.subcommand, Some(Subcommand::Skillsets(_))),
            "skillsets should be parsed as subcommand, got: {:?}",
            cli.subcommand
        );
    }

    /// "skillsets" subcommand should capture trailing arguments
    #[test]
    fn skillsets_subcommand_captures_trailing_args() {
        let cli =
            MultitoolCli::try_parse_from(["nori", "skillsets", "list-skillsets", "--verbose"])
                .expect("should parse");
        match cli.subcommand {
            Some(Subcommand::Skillsets(cmd)) => {
                assert_eq!(
                    cmd.args,
                    vec!["list-skillsets".to_string(), "--verbose".to_string()],
                    "should capture trailing args"
                );
            }
            _ => panic!("expected Skillsets subcommand"),
        }
    }

    /// "skillsets -h" should pass -h to nori-skillsets, not show clap help
    #[test]
    fn skillsets_subcommand_passes_help_flag_through() {
        let cli = MultitoolCli::try_parse_from(["nori", "skillsets", "-h"]).expect("should parse");
        match cli.subcommand {
            Some(Subcommand::Skillsets(cmd)) => {
                assert_eq!(
                    cmd.args,
                    vec!["-h".to_string()],
                    "-h should be passed through to nori-skillsets"
                );
            }
            _ => panic!("expected Skillsets subcommand"),
        }
    }
}
