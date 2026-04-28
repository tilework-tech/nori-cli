use clap::Parser;
use clap::ValueHint;
use codex_common::CliConfigOverrides;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(version)]
pub struct Cli {
    /// Optional user prompt to start the session.
    #[arg(value_name = "PROMPT", value_hint = clap::ValueHint::Other)]
    pub prompt: Option<String>,

    /// Optional image(s) to attach to the initial prompt.
    #[arg(long = "image", short = 'i', value_name = "FILE", value_delimiter = ',', num_args = 1..)]
    pub images: Vec<PathBuf>,

    // Internal controls set by the top-level `nori resume` subcommand.
    // These are not exposed as user flags on the base `nori` command.
    #[clap(skip)]
    pub resume_picker: bool,

    #[clap(skip)]
    pub resume_last: bool,

    /// Internal: resume a specific recorded session by id (UUID). Set by the
    /// top-level `nori resume <SESSION_ID>` wrapper; not exposed as a public flag.
    #[clap(skip)]
    pub resume_session_id: Option<String>,

    /// Internal: show all sessions (disables cwd filtering and shows CWD column).
    #[clap(skip)]
    pub resume_show_all: bool,

    /// Agent the CLI should use (e.g., "claude-code", "gemini", "codex").
    #[arg(long, short = 'a')]
    pub agent: Option<String>,

    /// Configuration profile from config.toml to specify default options.
    #[arg(long = "profile", short = 'p')]
    pub config_profile: Option<String>,

    /// Skip all confirmation prompts and execute commands without sandboxing.
    /// EXTREMELY DANGEROUS. Intended solely for running in environments that are externally sandboxed.
    #[arg(
        long = "dangerously-bypass-approvals-and-sandbox",
        alias = "yolo",
        default_value_t = false
    )]
    pub dangerously_bypass_approvals_and_sandbox: bool,

    /// Tell the agent to use the specified directory as its working root.
    #[clap(long = "cd", short = 'C', value_name = "DIR")]
    pub cwd: Option<PathBuf>,

    /// Additional directories that should be writable alongside the primary workspace.
    #[arg(long = "add-dir", value_name = "DIR", value_hint = ValueHint::DirPath)]
    pub add_dir: Vec<PathBuf>,

    #[clap(skip)]
    pub config_overrides: CliConfigOverrides,

    /// Skip the first-launch welcome screen.
    /// Use this flag to bypass the initial Nori welcome message on first launch.
    #[arg(long = "skip-welcome", default_value_t = false)]
    pub skip_welcome: bool,

    /// Skip the trust directory prompt for untrusted directories.
    /// When set, automatically trusts the current directory without prompting.
    /// Intended for testing and automation scenarios.
    #[arg(long = "skip-trust-directory", default_value_t = false)]
    pub skip_trust_directory: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    /// Test that --yolo flag is recognized and sets dangerously_bypass_approvals_and_sandbox to true.
    #[test]
    fn test_yolo_flag_is_recognized() {
        let cli = Cli::try_parse_from(["nori", "--yolo"]).expect("--yolo should be a valid flag");
        assert!(
            cli.dangerously_bypass_approvals_and_sandbox,
            "--yolo should set dangerously_bypass_approvals_and_sandbox to true"
        );
    }

    /// Test that --dangerously-bypass-approvals-and-sandbox works as the full flag name.
    #[test]
    fn test_dangerously_bypass_flag_is_recognized() {
        let cli = Cli::try_parse_from(["nori", "--dangerously-bypass-approvals-and-sandbox"])
            .expect("--dangerously-bypass-approvals-and-sandbox should be a valid flag");
        assert!(
            cli.dangerously_bypass_approvals_and_sandbox,
            "--dangerously-bypass-approvals-and-sandbox should set the field to true"
        );
    }

    /// Test that without --yolo, the field defaults to false.
    #[test]
    fn test_yolo_flag_defaults_to_false() {
        let cli = Cli::try_parse_from(["nori"]).expect("basic parsing should work");
        assert!(
            !cli.dangerously_bypass_approvals_and_sandbox,
            "dangerously_bypass_approvals_and_sandbox should default to false"
        );
    }
}
