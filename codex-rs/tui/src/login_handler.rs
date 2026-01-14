//! Login handler for /login slash command.
//!
//! This module handles authentication flows for ACP agents:
//! - OAuth browser flow (Codex)
//! - External CLI passthrough (Gemini, Claude Code)

use codex_acp::AgentKind;
use codex_acp::list_available_agents;
use codex_login::ShutdownHandle;
use tokio::task::JoinHandle;

/// Method used for authentication
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoginMethod {
    /// OAuth browser flow - starts local server, opens browser
    OAuthBrowser,
    /// External CLI passthrough - spawns agent CLI with PTY
    ExternalCli {
        /// The command to run (e.g., "gemini")
        command: String,
        /// Arguments to pass (e.g., ["login"])
        args: Vec<String>,
    },
}

/// State of the login flow
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoginFlowState {
    /// No login flow active
    Idle,
    /// OAuth flow in progress - waiting for browser
    AwaitingBrowserAuth,
    /// External CLI login in progress - spawned PTY process
    AwaitingExternalCli {
        /// The agent name for display purposes
        agent_name: String,
    },
    /// Login successful
    Success,
    /// Login cancelled by user
    Cancelled,
}

/// Result of checking agent support for login
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentLoginSupport {
    /// Agent supports in-app login
    Supported {
        agent: AgentKind,
        is_installed: bool,
        login_method: LoginMethod,
    },
    /// Agent doesn't support in-app login yet
    NotSupported { agent_name: String },
    /// Unknown agent
    Unknown { model_name: String },
}

/// Handler for the /login command flow.
///
/// Manages the OAuth authentication state and provides a shutdown handle
/// for cancelling the login server.
pub struct LoginHandler {
    /// Current state of the login flow
    state: LoginFlowState,
    /// Shutdown handle for cancelling OAuth flow
    shutdown_handle: Option<ShutdownHandle>,
    /// Task handle for cancelling external CLI PTY process
    pty_task_handle: Option<JoinHandle<()>>,
}

impl LoginHandler {
    /// Create a new login handler
    pub fn new() -> Self {
        Self {
            state: LoginFlowState::Idle,
            shutdown_handle: None,
            pty_task_handle: None,
        }
    }

    /// Check if an agent supports in-app login
    pub fn check_agent_support(model_name: &str) -> AgentLoginSupport {
        let normalized = model_name.to_lowercase();

        // Try to find the agent in the registry
        let agents = list_available_agents();
        let agent_info = agents
            .into_iter()
            .find(|a| a.model_name.to_lowercase() == normalized);

        match agent_info {
            Some(info) => {
                match info.agent {
                    // Codex supports in-app login via OAuth browser flow
                    AgentKind::Codex => AgentLoginSupport::Supported {
                        agent: AgentKind::Codex,
                        is_installed: info.is_installed,
                        login_method: LoginMethod::OAuthBrowser,
                    },
                    // Gemini supports in-app login via external CLI passthrough
                    AgentKind::Gemini => AgentLoginSupport::Supported {
                        agent: AgentKind::Gemini,
                        is_installed: info.is_installed,
                        login_method: LoginMethod::ExternalCli {
                            command: "gemini".to_string(),
                            args: vec!["login".to_string()],
                        },
                    },
                    // Other agents don't support in-app login yet
                    other => AgentLoginSupport::NotSupported {
                        agent_name: other.display_name().to_string(),
                    },
                }
            }
            None => AgentLoginSupport::Unknown {
                model_name: model_name.to_string(),
            },
        }
    }

    /// Start the OAuth flow
    pub fn start_oauth(&mut self) {
        self.state = LoginFlowState::AwaitingBrowserAuth;
    }

    /// Start the external CLI login flow
    pub fn start_external_cli(&mut self, agent_name: String) {
        self.state = LoginFlowState::AwaitingExternalCli { agent_name };
    }

    /// Set the shutdown handle for cancellation
    pub fn set_shutdown_handle(&mut self, handle: ShutdownHandle) {
        self.shutdown_handle = Some(handle);
    }

