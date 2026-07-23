use super::*;
use nori_config::NoriConfigEdits as ConfigEditsBuilder;
use std::path::Path;

async fn persist_acp_wire_recording_config(nori_home: &Path, enabled: bool) -> anyhow::Result<()> {
    ConfigEditsBuilder::new(nori_home)
        .set_path(&["acp_proxy", "enabled"], enabled)
        .apply()
        .await
}

/// Persist a session config option selection as the agent's default model.
///
/// Returns `Ok(true)` only when `config_id` names the agent's Model-category
/// option and the selection was written to `[default_models]` in config.toml.
/// Non-model options (mode, thought level, ...) are not persisted.
pub(super) async fn persist_default_model_selection(
    nori_home: &Path,
    agent: &str,
    config_id: &str,
    value: &str,
    config_options: &[nori_protocol::acp::v1::SessionConfigOption],
) -> anyhow::Result<bool> {
    let is_model_option = config_options.iter().any(|option| {
        option.id.to_string() == config_id
            && option.category == Some(nori_protocol::acp::v1::SessionConfigOptionCategory::Model)
    });
    if !is_model_option {
        return Ok(false);
    }

    ConfigEditsBuilder::new(nori_home)
        .set_default_model(agent, value)
        .apply()
        .await?;
    Ok(true)
}

impl App {
    fn sync_runtime_config(&mut self) {
        self.chat_widget.set_config(self.config.clone());
    }

    pub(super) async fn persist_default_model_selection(
        &mut self,
        agent: &str,
        config_id: &str,
        value: &str,
        config_options: &[nori_protocol::acp::v1::SessionConfigOption],
    ) -> anyhow::Result<bool> {
        let persisted = persist_default_model_selection(
            &self.config.nori_home,
            agent,
            config_id,
            value,
            config_options,
        )
        .await?;
        if persisted {
            self.config
                .default_models
                .insert(agent.to_string(), value.to_string());
            self.sync_runtime_config();
        }
        Ok(persisted)
    }

    /// Persist a TUI config setting to config.toml and apply it immediately.
    pub(super) async fn persist_config_setting(&mut self, setting_name: &str, enabled: bool) {
        match setting_name {
            "vertical_footer" => {}
            _ => {
                tracing::warn!("Unknown config setting: {setting_name}");
                return;
            }
        }

        // Persist to config.toml
        if let Err(err) = ConfigEditsBuilder::new(&self.config.nori_home)
            .set_path(&["tui", setting_name], enabled)
            .apply()
            .await
        {
            tracing::error!(
                error = %err,
                setting = %setting_name,
                "failed to persist TUI config setting"
            );
            self.chat_widget
                .add_error_message(format!("Failed to save {setting_name} setting: {err}"));
            return;
        }

        self.config.vertical_footer = enabled;
        self.sync_runtime_config();
        self.vertical_footer = enabled;
        self.chat_widget.set_vertical_footer(enabled);

        let status = if enabled { "enabled" } else { "disabled" };
        self.chat_widget
            .add_info_message(format!("{setting_name} {status}"), None);
    }

    pub(super) async fn persist_notify_after_idle_setting(
        &mut self,
        value: nori_config::NotifyAfterIdle,
    ) {
        let toml_str = value.toml_value();

        if let Err(err) = ConfigEditsBuilder::new(&self.config.nori_home)
            .set_path(&["tui", "notify_after_idle"], toml_str)
            .apply()
            .await
        {
            tracing::error!(
                error = %err,
                "failed to persist notify_after_idle setting"
            );
            self.chat_widget
                .add_error_message(format!("Failed to save notify_after_idle setting: {err}"));
            return;
        }

        self.config.notify_after_idle = value;
        self.sync_runtime_config();
        self.chat_widget.add_info_message(
            format!("Notify after idle set to {}.", value.display_name()),
            None,
        );
    }

    pub(super) async fn persist_script_timeout_setting(
        &mut self,
        value: nori_config::ScriptTimeout,
    ) {
        let toml_str = value.toml_value();

        if let Err(err) = ConfigEditsBuilder::new(&self.config.nori_home)
            .set_path(&["tui", "script_timeout"], toml_str)
            .apply()
            .await
        {
            tracing::error!(
                error = %err,
                "failed to persist script_timeout setting"
            );
            self.chat_widget
                .add_error_message(format!("Failed to save script_timeout setting: {err}"));
            return;
        }

        self.config.script_timeout = value.clone();
        self.sync_runtime_config();
        self.chat_widget.add_info_message(
            format!("Script timeout set to {}.", value.display_name()),
            None,
        );
    }

