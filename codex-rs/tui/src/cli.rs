use clap::Parser;
use clap::ValueHint;
#[cfg(feature = "codex-features")]
use codex_common::ApprovalModeCliArg;
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

    // Internal controls set by the top-level `codex resume` subcommand.
    // These are not exposed as user flags on the base `codex` command.
    #[clap(skip)]
    pub resume_picker: bool,

    #[clap(skip)]
    pub resume_last: bool,

    /// Internal: resume a specific recorded session by id (UUID). Set by the
    /// top-level `codex resume <SESSION_ID>` wrapper; not exposed as a public flag.
    #[clap(skip)]
    pub resume_session_id: Option<String>,

    /// Internal: show all sessions (disables cwd filtering and shows CWD column).
    #[clap(skip)]
    pub resume_show_all: bool,

    /// Model the agent should use.
    #[arg(long, short = 'm')]
    pub model: Option<String>,

    /// Convenience flag to select the local open source model provider. Equivalent to -c
    /// model_provider=oss; verifies a local LM Studio or Ollama server is running.
    #[cfg(feature = "codex-features")]
    #[arg(long = "oss", default_value_t = false)]
    pub oss: bool,

    /// Specify which local provider to use (lmstudio or ollama).
    /// If not specified with --oss, will use config default or show selection.
    #[cfg(feature = "codex-features")]
    #[arg(long = "local-provider")]
    pub oss_provider: Option<String>,

    /// Configuration profile from config.toml to specify default options.
    #[arg(long = "profile", short = 'p')]
    pub config_profile: Option<String>,

    /// Select the sandbox policy to use when executing model-generated shell
    /// commands.
    #[cfg(feature = "codex-features")]
    #[arg(long = "sandbox", short = 's')]
    pub sandbox_mode: Option<codex_common::SandboxModeCliArg>,

    /// Configure when the model requires human approval before executing a command.
    #[cfg(feature = "codex-features")]
    #[arg(long = "ask-for-approval", short = 'a')]
    pub approval_policy: Option<ApprovalModeCliArg>,

    /// Convenience alias for low-friction sandboxed automatic execution (-a on-request, --sandbox workspace-write).
    #[cfg(feature = "codex-features")]
    #[arg(long = "full-auto", default_value_t = false)]
    pub full_auto: bool,

    /// Skip all confirmation prompts and execute commands without sandboxing.
    /// EXTREMELY DANGEROUS. Intended solely for running in environments that are externally sandboxed.
    #[cfg(feature = "codex-features")]
    #[arg(
        long = "dangerously-bypass-approvals-and-sandbox",
        alias = "yolo",
        default_value_t = false,
        conflicts_with_all = ["approval_policy", "full_auto"]
    )]
    pub dangerously_bypass_approvals_and_sandbox: bool,

    /// Tell the agent to use the specified directory as its working root.
    #[clap(long = "cd", short = 'C', value_name = "DIR")]
    pub cwd: Option<PathBuf>,

    /// Enable web search (off by default). When enabled, the native Responses `web_search` tool is available to the model (no per‑call approval).
    #[cfg(feature = "codex-features")]
    #[arg(long = "search", default_value_t = false)]
    pub web_search: bool,

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
