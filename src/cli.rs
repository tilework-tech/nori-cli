use clap::Parser;

#[derive(Parser, Debug, PartialEq)]
#[command(name = "nori-cli")]
#[command(about = "A TUI for interacting with AI agents", long_about = None)]
pub struct Cli {
    /// Select the agent to use (claude, claudecode, codex, mock)
    #[arg(short, long)]
    pub agent: Option<String>,

    /// Initial message to send (or read from stdin)
    pub message: Option<String>,
}

/// Map agent name to backend index
/// Returns None if agent name is invalid
pub fn agent_name_to_index(name: &str) -> Option<usize> {
    match name.to_lowercase().as_str() {
        "claude" => Some(0),
        "codex" => Some(1),
        "claudecode" => Some(2),
        "mock" => Some(3),
        _ => None,
    }
}

/// Get list of valid agent names
pub fn valid_agent_names() -> Vec<&'static str> {
    vec!["claude", "codex", "claudecode", "mock"]
}
