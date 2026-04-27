use super::*;

impl ChatWidget {
    /// Handle the /login slash command
    pub(super) fn handle_login_command(&mut self) {
        // Use pending agent if set (user selected via /agent picker but hasn't submitted yet),
        // otherwise use the current config agent
        let agent_name = self
            .pending_agent
            .as_ref()
            .map(|p| p.agent_name.as_str())
            .unwrap_or(&self.config.model);

        match LoginHandler::check_agent_support(agent_name) {
            AgentLoginSupport::Supported {
                agent,
                is_installed,
                login_method,
            } => {
                if !is_installed {
                    // Agent not installed - show installation instructions
                    let display_name = agent.display_name();
                    let npm_package = agent.npm_package();
                    self.add_info_message(
                        format!(
                            "{display_name} is not installed. To install, run:\n\n  npm install -g {npm_package}\n\nThen run /login again to authenticate."
                        ),
                        Some("Install the agent first, then authenticate".to_string()),
                    );
                    return;
                }

                match login_method {
                    LoginMethod::OAuthBrowser => {
                        // Create and start the login handler
                        let mut handler = LoginHandler::new();
                        handler.start_oauth();

                        // Show auth method selection message
                        self.add_info_message(
                            "Starting authentication...\n\nA browser window will open for you to sign in with your OpenAI account.\n\nAlternatively, you can set the OPENAI_API_KEY environment variable.".to_string(),
                            Some("Press Esc to cancel".to_string()),
                        );

                        // Start the actual login server
                        self.start_oauth_login_flow(handler);
                    }
                    LoginMethod::ExternalCli { command, args } => {
                        // Create and start the login handler
                        let mut handler = LoginHandler::new();
                        let agent_display_name = agent.display_name().to_string();
                        handler.start_external_cli(agent_display_name.clone());

                        // Show starting message
                        self.add_info_message(
                            format!(
                                "Starting authentication for {agent_display_name}...\n\nThe {agent_display_name} login process will run in-app.",
                            ),
                            Some("Press Esc to cancel".to_string()),
                        );

                        // Start the external CLI login flow
                        self.start_external_cli_login_flow(
                            handler,
                            command,
                            args,
                            agent_display_name,
                        );
                    }
                }
            }
            AgentLoginSupport::NotSupported { agent_name } => {
                // Provide agent-specific instructions
                let instructions = match agent_name.as_str() {
                    "Claude Code" => {
                        "In-app login for Claude Code is not yet supported.\n\n\
                         To authenticate, run `claude` in a separate terminal and use the /login command.\n\n\
                         Alternatively, set the ANTHROPIC_API_KEY environment variable."
                    }
                    _ => {
                        "In-app login for this agent is not yet supported. Please authenticate externally using the agent's native login command or API keys."
                    }
                };
                self.add_info_message(instructions.to_string(), None);
            }
            AgentLoginSupport::Unknown { agent_name } => {
                self.add_info_message(
                    format!("Unknown agent '{agent_name}'. Cannot determine login method."),
                    None,
                );
            }
        }
    }