    /// Store the loop count as an ephemeral per-session override (not persisted
    /// to the TOML config). The user can still edit the home TOML directly for
    /// a persistent change.
    pub(super) fn set_session_loop_count(&mut self, value: Option<i32>) {
        self.loop_count_override = Some(value);
        self.chat_widget.set_loop_count_override(Some(value));

        let display = match value {
            Some(n) => format!("{n}"),
            None => "Disabled".to_string(),
        };
        self.chat_widget
            .add_info_message(format!("Loop count set to {display} (this session)."), None);
    }

    pub(super) async fn persist_vim_mode_setting(&mut self, value: nori_config::VimEnterBehavior) {
        if let Err(err) = ConfigEditsBuilder::new(&self.config.nori_home)
            .set_path(&["tui", "vim_mode"], value.toml_value())
            .apply()
            .await
        {
            tracing::error!(
                error = %err,
                "failed to persist vim_mode setting"
            );
            self.chat_widget
                .add_error_message(format!("Failed to save vim_mode setting: {err}"));
            return;
        }

        // Update in-memory state and propagate to the chat widget
        self.config.vim_mode = value;
        self.sync_runtime_config();
        self.vim_mode = value;
        self.chat_widget.set_vim_mode(value);

        let display = value.display_name();
        self.chat_widget
            .add_info_message(format!("Vim mode: {display}."), None);
    }

    pub(super) async fn persist_auto_worktree_setting(&mut self, value: nori_config::AutoWorktree) {
        let toml_str = value.toml_value();

        if let Err(err) = ConfigEditsBuilder::new(&self.config.nori_home)
            .set_path(&["tui", "auto_worktree"], toml_str)
            .apply()
            .await
        {
            tracing::error!(
                error = %err,
                "failed to persist auto_worktree setting"
            );
            self.chat_widget
                .add_error_message(format!("Failed to save auto_worktree setting: {err}"));
            return;
        }

        self.config.auto_worktree = value;
        self.sync_runtime_config();
        self.chat_widget.add_info_message(
            format!(
                "Auto worktree set to {}. Changes will take effect on next session.",
                value.display_name()
            ),
            None,
        );
    }

    pub(super) async fn persist_pinned_plan_drawer_setting(&mut self, enabled: bool) {
        if let Err(err) = ConfigEditsBuilder::new(&self.config.nori_home)
            .set_path(&["tui", "pinned_plan_drawer"], enabled)
            .apply()
            .await
        {
            tracing::error!(error = %err, "failed to persist pinned_plan_drawer setting");
            self.chat_widget
                .add_error_message(format!("Failed to save pinned_plan_drawer setting: {err}"));
            return;
        }
        self.config.pinned_plan_drawer = enabled;
        self.sync_runtime_config();
        let mode = if enabled {
            crate::chatwidget::PlanDrawerMode::Expanded
        } else {
            crate::chatwidget::PlanDrawerMode::Off
        };
        self.plan_drawer_mode = mode;
        self.chat_widget.set_plan_drawer_mode(mode);
        let status = if enabled { "enabled" } else { "disabled" };
        self.chat_widget
            .add_info_message(format!("Pinned plan drawer {status}."), None);
    }

    pub(super) async fn persist_acp_wire_recording_setting(&mut self, enabled: bool) {
        if let Err(err) = persist_acp_wire_recording_config(&self.config.nori_home, enabled).await {
            tracing::error!(error = %err, "failed to persist acp wire recording setting");
            self.chat_widget
                .add_error_message(format!("Failed to save ACP wire recording setting: {err}"));
            return;
        }

        self.config.acp_proxy.enabled = enabled;
        self.sync_runtime_config();
        self.chat_widget.set_acp_wire_recording_enabled(enabled);
        self.chat_widget.replace_agent_popup(enabled);
        let status = if enabled { "enabled" } else { "disabled" };
        self.chat_widget
            .add_info_message(format!("ACP wire recording {status}."), None);
    }

    pub(super) async fn persist_custom_working_messages_setting(&mut self, enabled: bool) {
        if let Err(err) = ConfigEditsBuilder::new(&self.config.nori_home)
            .set_path(&["tui", "custom_working_messages"], enabled)
            .apply()
            .await
        {
            tracing::error!(error = %err, "failed to persist custom_working_messages setting");
            self.chat_widget.add_error_message(format!(
                "Failed to save custom_working_messages setting: {err}"
            ));
            return;
        }
        self.config.custom_working_messages = enabled;
        self.sync_runtime_config();
        self.chat_widget.set_custom_working_messages(enabled);
        let status = if enabled { "enabled" } else { "disabled" };
        self.chat_widget
            .add_info_message(format!("Custom working messages {status}."), None);
    }

