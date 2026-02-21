use super::*;

impl App {
    /// Persist a TUI config setting to config.toml and apply it immediately.
    pub(super) async fn persist_config_setting(&mut self, setting_name: &str, enabled: bool) {
        // Apply immediately to the running TUI
        match setting_name {
            "vertical_footer" => {
                self.vertical_footer = enabled;
                self.chat_widget.set_vertical_footer(enabled);
            }
            _ => {
                tracing::warn!("Unknown config setting: {setting_name}");
                return;
            }
        }

        // Persist to config.toml
        if let Err(err) = ConfigEditsBuilder::new(&self.config.codex_home)
            .set_path(&["tui", setting_name], toml_value(enabled))
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

        let status = if enabled { "enabled" } else { "disabled" };
        self.chat_widget
            .add_info_message(format!("{setting_name} {status}"), None);
    }

    #[cfg(feature = "nori-config")]
    pub(super) async fn persist_notify_after_idle_setting(
        &mut self,
        value: codex_acp::config::NotifyAfterIdle,
    ) {
        let toml_str = value.toml_value();

        if let Err(err) = ConfigEditsBuilder::new(&self.config.codex_home)
            .set_path(&["tui", "notify_after_idle"], toml_value(toml_str))
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

        self.chat_widget.add_info_message(
            format!(
                "Notify after idle set to {}. Changes will take effect after restart.",
                value.display_name()
            ),
            None,
        );
    }

    #[cfg(feature = "nori-config")]
    pub(super) async fn persist_script_timeout_setting(
        &mut self,
        value: codex_acp::config::ScriptTimeout,
    ) {
        let toml_str = value.toml_value();

        if let Err(err) = ConfigEditsBuilder::new(&self.config.codex_home)
            .set_path(&["tui", "script_timeout"], toml_value(toml_str))
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

        self.chat_widget.add_info_message(
            format!("Script timeout set to {}.", value.display_name()),
            None,
        );
    }

    /// Store the loop count as an ephemeral per-session override (not persisted
    /// to the TOML config). The user can still edit the home TOML directly for
    /// a persistent change.
    #[cfg(feature = "nori-config")]
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

    pub(super) async fn persist_vim_mode_setting(&mut self, enabled: bool) {
        if let Err(err) = ConfigEditsBuilder::new(&self.config.codex_home)
            .set_path(&["tui", "vim_mode"], toml_value(enabled))
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
        self.vim_mode_enabled = enabled;
        self.chat_widget.set_vim_mode_enabled(enabled);

        let status = if enabled { "enabled" } else { "disabled" };
        self.chat_widget
            .add_info_message(format!("Vim mode {status}."), None);
    }

    #[cfg(feature = "nori-config")]
    pub(super) async fn persist_auto_worktree_setting(&mut self, enabled: bool) {
        if let Err(err) = ConfigEditsBuilder::new(&self.config.codex_home)
            .set_path(&["tui", "auto_worktree"], toml_value(enabled))
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

        let status = if enabled { "enabled" } else { "disabled" };
        self.chat_widget.add_info_message(
            format!("Auto worktree {status}. Changes will take effect on next session."),
            None,
        );
    }

    #[cfg(feature = "nori-config")]
    pub(super) async fn persist_skillset_per_session_setting(&mut self, enabled: bool) {
        let mut builder = ConfigEditsBuilder::new(&self.config.codex_home);
        builder = builder.set_path(&["tui", "skillset_per_session"], toml_value(enabled));
        if enabled {
            builder = builder.set_path(&["tui", "auto_worktree"], toml_value(true));
        }
        if let Err(err) = builder.apply().await {
            tracing::error!(error = %err, "failed to persist skillset_per_session setting");
            self.chat_widget.add_error_message(format!(
                "Failed to save skillset_per_session setting: {err}"
            ));
            return;
        }
        let status = if enabled { "enabled" } else { "disabled" };
        self.chat_widget.add_info_message(
            format!("Per Session Skillsets {status}. Changes will take effect on next session."),
            None,
        );
    }

    #[cfg(feature = "nori-config")]
    pub(super) async fn persist_footer_segment_setting(
        &mut self,
        segment: codex_acp::config::FooterSegment,
        enabled: bool,
    ) {
        if let Err(err) = ConfigEditsBuilder::new(&self.config.codex_home)
            .set_path(
                &["tui", "footer_segments", segment.toml_key()],
                toml_value(enabled),
            )
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
        self.chat_widget
            .set_footer_segment_enabled(segment, enabled);

        let status = if enabled { "enabled" } else { "disabled" };
        self.chat_widget
            .add_info_message(format!("{} {status}.", segment.display_name()), None);

        // Refresh the picker to show updated state without stacking a new view.
        let nori_config = codex_acp::config::NoriConfig::load().unwrap_or_default();
        self.chat_widget
            .replace_footer_segments_picker(&nori_config.footer_segment_config);
    }

    pub(super) async fn persist_notification_setting(&mut self, setting_name: &str, enabled: bool) {
        let enum_value = if enabled { "enabled" } else { "disabled" };

        // Persist to config.toml as a string enum value
        if let Err(err) = ConfigEditsBuilder::new(&self.config.codex_home)
            .set_path(&["tui", setting_name], toml_value(enum_value))
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

        let status = if enabled { "enabled" } else { "disabled" };
        self.chat_widget
            .add_info_message(format!("{setting_name} {status}"), None);
    }

    pub(super) async fn persist_hotkey_setting(
        &mut self,
        action: codex_acp::config::HotkeyAction,
        binding: codex_acp::config::HotkeyBinding,
    ) {
        let toml_key = action.toml_key();
        let toml_val = binding.toml_value();

        if let Err(err) = ConfigEditsBuilder::new(&self.config.codex_home)
            .set_path(&["tui", "hotkeys", toml_key], toml_value(&toml_val))
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
}
