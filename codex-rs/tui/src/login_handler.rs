//! Login handler for /login slash command.
//!
//! This module handles authentication flow for ACP agents, starting with Codex.
//! It manages:
//! - Agent detection and installation checking
//! - Binary installation prompts
//! - OAuth browser login flow
//! - API key entry flow

use codex_acp::AgentKind;
use codex_acp::list_available_agents;
use codex_core::AuthManager;
use codex_core::auth::AuthCredentialsStoreMode;
use codex_login::ShutdownHandle;
use std::path::PathBuf;
use std::sync::Arc;

/// State of the login flow
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoginFlowState {
    /// No login flow active
    Idle,
    /// Checking if agent is installed
    CheckingInstallation,
    /// Agent not installed, prompting user to install
    PromptingInstall {
        agent_name: String,
        install_command: String,
    },
    /// Installing the agent
    Installing { agent_name: String },
    /// Choosing between OAuth and API key
    ChoosingAuthMethod,
    /// OAuth flow in progress - waiting for browser
    AwaitingBrowserAuth { auth_url: String },
    /// API key entry mode
    EnteringApiKey {
        current_value: String,
        prepopulated_from_env: bool,
    },
    /// Login successful
    Success { method: AuthMethod },
    /// Login failed or cancelled
    Failed { message: String },
    /// Login cancelled by user
    Cancelled,
}

/// Authentication method used
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthMethod {
    OAuth,
    ApiKey,
}

/// Result of checking agent support for login
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentLoginSupport {
    /// Agent supports in-app login
    Supported {
        agent: AgentKind,
        is_installed: bool,
    },
    /// Agent doesn't support in-app login yet
    NotSupported { agent_name: String },
    /// Unknown agent
    Unknown { model_name: String },
}

/// Handler for the /login command flow
pub struct LoginHandler {
    /// Current state of the login flow
    state: LoginFlowState,
    /// The agent we're logging into
    agent: Option<AgentKind>,
    /// Path to store credentials
    codex_home: PathBuf,
    /// Credential storage mode
    store_mode: AuthCredentialsStoreMode,
    /// Auth manager for reloading after login
    auth_manager: Arc<AuthManager>,
    /// Shutdown handle for cancelling OAuth flow
    shutdown_handle: Option<ShutdownHandle>,
}

impl LoginHandler {
    /// Create a new login handler
    pub fn new(
        codex_home: PathBuf,
        store_mode: AuthCredentialsStoreMode,
        auth_manager: Arc<AuthManager>,
    ) -> Self {
        Self {
            state: LoginFlowState::Idle,
            agent: None,
            codex_home,
            store_mode,
            auth_manager,
            shutdown_handle: None,
        }
    }