    pub(super) async fn persist_skillset_per_session_setting(&mut self, enabled: bool) {
        let builder = ConfigEditsBuilder::new(&self.config.nori_home)
            .set_path(&["tui", "skillset_per_session"], enabled);
        if let Err(err) = builder.apply().await {
            tracing::error!(error = %err, "failed to persist skillset_per_session setting");
            self.chat_widget.add_error_message(format!(
                "Failed to save skillset_per_session setting: {err}"
            ));
            return;
        }
        self.config.skillset_per_session = enabled;
        self.sync_runtime_config();
        let status = if enabled { "enabled" } else { "disabled" };
        self.chat_widget.add_info_message(
            format!("Per Session Skillsets {status}. Changes will take effect on next session."),
            None,
        );
    }

    pub(super) async fn persist_footer_segment_setting(
        &mut self,
        segment: nori_config::FooterSegment,
        enabled: bool,
    ) {
        if let Err(err) = ConfigEditsBuilder::new(&self.config.nori_home)
            .set_path(&["tui", "footer_segments", segment.toml_key()], enabled)
            .apply()
            .await
        {
            tracing::error!(
                error = %err,
                "failed to persist footer_segment setting"
            );
            self.chat_widget
                .add_error_message(format!("Failed to save footer segment setting: {err}"));
            return;
        }

        // Update the local config and apply to the widget
        self.footer_segment_config.set_enabled(segment, enabled);
        self.config.footer_segment_config = self.footer_segment_config.clone();
        self.sync_runtime_config();
        self.chat_widget
            .set_footer_segment_enabled(segment, enabled);

        let status = if enabled { "enabled" } else { "disabled" };
        self.chat_widget
            .add_info_message(format!("{} {status}.", segment.display_name()), None);

        // Refresh the picker to show updated state without stacking a new view.
        self.chat_widget
            .replace_footer_segments_picker(&self.footer_segment_config);
    }

    pub(super) async fn persist_file_manager_setting(&mut self, value: nori_config::FileManager) {
        if let Err(err) = ConfigEditsBuilder::new(&self.config.nori_home)
            .set_path(&["tui", "file_manager"], value.command_name())
            .apply()
            .await
        {
            tracing::error!(error = %err, "failed to persist file_manager setting");
            self.chat_widget
                .add_error_message(format!("Failed to save file_manager setting: {err}"));
            return;
        }

        self.config.file_manager = Some(value);
        self.sync_runtime_config();
        self.chat_widget.add_info_message(
            format!("File manager set to {}.", value.display_name()),
            None,
        );
    }

    pub(super) async fn persist_notification_setting(&mut self, setting_name: &str, enabled: bool) {
        if !matches!(setting_name, "terminal_notifications" | "os_notifications") {
            tracing::warn!("Unknown notification setting: {setting_name}");
            return;
        }
        let enum_value = if enabled { "enabled" } else { "disabled" };

        // Persist to config.toml as a string enum value
        if let Err(err) = ConfigEditsBuilder::new(&self.config.nori_home)
            .set_path(&["tui", setting_name], enum_value)
            .apply()
            .await
        {
            tracing::error!(
                error = %err,
                setting = %setting_name,
                "failed to persist TUI notification setting"
            );
            self.chat_widget
                .add_error_message(format!("Failed to save {setting_name} setting: {err}"));
            return;
        }

        match setting_name {
            "terminal_notifications" => {
                self.config.terminal_notifications = if enabled {
                    nori_config::TerminalNotifications::Enabled
                } else {
                    nori_config::TerminalNotifications::Disabled
                };
            }
            "os_notifications" => {
                self.config.os_notifications = if enabled {
                    nori_config::OsNotifications::Enabled
                } else {
                    nori_config::OsNotifications::Disabled
                };
            }
            _ => unreachable!("notification setting was validated before persistence"),
        }
        self.sync_runtime_config();
        let status = if enabled { "enabled" } else { "disabled" };
        self.chat_widget
            .add_info_message(format!("{setting_name} {status}"), None);
    }

