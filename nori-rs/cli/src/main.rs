use clap::CommandFactory;
use clap::Parser;
use codex_arg0::arg0_dispatch_or_else;
use codex_common::CliConfigOverrides;
use codex_execpolicy::ExecPolicyCheckCommand;
use nori_cli::LandlockCommand;
use nori_cli::SeatbeltCommand;
use nori_cli::WindowsCommand;
use nori_config::AskForApproval;
use nori_config::NoriConfigOverrides;
use nori_config::SandboxMode;
use nori_config::find_nori_home;
use nori_harness::init_rolling_file_tracing;

use nori_tui::AppExitInfo;
use nori_tui::Cli as TuiCli;
use nori_tui::RESUME_HINT_LEAD;
use nori_tui::resume_command_for_conversation;
use nori_tui::update_action::UpdateAction;
use owo_colors::OwoColorize;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;
use supports_color::Stream;

mod stdin_prompt;
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

    /// Print the response and exit instead of starting the terminal UI.
    /// Equivalent to `nori exec`.
    #[arg(short = 'p', long = "print", default_value_t = false)]
    print: bool,

    #[clap(flatten)]
    interactive: TuiCli,

    #[clap(subcommand)]
    subcommand: Option<Subcommand>,
}

#[derive(Debug, clap::Subcommand)]
enum Subcommand {
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

    /// Run a TUI session backed by a cloud VM via the nori-sessions broker.
    Cloud(CloudCommand),

    /// Run a prompt non-interactively or serve the ACP stdio facade.
    Exec(ExecCommand),
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
struct CloudCommand {
    #[clap(flatten)]
    config_overrides: TuiCli,
}

#[derive(Debug, Parser)]
struct ExecCommand {
    /// Prompt to send. Reads stdin when omitted, or when set to `-`.
    #[arg(value_name = "PROMPT", conflicts_with = "acp")]
    prompt: Option<String>,

    /// Read piped stdin and append it to PROMPT as context.
    #[arg(long = "stdin", default_value_t = false, conflicts_with = "acp")]
    stdin: bool,

    /// Serve a bounded ACP agent facade over stdio.
    #[arg(long)]
    acp: bool,

    /// ACP agent to run behind the facade.
    #[arg(long)]
    agent: Option<String>,

    /// Working directory for the execution.
    #[arg(short = 'C', long = "cwd", value_name = "DIR")]
    cwd: Option<PathBuf>,

    /// Disable approval prompts and sandboxing for ACP-visible operations.
    #[arg(long)]
    dangerously_bypass_approvals_and_sandbox: bool,

    #[clap(flatten)]
    config_overrides: CliConfigOverrides,
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
        conversation_has_activity,
        ..
    } = exit_info;

    let mut lines = Vec::new();
    if !token_usage.is_zero() {
        lines.push(token_usage.to_string());
    }

    if conversation_has_activity && let Some(session_id) = conversation_id {
        let resume_cmd = resume_command_for_conversation(&session_id);
        let command = if color_enabled {
            resume_cmd.cyan().to_string()
        } else {
            resume_cmd
        };
        lines.push(RESUME_HINT_LEAD.to_string());
        lines.push(command);
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
        use nori_harness::detect_preferred_package_manager;

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
        print,
        mut interactive,
        subcommand,
    } = MultitoolCli::parse();

    // Initialize ACP rolling file tracing in $NORI_HOME/log/ (non-critical, log warning on failure)
    // Logs are stored as daily rolling files like: ~/.nori/cli/log/nori-acp.2024-01-15.log
    if let Ok(nori_home) = find_nori_home() {
        let log_dir = nori_home.join("log");
        if let Err(e) = init_rolling_file_tracing(&log_dir, "nori-acp") {
            eprintln!("Warning: Failed to initialize ACP file tracing: {e}");
        }
    }

    // `-p` selects a mode, so a subcommand that selects a different one is a
    // contradiction. Falling through would silently drop `print` and open the
    // TUI for someone who asked for printed output.
    if print && subcommand.is_some() {
        anyhow::bail!("--print cannot be combined with a subcommand; use `nori exec` instead");
    }