    /// Handle the /login <agent> command with explicit agent name
    pub(super) fn handle_login_command_with_agent(&mut self, agent_name: &str) {
        match LoginHandler::check_agent_support(agent_name) {
            AgentLoginSupport::Supported {
                agent,
                is_installed,
                login_method,
            } => {
                if !is_installed {
                    let display_name = agent.display_name();
                    let npm_package = agent.npm_package();
                    self.add_info_message(
                        format!(
                            "{display_name} is not installed. To install, run:\n\n  npm install -g {npm_package}\n\nThen run /login again to authenticate."
                        ),
                        Some("Install the agent first, then authenticate".to_string()),
                    );
                    return;
                }

                match login_method {
                    LoginMethod::OAuthBrowser => {
                        let mut handler = LoginHandler::new();
                        handler.start_oauth();

                        self.add_info_message(
                            "Starting authentication...\n\nA browser window will open for you to sign in with your OpenAI account.\n\nAlternatively, you can set the OPENAI_API_KEY environment variable.".to_string(),
                            Some("Press Esc to cancel".to_string()),
                        );

                        self.start_oauth_login_flow(handler);
                    }
                    LoginMethod::ExternalCli { command, args } => {
                        let mut handler = LoginHandler::new();
                        let agent_display_name = agent.display_name().to_string();
                        handler.start_external_cli(agent_display_name.clone());

                        self.add_info_message(
                            format!(
                                "Starting authentication for {agent_display_name}...\n\nThe {agent_display_name} login process will run in-app.",
                            ),
                            Some("Press Esc to cancel".to_string()),
                        );

                        self.start_external_cli_login_flow(
                            handler,
                            command,
                            args,
                            agent_display_name,
                        );
                    }
                }
            }
            AgentLoginSupport::NotSupported { agent_name } => {
                let instructions = match agent_name.as_str() {
                    "Claude Code" => {
                        "In-app login for Claude Code is not yet supported.\n\n\
                         To authenticate, run `claude` in a separate terminal and use the /login command.\n\n\
                         Alternatively, set the ANTHROPIC_API_KEY environment variable."
                    }
                    _ => {
                        "In-app login for this agent is not yet supported. Please authenticate externally using the agent's native login command or API keys."
                    }
                };
                self.add_info_message(instructions.to_string(), None);
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
        use codex_core::auth::CLIENT_ID;
        use codex_login::ServerOptions;
        use codex_login::run_login_server;

        let opts = ServerOptions::new(
            self.config.codex_home.clone(),
            CLIENT_ID.to_string(),
            None, // No forced workspace ID
            self.config.cli_auth_credentials_store_mode,
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

    /// Start the external CLI login flow (e.g., gemini login)
    #[cfg(feature = "login")]
    pub(super) fn start_external_cli_login_flow(
        &mut self,
        mut handler: LoginHandler,
        command: String,
        args: Vec<String>,
        agent_display_name: String,
    ) {
        use std::collections::HashMap;

        let app_event_tx = self.app_event_tx.clone();
        let cwd = self.config.cwd.clone();

        // Spawn the PTY process and stream output
        let task_handle = tokio::spawn(async move {
            // Build environment - inherit current environment
            let mut env: HashMap<String, String> = std::env::vars().collect();
            // Ensure TERM is set for proper terminal behavior
            env.entry("TERM".to_string())
                .or_insert_with(|| "xterm-256color".to_string());

            match codex_utils_pty::spawn_pty_process(&command, &args, &cwd, &env, &None).await {
                Ok(spawned) => {
                    // Keep session alive so process keeps running
                    let _session = spawned.session;
                    let mut output_rx = spawned.output_rx;
                    let exit_rx = spawned.exit_rx;

                    // Spawn a task to stream output
                    let output_event_tx = app_event_tx.clone();
                    let output_task = tokio::spawn(async move {
                        loop {
                            match output_rx.recv().await {
                                Ok(data) => {
                                    // Convert bytes to string, stripping invalid UTF-8
                                    let text = String::from_utf8_lossy(&data);
                                    // Strip ANSI escape codes using a simple regex-like approach
                                    let stripped = strip_ansi_codes(&text);
                                    if !stripped.is_empty() {
                                        output_event_tx.send(AppEvent::ExternalCliLoginOutput {
                                            data: stripped,
                                        });
                                    }
                                }
                                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                                    // Receiver lagged, continue
                                    continue;
                                }
                            }
                        }
                    });

                    // Wait for process exit
                    let exit_code = exit_rx.await.unwrap_or(-1);

                    // Cancel output task
                    output_task.abort();

                    // Send completion event
                    let success = exit_code == 0;
                    app_event_tx.send(AppEvent::ExternalCliLoginComplete {
                        success,
                        agent_name: agent_display_name,
                    });
                }
                Err(e) => {
                    tracing::error!("Failed to spawn external CLI login: {e}");
                    app_event_tx.send(AppEvent::ExternalCliLoginComplete {
                        success: false,
                        agent_name: agent_display_name,
                    });
                }
            }
        });

        // Store the task handle for cancellation support
        handler.set_pty_task_handle(task_handle);
        self.login_handler = Some(handler);
    }

    /// Start the external CLI login flow (stub for non-login builds)
    #[cfg(not(feature = "login"))]
    pub(super) fn start_external_cli_login_flow(
        &mut self,
        _handler: LoginHandler,
        _command: String,
        _args: Vec<String>,
        _agent_display_name: String,
    ) {
        self.add_error_message(
            "Login feature is not enabled. Rebuild with --features login".to_string(),
        );
    }

    /// Handle login completion event
    pub(crate) fn handle_login_complete(&mut self, success: bool) {
        if let Some(mut handler) = self.login_handler.take() {
            if success {
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

    /// Handle external CLI login output (streaming text from the PTY process)
    pub(crate) fn handle_external_cli_login_output(&mut self, data: String) {
        // Display the output as an info message (append to existing or create new)
        self.add_info_message(data, None);
        self.request_redraw();
    }

    /// Handle external CLI login completion
    pub(crate) fn handle_external_cli_login_complete(&mut self, success: bool, agent_name: String) {
        if let Some(mut handler) = self.login_handler.take() {
            handler.cancel(); // Clear any handler state
        }

        if success {
            self.add_info_message(
                format!(
                    "Successfully authenticated with {agent_name}!\n\nYou can now use {agent_name}."
                ),
                None,
            );
        } else {
            self.add_info_message(format!("{agent_name} login failed or was cancelled."), None);
        }
        self.request_redraw();
    }
}
