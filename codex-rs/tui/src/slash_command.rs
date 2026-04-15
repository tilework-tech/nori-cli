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
    New,
    Resume,
    ResumeViewonly,
    Init,
    Compact,
    Undo,
    Browse,
    Diff,
    Mention,
    Status,
    Memory,
    FirstPrompt,
    Mcp,
    Login,
    Logout,
    Quit,
    Exit,
    SwitchSkillset,
    Fork,
}

impl SlashCommand {
    /// User-visible description shown in the popup.
    pub fn description(self) -> &'static str {
        match self {
            SlashCommand::Agent => "switch between available ACP agents",
            SlashCommand::New => "start a new chat during a conversation",
            SlashCommand::Resume => "resume a previous session",
            SlashCommand::ResumeViewonly => "view a previous session transcript (read-only)",
            SlashCommand::Init => "create an AGENTS.md file with instructions for Nori",
            SlashCommand::Compact => "summarize conversation to prevent hitting the context limit",
            SlashCommand::Undo => "ask Nori to undo a turn",
            SlashCommand::Quit | SlashCommand::Exit => "exit Nori",
            SlashCommand::Browse => "open a file manager to browse and edit files",
            SlashCommand::Diff => "show git diff (including untracked files)",
            SlashCommand::Mention => "mention a file",
            SlashCommand::Status => "show current session configuration and context window usage",
            SlashCommand::Memory => "show the contents of all active instruction files",
            SlashCommand::FirstPrompt => "show the first prompt from this session",
            SlashCommand::Model => "choose what model and reasoning effort to use",
            SlashCommand::Approvals => "choose what Nori can do without approval",
            SlashCommand::Config => "toggle config settings",
            SlashCommand::Mcp => "manage MCP server connections",
            SlashCommand::Login => "log in to the current agent",
            SlashCommand::Logout => "show logout instructions",
            SlashCommand::SwitchSkillset => "switch between available skillsets",
            SlashCommand::Fork => "rewind conversation to a previous message",
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
            | SlashCommand::Resume
            | SlashCommand::ResumeViewonly
            | SlashCommand::Init
            | SlashCommand::Compact
            | SlashCommand::Undo
            | SlashCommand::Model
            | SlashCommand::Approvals
            | SlashCommand::Config
            | SlashCommand::Mcp
            | SlashCommand::Login
            | SlashCommand::Logout
            | SlashCommand::SwitchSkillset
            | SlashCommand::Fork => false,
            SlashCommand::Browse
            | SlashCommand::Diff
            | SlashCommand::Mention
            | SlashCommand::Status
            | SlashCommand::Memory
            | SlashCommand::FirstPrompt
            | SlashCommand::Quit
            | SlashCommand::Exit => true,
        }
    }

    fn is_visible(self) -> bool {
        match self {
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

    #[test]
    fn first_prompt_visible_in_commands() {
        let commands = built_in_slash_commands();
        let has_first_prompt = commands
            .iter()
            .any(|(_, cmd)| *cmd == SlashCommand::FirstPrompt);
        assert!(
            has_first_prompt,
            "/first-prompt should be visible in commands list"
        );
    }

    #[test]
    fn first_prompt_has_description() {
        let desc = SlashCommand::FirstPrompt.description();
        assert!(!desc.is_empty(), "/first-prompt should have a description");
    }

    #[test]
    fn first_prompt_available_during_task() {
        assert!(
            SlashCommand::FirstPrompt.available_during_task(),
            "/first-prompt should be available while task is running"
        );
    }

    #[test]
    fn switch_skillset_visible_in_commands() {
        let commands = built_in_slash_commands();
        let has_switch_skillset = commands
            .iter()
            .any(|(_, cmd)| *cmd == SlashCommand::SwitchSkillset);
        assert!(
            has_switch_skillset,
            "/switch-skillset should be visible in commands list"
        );
    }

    #[test]
    fn switch_skillset_has_description() {
        let desc = SlashCommand::SwitchSkillset.description();
        assert!(
            !desc.is_empty(),
            "/switch-skillset should have a description"
        );
    }

    #[test]
    fn switch_skillset_not_available_during_task() {
        assert!(
            !SlashCommand::SwitchSkillset.available_during_task(),
            "/switch-skillset should not be available while task is running"
        );
    }

    #[test]
    fn resume_visible_in_commands() {
        let commands = built_in_slash_commands();
        let has_resume = commands.iter().any(|(_, cmd)| *cmd == SlashCommand::Resume);
        assert!(has_resume, "/resume should be visible in commands list");
    }

    #[test]
    fn resume_has_description() {
        let desc = SlashCommand::Resume.description();
        assert!(!desc.is_empty(), "/resume should have a description");
    }

    #[test]
    fn resume_not_available_during_task() {
        assert!(
            !SlashCommand::Resume.available_during_task(),
            "/resume should not be available while task is running"
        );
    }

    #[test]
    fn browse_visible_in_commands() {
        let commands = built_in_slash_commands();
        let has_browse = commands.iter().any(|(_, cmd)| *cmd == SlashCommand::Browse);
        assert!(has_browse, "/browse should be visible in commands list");
    }

    #[test]
    fn browse_has_description() {
        let desc = SlashCommand::Browse.description();
        assert!(!desc.is_empty(), "/browse should have a description");
    }

    #[test]
    fn browse_available_during_task() {
        assert!(
            SlashCommand::Browse.available_during_task(),
            "/browse should be available while task is running"
        );
    }

    #[test]
    fn legacy_debug_commands_are_not_exposed() {
        let commands = built_in_slash_commands();

        assert!(
            commands.iter().all(|(name, _)| *name != "rollout"),
            "/rollout should not be visible in commands list"
        );
        assert!(
            commands.iter().all(|(name, _)| *name != "test-approval"),
            "/test-approval should not be visible in commands list"
        );
        assert!(
            "rollout".parse::<SlashCommand>().is_err(),
            "/rollout should not parse as a slash command"
        );
        assert!(
            "test-approval".parse::<SlashCommand>().is_err(),
            "/test-approval should not parse as a slash command"
        );
    }

    #[test]
    fn browse_parses_from_string() {
        let cmd: SlashCommand = "browse".parse().expect("/browse should parse from string");
        assert_eq!(cmd, SlashCommand::Browse);
    }

    #[test]
    fn resume_parses_from_string() {
        let cmd: SlashCommand = "resume".parse().expect("/resume should parse from string");
        assert_eq!(cmd, SlashCommand::Resume);
    }

    #[test]
    fn mcp_not_available_during_task() {
        assert!(
            !SlashCommand::Mcp.available_during_task(),
            "/mcp should not be available while task is running"
        );
    }
}