    pub(super) async fn persist_hotkey_setting(
        &mut self,
        action: nori_config::HotkeyAction,
        binding: nori_config::HotkeyBinding,
    ) {
        let toml_key = action.toml_key();
        let toml_val = binding.toml_value();

        if let Err(err) = ConfigEditsBuilder::new(&self.config.nori_home)
            .set_path(&["tui", "hotkeys", toml_key], toml_val)
            .apply()
            .await
        {
            tracing::error!(
                error = %err,
                action = %action.display_name(),
                "failed to persist hotkey setting"
            );
            self.chat_widget.add_error_message(format!(
                "Failed to save hotkey for {}: {err}",
                action.display_name()
            ));
            return;
        }

        self.hotkey_config.set_binding(action, binding.clone());
        self.config.hotkeys = self.hotkey_config.clone();
        self.sync_runtime_config();
        self.chat_widget
            .set_hotkey_config(self.hotkey_config.clone());
        self.chat_widget.add_info_message(
            format!(
                "{} bound to {}",
                action.display_name(),
                binding.display_name()
            ),
            None,
        );
    }

    pub(super) async fn persist_mcp_servers(
        &mut self,
        servers: std::collections::BTreeMap<String, nori_config::McpServerConfig>,
    ) {
        if let Err(err) = ConfigEditsBuilder::new(&self.config.nori_home)
            .replace_mcp_servers(&servers)
            .apply()
            .await
        {
            tracing::error!(error = %err, "failed to persist MCP servers");
            self.chat_widget
                .add_error_message(format!("Failed to save MCP servers: {err}"));
            return;
        }

        // Sync in-memory state so that ComputeMcpAuthStatuses (which reads
        // chat_widget.config_ref().mcp_servers) sees the newly added servers.
        self.config.mcp_servers = servers.into_iter().collect();
        self.sync_runtime_config();

        self.chat_widget.add_info_message(
            "MCP servers saved. Start a new session to use them.".to_string(),
            None,
        );
    }

    /// Start an async MCP OAuth login flow (no TUI suspension).
    #[allow(clippy::too_many_arguments)]
    pub(super) async fn perform_mcp_oauth_login(
        &mut self,
        _tui: &mut crate::tui::Tui,
        server_name: String,
        server_url: String,
        http_headers: Option<std::collections::HashMap<String, String>>,
        env_http_headers: Option<std::collections::HashMap<String, String>>,
        client_id: Option<String>,
        client_secret_env_var: Option<String>,
    ) {
        // Resolve client_secret from env var if provided.
        let client_secret = client_secret_env_var.as_deref().and_then(|var| {
            match std::env::var(var) {
                Ok(val) => Some(val),
                Err(_) => {
                    tracing::warn!(
                        "MCP OAuth: client_secret_env_var `{var}` is not set for `{server_name}`"
                    );
                    None
                }
            }
        });

        let app_event_tx = self.app_event_tx.clone();
        let name_for_task = server_name.clone();

        match codex_rmcp_client::start_oauth_login(
            server_name.clone(),
            server_url,
            codex_rmcp_client::OAuthCredentialsStoreMode::Auto,
            http_headers,
            env_http_headers,
            vec![],
            client_id,
            client_secret,
        )
        .await
        {
            Ok(handle) => {
                self.chat_widget.add_info_message(
                    mcp_oauth_login_started_message(&server_name, &handle.authorization_url),
                    Some("Press Esc in the MCP picker to cancel".to_string()),
                );

                // Store the cancel sender and spawn the task watcher.
                self.mcp_oauth_cancel_tx = handle.cancel_tx;
                let task = handle.task;

                let tx = app_event_tx;
                tokio::spawn(async move {
                    let result = task.await;
                    let (success, error) = match result {
                        Ok(Ok(())) => (true, None),
                        Ok(Err(e)) => (false, Some(format_mcp_oauth_error(&e))),
                        Err(e) => (false, Some(format!("OAuth task panicked: {e}"))),
                    };
                    tx.send(crate::app_event::AppEvent::McpOAuthLoginComplete {
                        server_name: name_for_task,
                        success,
                        error,
                    });
                });
            }
            Err(err) => {
                // Send completion event — the handler will display the error
                // and forward to the picker.
                app_event_tx.send(crate::app_event::AppEvent::McpOAuthLoginComplete {
                    server_name: name_for_task,
                    success: false,
                    error: Some(err.to_string()),
                });
            }
        }
    }

    /// Cancel the in-progress MCP OAuth login flow.
    pub(super) fn cancel_mcp_oauth_login(&mut self) {
        if let Some(tx) = self.mcp_oauth_cancel_tx.take() {
            let _ = tx.send(());
        }
    }
}

