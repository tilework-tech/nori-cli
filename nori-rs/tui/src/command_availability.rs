//! One availability verdict per builtin slash command, shared by plain `nori`
//! and `nori attach`.
//!
//! Both terminals list every builtin in the `/` menu. A command that cannot
//! run is greyed with the reason this module returns, and running it anyway
//! is refused with the same reason, so the menu and execution never disagree.

use std::str::FromStr;

use crate::slash_command::CommandScope;
use crate::slash_command::SlashCommand;
use crate::slash_command::built_in_slash_commands;

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
        | SlashCommand::Logout => Route::Terminal,
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

/// Whether the attached terminal has its own handling for the command.
fn runs_in_attach(command: SlashCommand) -> bool {
    match command {
        SlashCommand::Quit
        | SlashCommand::Exit
        | SlashCommand::Sessions
        | SlashCommand::New
        | SlashCommand::Delegate
        | SlashCommand::Resume
        | SlashCommand::Close => true,
        SlashCommand::Agent
        | SlashCommand::Model
        | SlashCommand::Config
        | SlashCommand::Approvals
        | SlashCommand::Settings
        | SlashCommand::Vim
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
        | SlashCommand::Logout
        | SlashCommand::ApiKey
        | SlashCommand::SwitchSkillset
        | SlashCommand::Fork
        | SlashCommand::Browser => false,
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
    /// `nori attach`: a terminal connected to a host it does not own.
    Attached { watching: bool, connected: bool },
}

/// `None` when `command` can run in `surface`, otherwise the reason shown in
/// the menu and when the command is refused. `/quit` and `/exit` always run.
pub(crate) fn unavailable_reason(command: SlashCommand, surface: Surface) -> Option<String> {
    match surface {
        Surface::Owned {
            cloud_mode,
            agent_session_close,
        } => scope_reason(command, cloud_mode, agent_session_close),
        Surface::Attached {
            watching,
            connected,
        } => {
            let route = route(command);
            if !runs_in_attach(command) {
                return Some(
                    if route == Route::InProcess {
                        NOT_ON_THIS_HOST
                    } else {
                        NOT_IN_ATTACH
                    }
                    .to_string(),
                );
            }
            match route {
                Route::Terminal | Route::InProcess => None,
                Route::Host {
                    needs_control: true,
                } if !connected => Some(RECONNECTING.to_string()),
                Route::Host {
                    needs_control: true,
                } if watching => Some(WATCHING.to_string()),
                Route::Host { .. } => None,
            }
        }
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

/// The builtin a submitted line starts with, and the text after its name.
/// Names that are not builtins (agent commands, custom prompts, paths) are
/// `None`, so they reach the agent unchanged.
pub(crate) fn parse_builtin(text: &str) -> Option<(SlashCommand, &str)> {
    let rest = text.strip_prefix('/')?;
    let name_end = rest.find(char::is_whitespace).unwrap_or(rest.len());
    let name = &rest[..name_end];
    let command = SlashCommand::from_str(name).ok()?;
    built_in_slash_commands()
        .iter()
        .any(|(_, visible)| *visible == command)
        .then_some((command, &rest[name_end..]))
}
