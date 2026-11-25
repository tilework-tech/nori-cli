//! One availability verdict per builtin slash command, shared by plain `nori`
//! and `nori attach`.
//!
//! Both terminals list every builtin in the `/` menu. A command that cannot
//! run is greyed with the reason this module returns, and running it anyway
//! is refused with the same reason, so the menu and execution never disagree.

use crate::host_client::HostFeatures;
use crate::slash_command::CommandScope;
use crate::slash_command::SlashCommand;

/// The host has no method for the command yet; only plain `nori`'s own
/// in-process session can run it.
pub(crate) const NOT_ON_THIS_HOST: &str = "not available on this host yet";
/// The host can run the command, but the attached terminal has no screen
/// for it yet.
pub(crate) const NOT_IN_ATTACH: &str = "not available in nori attach yet";
/// Only the controlling terminal may change sessions.
pub(crate) const WATCHING: &str = "watching · Alt+T to take control";
/// Host commands wait for the connection to come back.
pub(crate) const RECONNECTING: &str = "reconnecting";
/// Typed input waits for the connection to come back.
pub(crate) const RECONNECTING_INPUT: &str =
    "Reconnecting to the host. Your draft is kept; send it once the connection is back.";

/// Where a command does its work.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Route {
    /// Only the terminal itself is involved.
    Terminal,
    /// A host method runs it. Changing a session needs control.
    Host { needs_control: bool },
    /// It reads or changes state that only an in-process session has.
    InProcess,
}

fn route(command: SlashCommand) -> Route {
    match command {
        SlashCommand::Quit
        | SlashCommand::Exit
        | SlashCommand::Settings
        | SlashCommand::Vim
        | SlashCommand::Logout
        | SlashCommand::Screen => Route::Terminal,
        SlashCommand::Sessions => Route::Host {
            needs_control: false,
        },
        SlashCommand::Agent
        | SlashCommand::Model
        | SlashCommand::Config
        | SlashCommand::Approvals
        | SlashCommand::New
        | SlashCommand::Delegate
        | SlashCommand::Resume
        | SlashCommand::Close => Route::Host {
            needs_control: true,
        },
        SlashCommand::Goal
        | SlashCommand::ResumeViewonly
        | SlashCommand::Init
        | SlashCommand::Compact
        | SlashCommand::Undo
        | SlashCommand::Browse
        | SlashCommand::Diff
        | SlashCommand::Mention
        | SlashCommand::Status
        | SlashCommand::Context
        | SlashCommand::Memory
        | SlashCommand::FirstPrompt
        | SlashCommand::Mcp
        | SlashCommand::Login
        | SlashCommand::ApiKey
        | SlashCommand::SwitchSkillset
        | SlashCommand::Fork
        | SlashCommand::Browser => Route::InProcess,
    }
}

/// The terminal a command was typed in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Surface {
    /// Plain `nori`: the terminal owns its host, always controls it, and
    /// keeps an in-process session for commands the host has no method for.
    Owned {
        cloud_mode: bool,
        agent_session_close: bool,
    },
    /// The shared terminal joined to a host it does not own. It has no
    /// in-process session, so only terminal commands and host methods run.
    Joined {
        watching: bool,
        connected: bool,
        features: HostFeatures,
    },
}

/// Commands plain `nori` runs in process that a joined host also runs once
/// it advertises their method; `Some(needs_control)` when it does.
fn served_by_host(command: SlashCommand, features: HostFeatures) -> Option<bool> {
    match command {
        SlashCommand::Goal => features.goal.then_some(false),
        SlashCommand::Context => features.context.then_some(false),
        SlashCommand::Quit
        | SlashCommand::Exit
        | SlashCommand::Settings
        | SlashCommand::Vim
        | SlashCommand::Logout
        | SlashCommand::Screen
        | SlashCommand::Sessions
        | SlashCommand::Agent
        | SlashCommand::Model
        | SlashCommand::Config
        | SlashCommand::Approvals
        | SlashCommand::New
        | SlashCommand::Delegate
        | SlashCommand::Resume
        | SlashCommand::Close
        | SlashCommand::ResumeViewonly
        | SlashCommand::Init
        | SlashCommand::Compact
        | SlashCommand::Undo
        | SlashCommand::Browse
        | SlashCommand::Diff
        | SlashCommand::Mention
        | SlashCommand::Status
        | SlashCommand::Memory
        | SlashCommand::FirstPrompt
        | SlashCommand::Mcp
        | SlashCommand::Login
        | SlashCommand::ApiKey
        | SlashCommand::SwitchSkillset
        | SlashCommand::Fork
        | SlashCommand::Browser => None,
    }
}