fn mcp_oauth_login_started_message(server_name: &str, authorization_url: &str) -> String {
    format!(
        "Opening browser to authenticate `{server_name}`...\n\nIf the browser doesn't open automatically, visit:\n{authorization_url}\n\nWaiting for authentication to complete..."
    )
}

fn format_mcp_oauth_error(error: &anyhow::Error) -> String {
    format!("{error:#}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn mcp_oauth_login_started_message_includes_manual_url() {
        let message = mcp_oauth_login_started_message("linear", "https://linear.example.com/oauth");

        assert!(message.contains("Opening browser to authenticate `linear`"));
        assert!(message.contains("If the browser doesn't open automatically, visit:"));
        assert!(message.contains("https://linear.example.com/oauth"));
        assert!(message.contains("Waiting for authentication to complete"));
    }

    #[test]
    fn mcp_oauth_error_message_includes_error_chain() {
        let err = anyhow::anyhow!("OAuth token exchange failed: server returned 400")
            .context("failed to handle OAuth callback");

        let message = format_mcp_oauth_error(&err);

        assert_eq!(
            message,
            "failed to handle OAuth callback: OAuth token exchange failed: server returned 400"
        );
    }

    fn session_config_options_with_model() -> Vec<nori_protocol::acp::v1::SessionConfigOption> {
        vec![
            nori_protocol::acp::v1::SessionConfigOption::select(
                "model",
                "Model",
                "sonnet",
                vec![
                    nori_protocol::acp::v1::SessionConfigSelectOption::new("sonnet", "Sonnet"),
                    nori_protocol::acp::v1::SessionConfigSelectOption::new("opus", "Opus"),
                ],
            )
            .category(nori_protocol::acp::v1::SessionConfigOptionCategory::Model),
            nori_protocol::acp::v1::SessionConfigOption::select(
                "permission-mode",
                "Mode",
                "default",
                vec![
                    nori_protocol::acp::v1::SessionConfigSelectOption::new("default", "Default"),
                    nori_protocol::acp::v1::SessionConfigSelectOption::new(
                        "acceptEdits",
                        "Accept Edits",
                    ),
                ],
            )
            .category(nori_protocol::acp::v1::SessionConfigOptionCategory::Mode),
        ]
    }

    #[tokio::test]
    async fn model_selection_persists_default_model_for_agent() {
        let temp = TempDir::new().expect("temp home");

        let persisted = persist_default_model_selection(
            temp.path(),
            "claude-code",
            "model",
            "opus",
            &session_config_options_with_model(),
        )
        .await
        .expect("persist model selection");

        assert!(persisted, "model selection should be persisted");
        let content = std::fs::read_to_string(temp.path().join("config.toml"))
            .expect("read persisted config");
        let parsed: toml::Value = toml::from_str(&content).expect("config toml");
        assert_eq!(
            parsed
                .get("default_models")
                .and_then(|section| section.get("claude-code"))
                .and_then(toml::Value::as_str),
            Some("opus")
        );
    }

    #[tokio::test]
    async fn non_model_selection_is_not_persisted() {
        let temp = TempDir::new().expect("temp home");

        let persisted = persist_default_model_selection(
            temp.path(),
            "claude-code",
            "permission-mode",
            "acceptEdits",
            &session_config_options_with_model(),
        )
        .await
        .expect("handle mode selection");

        assert!(!persisted, "mode selection should not be persisted");
        assert!(
            !temp.path().join("config.toml").exists(),
            "no config file should be written for non-model selections"
        );
    }

    #[tokio::test]
    async fn unknown_config_id_is_not_persisted() {
        let temp = TempDir::new().expect("temp home");

        let persisted = persist_default_model_selection(
            temp.path(),
            "claude-code",
            "not-a-real-option",
            "opus",
            &session_config_options_with_model(),
        )
        .await
        .expect("handle unknown option");

        assert!(!persisted, "unknown options should not be persisted");
        assert!(!temp.path().join("config.toml").exists());
    }

    #[tokio::test]
    async fn persists_acp_wire_recording_to_top_level_acp_proxy_section() {
        let temp = TempDir::new().expect("temp home");

        persist_acp_wire_recording_config(temp.path(), true)
            .await
            .expect("persist recording enabled");

        let content = std::fs::read_to_string(temp.path().join("config.toml"))
            .expect("read persisted config");
        let parsed: toml::Value = toml::from_str(&content).expect("config toml");
        assert_eq!(
            parsed
                .get("acp_proxy")
                .and_then(|section| section.get("enabled"))
                .and_then(toml::Value::as_bool),
            Some(true)
        );
    }
}
