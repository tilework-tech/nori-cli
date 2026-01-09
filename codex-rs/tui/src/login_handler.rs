//! Login handler for /login slash command.
//!
//! This module handles authentication flows for ACP agents:
//! - OAuth browser flow (Codex)
//! - External CLI passthrough (Gemini, Claude Code)

use codex_acp::AgentKind;
use codex_acp::list_available_agents;
use codex_login::ShutdownHandle;

/// Method used for authentication
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoginMethod {
    /// OAuth browser flow - starts local server, opens browser
    OAuthBrowser,
    // Note: External CLI passthrough support is planned for future implementation
    // when input forwarding to PTY is added. For now, agents that require interactive
    // CLI auth (like Gemini) show instructions instead.
}

/// State of the login flow
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoginFlowState {
    /// No login flow active
    Idle,
    /// OAuth flow in progress - waiting for browser
    AwaitingBrowserAuth,
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
}

impl LoginHandler {
    /// Create a new login handler
    pub fn new() -> Self {
        Self {
            state: LoginFlowState::Idle,
            shutdown_handle: None,
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
                    // Gemini requires interactive CLI for auth - show instructions
                    AgentKind::Gemini => AgentLoginSupport::NotSupported {
                        agent_name: "Gemini".to_string(),
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

    /// Set the shutdown handle for cancellation
    pub fn set_shutdown_handle(&mut self, handle: ShutdownHandle) {
        self.shutdown_handle = Some(handle);
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
    fn check_agent_support_returns_not_supported_for_gemini() {
        // Gemini requires interactive CLI for auth, so we show instructions instead
        let support = LoginHandler::check_agent_support("gemini");

        match support {
            AgentLoginSupport::NotSupported { agent_name } => {
                assert_eq!(agent_name, "Gemini");
            }
            _ => panic!("Expected NotSupported variant for gemini"),
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
