//! Login handler for /login slash command.
//!
//! Codex supports browser login here. Other providers use their native credentials.

use codex_login::ShutdownHandle;
use nori_harness::AgentKind;

/// Result of looking up authentication for the selected distribution.
pub enum AgentLoginSupport {
    OAuthBrowser,
    NotSupported { instructions: String },
    Unknown { agent_name: String },
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

/// Tracks browser authentication and allows the user to cancel it.
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
    pub fn check_agent_support(agent_name: &str) -> AgentLoginSupport {
        match nori_harness::get_agent_config(agent_name) {
            Ok(config) => {
                let instructions = match config.builtin_kind {
                    Some(AgentKind::Gemini) => "Run agy in another terminal and choose your Google sign-in, or set GEMINI_API_KEY.".to_owned(),
                    Some(AgentKind::GrokBuild) => "Run npx --yes @xai-official/grok@latest login in another terminal, or set XAI_API_KEY.".to_owned(),
                    Some(AgentKind::ClaudeCode) => "Run claude in another terminal and use /login, or set ANTHROPIC_API_KEY.".to_owned(),
                    Some(AgentKind::OpenCode) => config.auth_hint,
                    Some(AgentKind::Codex) => return AgentLoginSupport::OAuthBrowser,
                    Some(AgentKind::Cursor) => config.auth_hint,
                    None if !config.auth_hint.is_empty() => config.auth_hint,
                    None => format!("Use the authentication method for {}.", config.display_name),
                };
                AgentLoginSupport::NotSupported { instructions }
            }
            Err(_) => AgentLoginSupport::Unknown {
                agent_name: agent_name.to_owned(),
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
        assert!(matches!(
            LoginHandler::check_agent_support("codex-acp"),
            AgentLoginSupport::OAuthBrowser
        ));
    }

    #[test]
    fn check_agent_support_returns_not_supported_for_claude() {
        // Claude Code login support will be added later
        let support = LoginHandler::check_agent_support("claude-code");

        match support {
            AgentLoginSupport::NotSupported { instructions } => {
                assert!(instructions.contains("ANTHROPIC_API_KEY"));
            }
            _ => panic!("Expected NotSupported variant for claude-code"),
        }
    }

    #[test]
    fn google_and_xai_login_identify_the_native_provider() {
        for (agent, key, command) in [
            ("agy", "GEMINI_API_KEY", "agy"),
            ("grok", "XAI_API_KEY", "@xai-official/grok@latest login"),
        ] {
            assert!(matches!(LoginHandler::check_agent_support(agent),
                AgentLoginSupport::NotSupported { instructions } if instructions.contains(key) && instructions.contains(command)));
        }
    }

    #[test]
    fn check_agent_support_returns_unknown_for_invalid_agent() {
        let support = LoginHandler::check_agent_support("unknown-agent");

        match support {
            AgentLoginSupport::Unknown { agent_name } => {
                assert_eq!(agent_name, "unknown-agent");
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