/// Host commands the shared terminal runs on a host it joined.
fn runs_when_joined(command: SlashCommand) -> bool {
    match command {
        SlashCommand::Sessions
        | SlashCommand::New
        | SlashCommand::Close
        | SlashCommand::Delegate
        | SlashCommand::Approvals => true,
        SlashCommand::Quit
        | SlashCommand::Exit
        | SlashCommand::Settings
        | SlashCommand::Vim
        | SlashCommand::Logout
        | SlashCommand::Screen
        | SlashCommand::Agent
        | SlashCommand::Model
        | SlashCommand::Config
        | SlashCommand::Resume
        | SlashCommand::Goal
        | SlashCommand::ResumeViewonly
        | SlashCommand::Init
        | SlashCommand::Compact
        | SlashCommand::Undo
        | SlashCommand::Browse
        | SlashCommand::Diff
        | SlashCommand::Mention
        | SlashCommand::Status
        | SlashCommand::Context
        | SlashCommand::Memory
        | SlashCommand::FirstPrompt
        | SlashCommand::Mcp
        | SlashCommand::Login
        | SlashCommand::ApiKey
        | SlashCommand::SwitchSkillset
        | SlashCommand::Fork
        | SlashCommand::Browser => false,
    }
}

/// `None` when `command` can run in `surface`, otherwise the reason shown in
/// the menu and when the command is refused. `/quit` and `/exit` always run.
pub(crate) fn unavailable_reason(command: SlashCommand, surface: Surface) -> Option<String> {
    match surface {
        Surface::Owned {
            cloud_mode,
            agent_session_close,
        } => scope_reason(command, cloud_mode, agent_session_close),
        Surface::Joined {
            watching,
            connected,
            features,
        } => match served_by_host(command, features) {
            Some(needs_control) => joined_host_reason(needs_control, watching, connected),
            None => match route(command) {
                Route::Terminal => None,
                Route::InProcess => Some(NOT_ON_THIS_HOST.to_string()),
                Route::Host { .. } if !runs_when_joined(command) => Some(NOT_IN_ATTACH.to_string()),
                Route::Host { needs_control } => {
                    joined_host_reason(needs_control, watching, connected)
                }
            },
        },
    }
}

/// Why a host command cannot run on a joined host now, if it cannot.
fn joined_host_reason(needs_control: bool, watching: bool, connected: bool) -> Option<String> {
    if needs_control && !connected {
        Some(RECONNECTING.to_string())
    } else if needs_control && watching {
        Some(WATCHING.to_string())
    } else {
        None
    }
}

