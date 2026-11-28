//! Synthetic notices for the storybook and snapshot tests. Versions, adapter
//! output, and copy are invented; nothing here comes from a real session.

use super::Notice;
use super::NoticeAction;

/// One notice per use the cell is designed for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoticeSpecimen {
    CliUpdate,
    AdapterUpdate,
    AdapterSignIn,
    AdapterInfo,
    Invitation,
    Tip,
}

impl NoticeSpecimen {
    pub const ALL: [Self; 6] = [
        Self::CliUpdate,
        Self::AdapterUpdate,
        Self::AdapterSignIn,
        Self::AdapterInfo,
        Self::Invitation,
        Self::Tip,
    ];

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::CliUpdate => "CLI update",
            Self::AdapterUpdate => "ACP adapter update",
            Self::AdapterSignIn => "adapter stderr: sign-in",
            Self::AdapterInfo => "adapter stderr: info",
            Self::Invitation => "repo invitation",
            Self::Tip => "feature tip",
        }
    }

    #[must_use]
    #[allow(clippy::expect_used, reason = "stderr fixtures are constant visible text")]
    pub fn notice(self) -> Notice {
        match self {
            Self::CliUpdate => {
                Notice::new("nori CLI", "update", "Version 0.31.0 is ready. You have 0.30.2.")
                    .with_action(NoticeAction::new(
                        "cli.update",
                        "update when you exit",
                        "Nori will update when you exit.",
                    ))
                    .with_later("To update later, run npm install -g nori-ai-cli@latest")
            }
            Self::AdapterUpdate => Notice::new(
                "Claude Code adapter",
                "update",
                "claude-agent-acp 0.14.0 is out. This project pins 0.12.3.",
            )
            .with_action(NoticeAction::new(
                "adapter.use-latest",
                "use 0.14.0 next session",
                "New Claude Code sessions will start with 0.14.0.",
            ))
            .with_later("To move the pin, edit [acp_adapters.claude-code] version"),
            Self::AdapterSignIn => Notice::from_stderr(
                "Claude Code adapter",
                "\u{1b}[33mNot signed in.\u{1b}[0m Run `claude` and then /login, or set ANTHROPIC_API_KEY.\n",
            )
            .expect("fixture has visible text"),
            Self::AdapterInfo => Notice::from_stderr(
                "Codex adapter",
                "Using the cached model list from 2 hours ago.\nRefreshing in the background.",
            )
            .expect("fixture has visible text"),
            Self::Invitation => Notice::new(
                "nori",
                "invitation",
                "If nori is useful to you, star tilework-tech/nori-cli on GitHub. The repo is also a starting point for building your own ACP agent or client.",
            )
            .with_action(NoticeAction::new(
                "repo.open",
                "open the repo",
                "Opened github.com/tilework-tech/nori-cli in your browser.",
            ))
            .with_later("github.com/tilework-tech/nori-cli"),
            Self::Tip => Notice::new(
                "nori",
                "tip",
                "Press Left on an empty prompt to see your agent sessions. Ctrl+T opens the full transcript, and /settings changes the rest.",
            ),
        }
    }
}
