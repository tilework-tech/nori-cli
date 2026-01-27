use strum::IntoEnumIterator;
use strum_macros::AsRefStr;
use strum_macros::EnumIter;
use strum_macros::EnumString;
use strum_macros::IntoStaticStr;

/// Commands that can be invoked by starting a message with a leading slash.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, EnumString, EnumIter, AsRefStr, IntoStaticStr,
)]
#[strum(serialize_all = "kebab-case")]
pub enum SlashCommand {
    // DO NOT ALPHA-SORT! Enum order is presentation order in the popup, so
    // more frequently used commands should be listed first.
    Agent,
    Model,
    Approvals,
    Config,
    Review,
    New,
    Init,
    Compact,
    Undo,
    Diff,
    Mention,
    Status,
    Mcp,
    Login,
    Logout,
    Quit,
    Exit,
    Rollout,
    TestApproval,
}

impl SlashCommand {
    /// User-visible description shown in the popup.
    pub fn description(self) -> &'static str {
        match self {
            SlashCommand::Agent => "switch between available ACP agents",
            SlashCommand::New => "start a new chat during a conversation",
            SlashCommand::Init => "create an AGENTS.md file with instructions for Nori",
            SlashCommand::Compact => "summarize conversation to prevent hitting the context limit",
            SlashCommand::Review => "review my current changes and find issues",
            SlashCommand::Undo => "ask Nori to undo a turn",
            SlashCommand::Quit | SlashCommand::Exit => "exit Nori",
            SlashCommand::Diff => "show git diff (including untracked files)",
            SlashCommand::Mention => "mention a file",
            SlashCommand::Status => "show current session configuration and token usage",
            SlashCommand::Model => "choose what model and reasoning effort to use",
            SlashCommand::Approvals => "choose what Nori can do without approval",
            SlashCommand::Config => "toggle TUI settings (vertical footer)",
            SlashCommand::Mcp => "list configured MCP tools",
            SlashCommand::Login => "log in to the current agent",
            SlashCommand::Logout => "show logout instructions",
            SlashCommand::Rollout => "print the rollout file path",
            SlashCommand::TestApproval => "test approval request",
        }
    }

    /// Command string without the leading '/'. Provided for compatibility with
    /// existing code that expects a method named `command()`.
    pub fn command(self) -> &'static str {
        self.into()
    }

    /// Whether this command can be run while a task is in progress.
    pub fn available_during_task(self) -> bool {
        match self {
            SlashCommand::Agent
            | SlashCommand::New
            | SlashCommand::Init
            | SlashCommand::Compact
            | SlashCommand::Undo
            | SlashCommand::Model
            | SlashCommand::Approvals
            | SlashCommand::Config
            | SlashCommand::Review
            | SlashCommand::Login
            | SlashCommand::Logout => false,
            SlashCommand::Diff
            | SlashCommand::Mention
            | SlashCommand::Status
            | SlashCommand::Mcp
            | SlashCommand::Quit
            | SlashCommand::Exit => true,
            SlashCommand::Rollout => true,
            SlashCommand::TestApproval => true,
        }
    }

    fn is_visible(self) -> bool {
        match self {
            SlashCommand::Rollout | SlashCommand::TestApproval => cfg!(debug_assertions),
            #[cfg(not(feature = "login"))]
            SlashCommand::Logout => false,
            _ => true,
        }
    }
}

/// Return all built-in commands in a Vec paired with their command string.
pub fn built_in_slash_commands() -> Vec<(&'static str, SlashCommand)> {
    SlashCommand::iter()
        .filter(|command| command.is_visible())
        .map(|c| (c.command(), c))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(not(feature = "login"))]
    fn logout_hidden_when_login_feature_disabled() {
        let commands = built_in_slash_commands();
        let has_logout = commands.iter().any(|(_, cmd)| *cmd == SlashCommand::Logout);
        assert!(
            !has_logout,
            "/logout should be hidden when login feature is disabled"
        );
    }

    #[test]
    #[cfg(feature = "login")]
    fn logout_visible_when_login_feature_enabled() {
        let commands = built_in_slash_commands();
        let has_logout = commands.iter().any(|(_, cmd)| *cmd == SlashCommand::Logout);
        assert!(
            has_logout,
            "/logout should be visible when login feature is enabled"
        );
    }

    #[test]
    fn login_visible_in_commands() {
        let commands = built_in_slash_commands();
        let has_login = commands.iter().any(|(_, cmd)| *cmd == SlashCommand::Login);
        assert!(has_login, "/login should be visible in commands list");
    }

    #[test]
    fn login_has_description() {
        let desc = SlashCommand::Login.description();
        assert!(!desc.is_empty(), "/login should have a description");
    }

    #[test]
    fn login_not_available_during_task() {
        assert!(
            !SlashCommand::Login.available_during_task(),
            "/login should not be available while task is running"
        );
    }

    #[test]
    fn config_visible_in_commands() {
        let commands = built_in_slash_commands();
        let has_config = commands.iter().any(|(_, cmd)| *cmd == SlashCommand::Config);
        assert!(has_config, "/config should be visible in commands list");
    }

    #[test]
    fn config_has_description() {
        let desc = SlashCommand::Config.description();
        assert!(!desc.is_empty(), "/config should have a description");
    }

    #[test]
    fn config_not_available_during_task() {
        assert!(
            !SlashCommand::Config.available_during_task(),
            "/config should not be available while task is running"
        );
    }
}