    /// Set the PTY task handle for cancellation
    pub fn set_pty_task_handle(&mut self, handle: JoinHandle<()>) {
        self.pty_task_handle = Some(handle);
    }

    /// OAuth login completed successfully
    pub fn oauth_complete(&mut self) {
        self.state = LoginFlowState::Success;
    }

    /// Cancel the login flow
    pub fn cancel(&mut self) {
        // Shutdown OAuth server if running
        if let Some(handle) = self.shutdown_handle.take() {
            handle.shutdown();
        }
        // Abort PTY task if running (this will drop the session and kill the process)
        if let Some(handle) = self.pty_task_handle.take() {
            handle.abort();
        }
        self.state = LoginFlowState::Cancelled;
    }
}

impl Default for LoginHandler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_agent_support_returns_supported_for_codex_with_oauth() {
        let support = LoginHandler::check_agent_support("codex");

        match support {
            AgentLoginSupport::Supported {
                agent,
                login_method,
                ..
            } => {
                assert_eq!(agent, AgentKind::Codex);
                assert_eq!(login_method, LoginMethod::OAuthBrowser);
            }
            _ => panic!("Expected Supported variant for codex"),
        }
    }

    #[test]
    fn check_agent_support_returns_not_supported_for_claude() {
        // Claude Code login support will be added later
        let support = LoginHandler::check_agent_support("claude-code");

        match support {
            AgentLoginSupport::NotSupported { agent_name } => {
                assert_eq!(agent_name, "Claude Code");
            }
            _ => panic!("Expected NotSupported variant for claude-code"),
        }
    }

    #[test]
    fn check_agent_support_returns_supported_for_gemini_with_external_cli() {
        // Gemini supports in-app login via external CLI passthrough
        let support = LoginHandler::check_agent_support("gemini");

        match support {
            AgentLoginSupport::Supported {
                agent,
                login_method,
                ..
            } => {
                assert_eq!(agent, AgentKind::Gemini);
                match login_method {
                    LoginMethod::ExternalCli { command, args } => {
                        assert_eq!(command, "gemini");
                        assert_eq!(args, vec!["login".to_string()]);
                    }
                    _ => panic!("Expected ExternalCli login method for gemini"),
                }
            }
            _ => panic!("Expected Supported variant for gemini"),
        }
    }

    #[test]
    fn start_external_cli_transitions_to_awaiting_external_cli() {
        let mut handler = LoginHandler::new();

        handler.start_external_cli("Gemini".to_string());

        match handler.state {
            LoginFlowState::AwaitingExternalCli { agent_name } => {
                assert_eq!(agent_name, "Gemini");
            }
            _ => panic!("Expected AwaitingExternalCli state"),
        }
    }

    #[test]
    fn check_agent_support_returns_unknown_for_invalid_agent() {
        let support = LoginHandler::check_agent_support("unknown-agent");

        match support {
            AgentLoginSupport::Unknown { model_name } => {
                assert_eq!(model_name, "unknown-agent");
            }
            _ => panic!("Expected Unknown variant for unknown-agent"),
        }
    }

    #[test]
    fn start_oauth_transitions_to_awaiting_browser_auth() {
        let mut handler = LoginHandler::new();

        handler.start_oauth();

        assert_eq!(handler.state, LoginFlowState::AwaitingBrowserAuth);
    }

    #[test]
    fn oauth_complete_transitions_to_success() {
        let mut handler = LoginHandler::new();
        handler.state = LoginFlowState::AwaitingBrowserAuth;

        handler.oauth_complete();

        assert_eq!(handler.state, LoginFlowState::Success);
    }

    #[test]
    fn cancel_transitions_to_cancelled_state() {
        let mut handler = LoginHandler::new();
        handler.state = LoginFlowState::AwaitingBrowserAuth;

        handler.cancel();

        assert_eq!(handler.state, LoginFlowState::Cancelled);
    }
}
