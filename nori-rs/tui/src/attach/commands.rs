//! Builtin slash commands in an attached terminal. The `/` menu and
//! execution share [`crate::command_availability`], so a greyed command is
//! refused with the reason the menu shows and never reaches the agent.

use strum::IntoEnumIterator;

use super::runtime::Pending;
use super::runtime::Runtime;
use crate::command_availability::RECONNECTING;
use crate::command_availability::Surface;
use crate::command_availability::unavailable_reason;
use crate::host_client::HostCall;
use crate::slash_command::SlashCommand;

const CHOOSE_SESSION: &str = "Choose a session with /sessions or start one with /new.";

impl Runtime {
    fn command_surface(&self) -> Surface {
        Surface::Attached {
            watching: self.state.watching,
            connected: self.state.connected && self.state.stopped.is_none(),
        }
    }

    /// Grey every builtin the terminal cannot run right now, with its reason.
    pub(super) fn sync_command_menu(&mut self) {
        let surface = self.command_surface();
        self.composer
            .set_disabled_builtin_commands(SlashCommand::iter().filter_map(|command| {
                unavailable_reason(command, surface).map(|reason| (command, reason))
            }));
    }

    /// Run a builtin typed as `draft`, with `args` after its name. A refused
    /// command puts `draft` back in the composer. False means detach.
    pub(super) fn run_command(&mut self, command: SlashCommand, args: &str, draft: String) -> bool {
        // While reconnecting, the connection check below gives the fuller
        // reason (the draft is kept and can be sent later).
        if let Some(reason) = unavailable_reason(command, self.command_surface())
            && reason != RECONNECTING
        {
            self.composer.set_text(draft);
            self.state
                .show_notice(format!("/{}: {reason}", command.command()));
            return true;
        }
        match command {
            SlashCommand::Quit | SlashCommand::Exit => return false,
            SlashCommand::Sessions => {
                self.state.list_open = true;
                self.refresh_list();
                return true;
            }
            SlashCommand::New
            | SlashCommand::Delegate
            | SlashCommand::Resume
            | SlashCommand::Close
            | SlashCommand::Approvals => {}
            SlashCommand::Agent
            | SlashCommand::Model
            | SlashCommand::Config
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
            | SlashCommand::Browser
            | SlashCommand::Screen => {
                // The availability table refuses these above.
                self.composer.set_text(draft);
                return true;
            }
        }
        if let Err(error) = self.require_mutation() {
            self.composer.set_text(draft);
            self.state.show_notice(error.to_string());
            return true;
        }
        self.state.notice = None;
        if command == SlashCommand::New {
            let session_folder = self.state.selected.as_ref().and_then(|id| {
                self.state
                    .sessions
                    .iter()
                    .find(|row| &row.session_id == id)
                    .and_then(|row| row.cwd.clone())
                    .or_else(|| {
                        self.state
                            .conversations
                            .get(id)
                            .map(|conversation| conversation.cwd.clone())
                            .filter(|cwd| !cwd.as_os_str().is_empty())
                    })
            });
            let base = match &session_folder {
                Some(folder) => super::new_session::Base::Session(folder),
                None => super::new_session::Base::Terminal(&self.cwd),
            };
            match super::new_session::params(
                args,
                base,
                self.session_folders,
                self.find_paths,
                self.remote,
            ) {
                Ok(params) => {
                    self.send(HostCall::NewSession(params), Pending::New);
                }
                Err(message) => {
                    self.composer.set_text(draft);
                    self.state.show_notice(message);
                }
            }
            return true;
        }
        let Some(id) = self.state.selected.clone() else {
            self.composer.set_text(draft);
            self.state.show_notice(CHOOSE_SESSION);
            return true;
        };
        if command == SlashCommand::Approvals {
            self.approvals_command(id, args, draft);
            return true;
        }
        if command == SlashCommand::Delegate {
            match crate::delegation::parse_command(draft.trim()) {
                Some(Ok((agent, task))) => {
                    self.send(
                        HostCall::Delegate {
                            session_id: id,
                            agent,
                            task,
                        },
                        Pending::Mutation { draft: Some(draft) },
                    );
                }
                Some(Err(message)) => {
                    self.composer.set_text(draft);
                    self.state.show_notice(message);
                }
                None => {
                    self.composer.set_text(draft);
                    self.state.show_notice(SlashCommand::Delegate.description());
                }
            }
            return true;
        }
        if command == SlashCommand::Close {
            self.close(id);
        } else {
            self.resume(&id);
        }
        self.state.scroll = 0;
        true
    }
}