    /// Get current state
    pub fn state(&self) -> &LoginFlowState {
        &self.state
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
                    // Codex supports in-app login
                    AgentKind::Codex => AgentLoginSupport::Supported {
                        agent: AgentKind::Codex,
                        is_installed: info.is_installed,
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

    /// Start the login flow for a given model
    pub fn start_login(&mut self, model_name: &str) -> Result<(), String> {
        match Self::check_agent_support(model_name) {
            AgentLoginSupport::Supported {
                agent,
                is_installed,
            } => {
                self.agent = Some(agent);
                if is_installed {
                    // Agent is installed, proceed to auth method selection
                    self.state = LoginFlowState::ChoosingAuthMethod;
                    Ok(())
                } else {
                    // Agent not installed, prompt for installation
                    let install_cmd = get_install_command(agent);
                    self.state = LoginFlowState::PromptingInstall {
                        agent_name: agent.display_name().to_string(),
                        install_command: install_cmd,
                    };
                    Ok(())
                }
            }
            AgentLoginSupport::NotSupported { agent_name } => Err(format!(
                "In-app login for '{agent_name}' is not yet supported. Please authenticate externally."
            )),
            AgentLoginSupport::Unknown { model_name } => Err(format!(
                "Unknown agent '{model_name}'. Cannot determine login method."
            )),
        }
    }

    /// User confirmed installation
    pub fn confirm_install(&mut self) {
        if let LoginFlowState::PromptingInstall { agent_name, .. } = &self.state {
            self.state = LoginFlowState::Installing {
                agent_name: agent_name.clone(),
            };
        }
    }

    /// Installation completed successfully
    pub fn installation_complete(&mut self) {
        if matches!(self.state, LoginFlowState::Installing { .. }) {
            self.state = LoginFlowState::ChoosingAuthMethod;
        }
    }

    /// Installation failed
    pub fn installation_failed(&mut self, error: String) {
        self.state = LoginFlowState::Failed { message: error };
    }

    /// User chose OAuth login
    pub fn choose_oauth(&mut self) {
        if matches!(self.state, LoginFlowState::ChoosingAuthMethod) {
            self.state = LoginFlowState::AwaitingBrowserAuth {
                auth_url: String::new(), // Will be set when server starts
            };
        }
    }

    /// User chose API key login
    pub fn choose_api_key(&mut self) {
        if matches!(self.state, LoginFlowState::ChoosingAuthMethod) {
            // Check for environment variable
            let env_key = codex_core::auth::read_openai_api_key_from_env();
            self.state = LoginFlowState::EnteringApiKey {
                current_value: env_key.clone().unwrap_or_default(),
                prepopulated_from_env: env_key.is_some(),
            };
        }
    }

    /// Set the auth URL when OAuth server starts
    pub fn set_auth_url(&mut self, url: String) {
        if let LoginFlowState::AwaitingBrowserAuth { auth_url } = &mut self.state {
            *auth_url = url;
        }
    }

    /// Set the shutdown handle for cancellation
    pub fn set_shutdown_handle(&mut self, handle: ShutdownHandle) {
        self.shutdown_handle = Some(handle);
    }

    /// Update API key value
    pub fn update_api_key(&mut self, value: String) {
        if let LoginFlowState::EnteringApiKey {
            current_value,
            prepopulated_from_env,
        } = &mut self.state
        {
            *current_value = value;
            *prepopulated_from_env = false;
        }
    }

    /// Submit the API key
    pub fn submit_api_key(&mut self) -> Result<(), String> {
        if let LoginFlowState::EnteringApiKey { current_value, .. } = &self.state {
            let api_key = current_value.trim();
            if api_key.is_empty() {
                return Err("API key cannot be empty".to_string());
            }

            // Save the API key
            match codex_core::auth::login_with_api_key(&self.codex_home, api_key, self.store_mode) {
                Ok(()) => {
                    self.auth_manager.reload();
                    self.state = LoginFlowState::Success {
                        method: AuthMethod::ApiKey,
                    };
                    Ok(())
                }
                Err(e) => Err(format!("Failed to save API key: {e}")),
            }
        } else {
            Err("Not in API key entry state".to_string())
        }
    }

    /// OAuth login completed successfully.
    /// Note: The caller should call auth_manager.reload() before this method.
    pub fn oauth_complete(&mut self) {
        if matches!(self.state, LoginFlowState::AwaitingBrowserAuth { .. }) {
            // auth_manager.reload() is called by the spawned task before this
            self.state = LoginFlowState::Success {
                method: AuthMethod::OAuth,
            };
        }
    }

    /// OAuth login failed
    pub fn oauth_failed(&mut self, error: String) {
        if matches!(self.state, LoginFlowState::AwaitingBrowserAuth { .. }) {
            self.state = LoginFlowState::Failed { message: error };
        }
    }

    /// Cancel the login flow
    pub fn cancel(&mut self) {
        // Shutdown OAuth server if running
        if let Some(handle) = self.shutdown_handle.take() {
            handle.shutdown();
        }
        self.state = LoginFlowState::Cancelled;
    }

    /// Reset to idle state
    pub fn reset(&mut self) {
        if let Some(handle) = self.shutdown_handle.take() {
            handle.shutdown();
        }
        self.state = LoginFlowState::Idle;
        self.agent = None;
    }

    /// Check if the flow is active (not idle, success, failed, or cancelled)
    pub fn is_active(&self) -> bool {
        !matches!(
            self.state,
            LoginFlowState::Idle
                | LoginFlowState::Success { .. }
                | LoginFlowState::Failed { .. }
                | LoginFlowState::Cancelled
        )
    }
}

/// Get the installation command for an agent
fn get_install_command(agent: AgentKind) -> String {
    let package = agent.npm_package();
    format!("npm install -g {package}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn create_test_handler() -> (LoginHandler, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let handler = LoginHandler::new(
            temp_dir.path().to_path_buf(),
            AuthCredentialsStoreMode::File,
            AuthManager::shared(
                temp_dir.path().to_path_buf(),
                false,
                AuthCredentialsStoreMode::File,
            ),
        );
        (handler, temp_dir)
    }

    #[test]
    fn check_agent_support_returns_supported_for_codex() {
        let support = LoginHandler::check_agent_support("codex");

        match support {
            AgentLoginSupport::Supported { agent, .. } => {
                assert_eq!(agent, AgentKind::Codex);
            }
            _ => panic!("Expected Supported variant for codex"),
        }
    }

    #[test]
    fn check_agent_support_returns_not_supported_for_claude() {
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
    fn start_login_returns_error_for_unsupported_agent() {
        let (mut handler, _tmp) = create_test_handler();

        let result = handler.start_login("claude-code");

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not yet supported"));
    }

    #[test]
    fn start_login_returns_error_for_unknown_agent() {
        let (mut handler, _tmp) = create_test_handler();

        let result = handler.start_login("unknown-agent");

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Unknown agent"));
    }

    #[test]
    fn start_login_for_codex_proceeds_to_next_state() {
        let (mut handler, _tmp) = create_test_handler();

        let result = handler.start_login("codex");

        assert!(result.is_ok());
        // State should be either ChoosingAuthMethod (if installed) or PromptingInstall (if not)
        assert!(matches!(
            handler.state(),
            LoginFlowState::ChoosingAuthMethod | LoginFlowState::PromptingInstall { .. }
        ));
    }

    #[test]
    fn choose_oauth_transitions_to_awaiting_browser_auth() {
        let (mut handler, _tmp) = create_test_handler();
        handler.state = LoginFlowState::ChoosingAuthMethod;

        handler.choose_oauth();

        assert!(matches!(
            handler.state(),
            LoginFlowState::AwaitingBrowserAuth { .. }
        ));
    }

    #[test]
    fn choose_api_key_transitions_to_entering_api_key() {
        let (mut handler, _tmp) = create_test_handler();
        handler.state = LoginFlowState::ChoosingAuthMethod;

        handler.choose_api_key();

        assert!(matches!(
            handler.state(),
            LoginFlowState::EnteringApiKey { .. }
        ));
    }

    #[test]
    fn submit_empty_api_key_returns_error() {
        let (mut handler, _tmp) = create_test_handler();
        handler.state = LoginFlowState::EnteringApiKey {
            current_value: "".to_string(),
            prepopulated_from_env: false,
        };

        let result = handler.submit_api_key();

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("cannot be empty"));
    }

    #[test]
    fn submit_valid_api_key_transitions_to_success() {
        let (mut handler, _tmp) = create_test_handler();
        handler.state = LoginFlowState::EnteringApiKey {
            current_value: "sk-test-key-12345".to_string(),
            prepopulated_from_env: false,
        };

        let result = handler.submit_api_key();

        assert!(result.is_ok());
        assert!(matches!(
            handler.state(),
            LoginFlowState::Success {
                method: AuthMethod::ApiKey
            }
        ));
    }

    #[test]
    fn cancel_transitions_to_cancelled_state() {
        let (mut handler, _tmp) = create_test_handler();
        handler.state = LoginFlowState::AwaitingBrowserAuth {
            auth_url: "https://example.com/auth".to_string(),
        };

        handler.cancel();

        assert!(matches!(handler.state(), LoginFlowState::Cancelled));
    }

    #[test]
    fn oauth_complete_transitions_to_success() {
        let (mut handler, _tmp) = create_test_handler();
        handler.state = LoginFlowState::AwaitingBrowserAuth {
            auth_url: "https://example.com/auth".to_string(),
        };

        handler.oauth_complete();

        assert!(matches!(
            handler.state(),
            LoginFlowState::Success {
                method: AuthMethod::OAuth
            }
        ));
    }

    #[test]
    fn is_active_returns_false_for_terminal_states() {
        let (mut handler, _tmp) = create_test_handler();

        handler.state = LoginFlowState::Idle;
        assert!(!handler.is_active());

        handler.state = LoginFlowState::Success {
            method: AuthMethod::OAuth,
        };
        assert!(!handler.is_active());

        handler.state = LoginFlowState::Failed {
            message: "test".to_string(),
        };
        assert!(!handler.is_active());

        handler.state = LoginFlowState::Cancelled;
        assert!(!handler.is_active());
    }

    #[test]
    fn is_active_returns_true_for_in_progress_states() {
        let (mut handler, _tmp) = create_test_handler();

        handler.state = LoginFlowState::ChoosingAuthMethod;
        assert!(handler.is_active());

        handler.state = LoginFlowState::AwaitingBrowserAuth {
            auth_url: "".to_string(),
        };
        assert!(handler.is_active());

        handler.state = LoginFlowState::EnteringApiKey {
            current_value: "".to_string(),
            prepopulated_from_env: false,
        };
        assert!(handler.is_active());
    }

    #[test]
    fn get_install_command_returns_correct_npm_command() {
        let cmd = get_install_command(AgentKind::Codex);
        assert_eq!(cmd, "npm install -g @openai/codex");
    }

    #[test]
    fn confirm_install_transitions_to_installing() {
        let (mut handler, _tmp) = create_test_handler();
        handler.state = LoginFlowState::PromptingInstall {
            agent_name: "Codex".to_string(),
            install_command: "npm install -g @openai/codex".to_string(),
        };

        handler.confirm_install();

        assert!(matches!(handler.state(), LoginFlowState::Installing { .. }));
    }

    #[test]
    fn installation_complete_transitions_to_choosing_auth() {
        let (mut handler, _tmp) = create_test_handler();
        handler.state = LoginFlowState::Installing {
            agent_name: "Codex".to_string(),
        };

        handler.installation_complete();

        assert!(matches!(
            handler.state(),
            LoginFlowState::ChoosingAuthMethod
        ));
    }
}