    match subcommand {
        None if print => {
            // `run_exec` has no way to attach images, so accepting them here
            // would silently answer without the attachment.
            if !interactive.images.is_empty() {
                anyhow::bail!(
                    "--print does not support --image; use the interactive CLI to attach images"
                );
            }
            // `-p` is an alias for `exec`; the remaining flags are resolved from
            // `interactive` inside `run_exec`, which also ingests piped stdin.
            let cmd = ExecCommand {
                prompt: interactive.prompt.take(),
                stdin: interactive.stdin,
                acp: false,
                agent: None,
                cwd: None,
                dangerously_bypass_approvals_and_sandbox: false,
                config_overrides: CliConfigOverrides::default(),
            };
            run_exec(
                cmd,
                interactive,
                root_config_overrides,
                env!("CARGO_PKG_VERSION").to_string(),
            )
            .await?;
        }
        None => {
            prepend_config_flags(
                &mut interactive.config_overrides,
                root_config_overrides.clone(),
            );
            // A piped prompt seeds the session; the TUI still starts normally so
            // the user keeps the conversation going after the first turn.
            let prompt = interactive.prompt.take();
            let consumed_stdin = prompt.is_none() || interactive.stdin;
            interactive.prompt = stdin_prompt::resolve_prompt(prompt, interactive.stdin)?;
            if consumed_stdin {
                stdin_prompt::restore_stdin_from_terminal();
            }
            let exit_info = nori_tui::run_main(interactive, codex_linux_sandbox_exe).await?;
            handle_app_exit(exit_info)?;
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
        Some(Subcommand::Cloud(cloud_cmd)) => {
            // All Sessions concerns (auth, broker, transport) live in
            // nori-handroll; the CLI only pins the TUI's agent to a
            // `nori-handroll cloud-acp` child. Fail before the TUI starts if
            // the binary is missing.
            let handroll_bin = nori_cli::cloud::resolve_handroll_bin(
                std::env::var_os("NORI_HANDROLL_BIN"),
                std::env::var_os("PATH"),
            )?;
            let nori_config = nori_config::NoriConfig::load().unwrap_or_default();

            merge_interactive_cli_flags(&mut interactive, cloud_cmd.config_overrides);
            prepend_config_flags(
                &mut interactive.config_overrides,
                root_config_overrides.clone(),
            );
            // Cloud mode always uses the handroll adapter — `--agent` cannot
            // bypass Sessions.
            interactive.agent = Some(nori_cli::cloud::CLOUD_AGENT_SLUG.to_string());
            interactive.extra_agents = vec![nori_cli::cloud::cloud_agent_config(
                &handroll_bin,
                nori_config.cloud_broker_url.as_deref(),
            )];
            // Cloud entry is picker-first: list live sessions before
            // anything can claim a VM; creating one is an explicit pick.
            interactive.cloud_mode = true;

            let exit_info = nori_tui::run_main(interactive, codex_linux_sandbox_exe).await?;
            handle_app_exit(exit_info)?;
        }
        Some(Subcommand::Exec(cmd)) => {
            run_exec(
                cmd,
                interactive,
                root_config_overrides,
                env!("CARGO_PKG_VERSION").to_string(),
            )
            .await?;
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

async fn run_exec(
    cmd: ExecCommand,
    interactive: TuiCli,
    root_config_overrides: CliConfigOverrides,
    cli_version: String,
) -> anyhow::Result<()> {
    let ExecCommand {
        prompt,
        stdin,
        acp,
        agent,
        cwd,
        dangerously_bypass_approvals_and_sandbox,
        config_overrides,
    } = cmd;

    let mut raw_overrides = root_config_overrides.raw_overrides;
    raw_overrides.extend(interactive.config_overrides.raw_overrides);
    raw_overrides.extend(config_overrides.raw_overrides);
    let raw_overrides = CliConfigOverrides { raw_overrides }
        .parse_overrides()
        .map_err(anyhow::Error::msg)?;

    let bypass = dangerously_bypass_approvals_and_sandbox
        || interactive.dangerously_bypass_approvals_and_sandbox;
    let (sandbox_mode, approval_policy) = if bypass {
        (
            Some(SandboxMode::DangerFullAccess),
            Some(AskForApproval::Never),
        )
    } else {
        (None, Some(AskForApproval::OnRequest))
    };
    let cwd = cwd
        .or(interactive.cwd)
        .map(|path| path.canonicalize().unwrap_or(path));
    let overrides = NoriConfigOverrides {
        agent: agent.or(interactive.agent),
        sandbox_mode,
        approval_policy,
        cwd,
        additional_writable_roots: interactive.add_dir,
        raw_overrides,
    };
    let config = nori_config::NoriConfig::load_with_overrides(overrides)?;
    nori_harness::initialize_registry(config.agents.clone())
        .map_err(|error| anyhow::anyhow!("failed to initialize agent registry: {error}"))?;
    let config = Arc::new(config);

    if acp {
        return nori_exec::run_acp(config, cli_version).await;
    }

    // Read stdin only after the ACP facade has had its chance to claim it.
    let Some(prompt) = stdin_prompt::resolve_prompt(prompt, stdin)? else {
        anyhow::bail!("a prompt argument or piped stdin is required");
    };
    let outcome = nori_exec::run_plaintext(config, cli_version, prompt).await?;
    {
        let mut stdout = std::io::stdout().lock();
        stdout.write_all(outcome.output.as_bytes())?;
        if !outcome.output.ends_with('\n') {
            stdout.write_all(b"\n")?;
        }
        stdout.flush()?;
    }
    if outcome.permission_denied {
        anyhow::bail!("permission request was rejected during unattended execution");
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
    use nori_harness::ConversationId;
    use nori_tui::TokenUsage;
    use pretty_assertions::assert_eq;

    fn finalize_resume_from_args(args: &[&str]) -> TuiCli {
        let cli = MultitoolCli::try_parse_from(args).expect("parse");
        let MultitoolCli {
            interactive,
            config_overrides: root_overrides,
            print: _,
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
            conversation_has_activity: conversation.is_some(),
            update_action: None,
        }
    }

    #[test]
    fn format_exit_messages_skips_zero_usage() {
        let exit_info = AppExitInfo {
            token_usage: TokenUsage::default(),
            conversation_id: None,
            conversation_has_activity: false,
            update_action: None,
        };
        let lines = format_exit_messages(exit_info, false);
        assert!(lines.is_empty());
    }

    #[test]
    fn format_exit_messages_includes_resume_hint_without_token_usage() {
        let exit_info = AppExitInfo {
            token_usage: TokenUsage::default(),
            conversation_id: Some(
                ConversationId::from_string("123e4567-e89b-12d3-a456-426614174000").unwrap(),
            ),
            conversation_has_activity: true,
            update_action: None,
        };
        let lines = format_exit_messages(exit_info, false);
        assert_eq!(
            lines,
            vec![
                "To continue this session, run:".to_string(),
                "nori resume 123e4567-e89b-12d3-a456-426614174000".to_string(),
            ]
        );
    }

    #[test]
    fn format_exit_messages_skips_resume_hint_without_activity() {
        let exit_info = AppExitInfo {
            token_usage: TokenUsage::default(),
            conversation_id: Some(
                ConversationId::from_string("123e4567-e89b-12d3-a456-426614174000").unwrap(),
            ),
            conversation_has_activity: false,
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
                "To continue this session, run:".to_string(),
                "nori resume 123e4567-e89b-12d3-a456-426614174000".to_string(),
            ]
        );
    }

    #[test]
    fn format_exit_messages_applies_color_when_enabled() {
        let exit_info = sample_exit_info(Some("123e4567-e89b-12d3-a456-426614174000"));
        let lines = format_exit_messages(exit_info, true);
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[1], "To continue this session, run:");
        assert!(lines[2].contains("\u{1b}[36m"));
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

    fn finalize_cloud_from_args(args: &[&str]) -> TuiCli {
        let cli = MultitoolCli::try_parse_from(args).expect("parse");
        let MultitoolCli {
            mut interactive,
            config_overrides: root_overrides,
            print: _,
            subcommand,
        } = cli;

        let Subcommand::Cloud(cloud_cmd) = subcommand.expect("cloud present") else {
            unreachable!()
        };

        merge_interactive_cli_flags(&mut interactive, cloud_cmd.config_overrides);
        prepend_config_flags(&mut interactive.config_overrides, root_overrides);
        interactive
    }

    #[test]
    fn cloud_preserves_root_level_flags() {
        let interactive = finalize_cloud_from_args(
            [
                "nori",
                "--skip-trust-directory",
                "cloud",
                "--agent",
                "codex",
            ]
            .as_ref(),
        );
        assert!(
            interactive.skip_trust_directory,
            "root --skip-trust-directory should be preserved through cloud subcommand"
        );
        assert_eq!(interactive.agent.as_deref(), Some("codex"));
    }

    #[test]
    fn cloud_merges_subcommand_flags_into_root() {
        let interactive = finalize_cloud_from_args(
            [
                "nori",
                "cloud",
                "--agent",
                "codex",
                "--dangerously-bypass-approvals-and-sandbox",
                "-C",
                "/tmp",
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
        assert!(interactive.skip_welcome);
        assert!(interactive.skip_trust_directory);
    }
}