/// Session-type scope in plain `nori`. LocalOnly commands work on the local
/// machine and mean nothing in a cloud session. With several local sessions,
/// `/close` closes the one on screen; in a cloud session it releases the
/// session and needs the agent's `session/close`.
fn scope_reason(
    command: SlashCommand,
    cloud_mode: bool,
    agent_session_close: bool,
) -> Option<String> {
    match command.scope() {
        CommandScope::LocalOnly if cloud_mode => Some(format!(
            "/{} runs on the local machine and is unavailable in cloud sessions.",
            command.command()
        )),
        CommandScope::CloudOnly if cloud_mode && !agent_session_close => Some(
            "/close releases a cloud session and needs the agent's session/close capability."
                .to_string(),
        ),
        CommandScope::LocalOnly | CommandScope::CloudOnly | CommandScope::Universal => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::slash_command::built_in_slash_commands;
    use pretty_assertions::assert_eq;
    use std::str::FromStr;

    #[test]
    fn screen_runs_in_plain_nori_and_in_attach_even_during_a_turn() {
        let command = SlashCommand::from_str("screen").expect("/screen parses");
        assert!(
            built_in_slash_commands()
                .iter()
                .any(|(_, visible)| *visible == command),
            "/screen is a builtin"
        );
        assert!(command.available_during_task());
        for cloud_mode in [false, true] {
            assert_eq!(
                unavailable_reason(
                    command,
                    Surface::Owned {
                        cloud_mode,
                        agent_session_close: false,
                    }
                ),
                None
            );
        }
        for (watching, connected) in [(false, true), (true, true), (false, false)] {
            assert_eq!(
                unavailable_reason(
                    command,
                    Surface::Joined {
                        watching,
                        connected,
                        features: HostFeatures::default(),
                    }
                ),
                None
            );
        }
    }

    #[test]
    fn a_joined_terminal_runs_terminal_commands_and_host_commands_its_role_allows() {
        let joined = |watching, connected| Surface::Joined {
            watching,
            connected,
            features: HostFeatures::default(),
        };
        let cases = [
            (SlashCommand::Screen, joined(false, true), None),
            (SlashCommand::New, joined(false, true), None),
            (SlashCommand::Close, joined(false, true), None),
            (
                SlashCommand::Close,
                joined(false, false),
                Some(RECONNECTING),
            ),
            (SlashCommand::Approvals, joined(false, true), None),
            (SlashCommand::Sessions, joined(true, true), None),
            (SlashCommand::New, joined(true, true), Some(WATCHING)),
            (SlashCommand::Close, joined(true, true), Some(WATCHING)),
            (SlashCommand::New, joined(false, false), Some(RECONNECTING)),
            (
                SlashCommand::Compact,
                joined(false, true),
                Some(NOT_ON_THIS_HOST),
            ),
            (
                SlashCommand::Model,
                joined(false, true),
                Some(NOT_IN_ATTACH),
            ),
            (SlashCommand::New, joined(true, false), Some(RECONNECTING)),
            (SlashCommand::Sessions, joined(false, false), None),
            (SlashCommand::Quit, joined(true, false), None),
            (SlashCommand::Exit, joined(true, true), None),
        ];
        for (command, surface, expected) in cases {
            assert_eq!(
                unavailable_reason(command, surface),
                expected.map(str::to_string),
                "/{} in {surface:?}",
                command.command()
            );
        }
    }

    #[test]
    fn a_joined_terminal_runs_goal_only_on_a_host_that_serves_it() {
        let older = Surface::Joined {
            watching: false,
            connected: true,
            features: HostFeatures::default(),
        };
        let serving = |watching, connected| Surface::Joined {
            watching,
            connected,
            features: HostFeatures {
                goal: true,
                ..HostFeatures::default()
            },
        };
        let cases = [
            (older, Some(NOT_ON_THIS_HOST)),
            (serving(false, true), None),
            (serving(true, true), None),
            (serving(false, false), None),
        ];
        for (surface, expected) in cases {
            assert_eq!(
                unavailable_reason(SlashCommand::Goal, surface),
                expected.map(str::to_string),
                "/goal in {surface:?}"
            );
        }
    }

    #[test]
    fn a_joined_terminal_shows_context_only_on_a_host_that_serves_it() {
        let joined = |watching, context| Surface::Joined {
            watching,
            connected: true,
            features: HostFeatures {
                context,
                ..HostFeatures::default()
            },
        };
        let cases = [
            (joined(false, false), Some(NOT_ON_THIS_HOST)),
            (joined(false, true), None),
            (joined(true, true), None),
        ];
        for (surface, expected) in cases {
            assert_eq!(
                unavailable_reason(SlashCommand::Context, surface),
                expected.map(str::to_string),
                "/context in {surface:?}"
            );
        }
    }
}
