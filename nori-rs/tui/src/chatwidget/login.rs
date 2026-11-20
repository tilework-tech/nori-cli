use super::*;

use std::path::Path;

impl ChatWidget {
    /// Handle the /login slash command
    pub(super) fn handle_login_command(&mut self) {
        let agent_name = self.login_target_agent().to_string();
        self.handle_login_command_with_agent(&agent_name);
    }

    pub(super) fn login_target_agent(&self) -> &str {
        self.login_agent_override
            .as_deref()
            .unwrap_or(&self.config.active_agent)
    }

    pub(crate) fn set_login_agent_override(&mut self, agent_name: Option<String>) {
        self.login_agent_override = agent_name;
    }

    /// Handle the /login <agent> command with explicit agent name
    pub(super) fn handle_login_command_with_agent(&mut self, agent_name: &str) {
        match LoginHandler::check_agent_support(agent_name) {
            AgentLoginSupport::OAuthBrowser => {
                let mut handler = LoginHandler::new();
                handler.start_oauth();

                self.add_info_message(
                            "Starting authentication...\n\nA browser window will open for you to sign in with your OpenAI account.\n\nAlternatively, you can set the OPENAI_API_KEY environment variable.".to_string(),
                            Some("Press Esc to cancel".to_string()),
                        );

                self.start_oauth_login_flow(handler);
            }
            AgentLoginSupport::NotSupported { instructions } => {
                self.add_info_message(instructions, None);
            }
            AgentLoginSupport::Unknown { agent_name } => {
                self.add_info_message(
                    format!("Unknown agent '{agent_name}'. Cannot determine login method."),
                    None,
                );
            }
        }
    }

    /// Start the OAuth login flow
    pub(super) fn start_oauth_login_flow(&mut self, mut handler: LoginHandler) {
        use codex_login::CLIENT_ID;
        use codex_login::ServerOptions;
        use codex_login::run_login_server;

        let credentials_home =
            codex_oauth_credentials_home(&self.config.nori_home, dirs::home_dir());
        let opts = ServerOptions::new(
            credentials_home,
            CLIENT_ID.to_string(),
            None, // No forced workspace ID
            codex_login::AuthCredentialsStoreMode::File,
        );

        match run_login_server(opts) {
            Ok(child) => {
                let auth_url = child.auth_url.clone();
                handler.set_shutdown_handle(child.cancel_handle());

                // Store the handler
                self.login_handler = Some(handler);

                // Update the info message with the URL
                self.add_info_message(
                    format!(
                        "Opening browser for authentication...\n\nIf the browser doesn't open automatically, visit:\n{auth_url}\n\nWaiting for authentication to complete..."
                    ),
                    Some("Press Esc to cancel".to_string()),
                );

                // Spawn a task to wait for completion
                let app_event_tx = self.app_event_tx.clone();
                let auth_manager = self.auth_manager.clone();
                tokio::spawn(async move {
                    match child.block_until_done().await {
                        Ok(()) => {
                            auth_manager.reload();
                            app_event_tx.send(AppEvent::LoginComplete { success: true });
                        }
                        Err(e) => {
                            tracing::error!("OAuth login failed: {e}");
                            app_event_tx.send(AppEvent::LoginComplete { success: false });
                        }
                    }
                });
            }
            Err(e) => {
                self.add_error_message(format!("Failed to start login server: {e}"));
            }
        }
    }

    pub(crate) fn handle_login_complete(&mut self, success: bool) {
        if let Some(mut handler) = self.login_handler.take() {
            if success {
                self.login_agent_override = None;
                handler.oauth_complete();
                self.add_info_message(
                    "Successfully authenticated with OpenAI!\n\nYou can now use Nori.".to_string(),
                    None,
                );
            } else {
                handler.cancel();
                self.add_info_message("Login cancelled or failed.".to_string(), None);
            }
        }
        self.request_redraw();
    }
}

/// Directory the Codex OAuth login writes credentials to.
///
/// This must match the directory the codex-acp subprocess reads auth from. That
/// subprocess is spawned with `CODEX_HOME` stripped (see the connection spawn in
/// `nori-acp-host`), so it resolves its home to `$HOME/.codex`. Writing anywhere
/// else (for example `nori_home`) means the agent never sees the credentials.
/// Falls back to `nori_home` only when the home directory cannot be determined.
fn codex_oauth_credentials_home(nori_home: &Path, home_dir: Option<PathBuf>) -> PathBuf {
    home_dir
        .map(|home| home.join(".codex"))
        .unwrap_or_else(|| nori_home.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::codex_oauth_credentials_home;
    use pretty_assertions::assert_eq;
    use std::path::PathBuf;

    #[test]
    fn oauth_login_targets_codex_agent_home_when_home_known() {
        let nori_home = PathBuf::from("/example/home/.nori/cli");
        let home_dir = PathBuf::from("/example/home");

        let destination = codex_oauth_credentials_home(&nori_home, Some(home_dir));

        assert_eq!(destination, PathBuf::from("/example/home/.codex"));
    }

    #[test]
    fn oauth_login_falls_back_to_nori_home_when_home_unknown() {
        let nori_home = PathBuf::from("/example/home/.nori/cli");

        let destination = codex_oauth_credentials_home(&nori_home, None);

        assert_eq!(destination, nori_home);
    }
}
