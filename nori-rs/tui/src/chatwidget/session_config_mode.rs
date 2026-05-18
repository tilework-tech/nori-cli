use super::*;
use std::sync::atomic::AtomicI64;
use std::sync::atomic::Ordering;

static ACP_MODE_CONFIG_GENERATION: AtomicI64 = AtomicI64::new(1);

pub(super) fn next_acp_mode_config_generation() -> i64 {
    ACP_MODE_CONFIG_GENERATION.fetch_add(1, Ordering::Relaxed)
}

impl ChatWidget {
    #[cfg(test)]
    pub(crate) fn acp_mode_config_generation(&self) -> i64 {
        self.acp_mode_config_generation
    }

    pub(crate) fn apply_acp_mode_config_snapshot(
        &mut self,
        generation: i64,
        mode: Option<crate::nori::session_config_mode::AcpModeConfig>,
    ) {
        if generation != self.acp_mode_config_generation {
            return;
        }

        self.bottom_pane
            .set_acp_mode_label(mode.as_ref().map(|mode| mode.current_label.clone()));
        self.acp_mode_config = mode;
        self.request_redraw();
    }

    pub(super) fn handle_acp_session_mode_changed(&mut self, current_mode_id: &str) {
        // Dedup: the picker path optimistically updates `acp_mode_config` before
        // dispatching the request, so by the time the agent echoes its
        // `CurrentModeUpdate` the cache already reflects the new value. Skip
        // the duplicate history line (the picker's "Mode set to: X" cell
        // already confirmed the change). Agent-autonomous mode changes still
        // render because the cache is on the previous value.
        if let Some(mode) = self.acp_mode_config.as_ref()
            && mode.current_value() == current_mode_id
        {
            self.refresh_acp_mode_config_snapshot();
            return;
        }

        let label = self
            .acp_mode_config
            .as_ref()
            .and_then(|mode| mode.label_for_value(current_mode_id))
            .map(str::to_string)
            .unwrap_or_else(|| current_mode_id.to_string());

        let agent_display_name = nori_acp::get_agent_display_name(&self.config.model);
        self.add_to_history(
            crate::nori::agent_mode_history::new_agent_mode_changed_cell(
                &agent_display_name,
                &label,
            ),
        );
        self.refresh_acp_mode_config_snapshot();
        self.request_redraw();
    }

    pub(super) fn refresh_acp_mode_config_snapshot(&self) {
        let Some(handle) = self.acp_handle.clone() else {
            return;
        };
        let generation = self.acp_mode_config_generation;
        let app_event_tx = self.app_event_tx.clone();
        tokio::spawn(async move {
            let Some(config_options) = handle.get_session_config().await else {
                return;
            };
            app_event_tx.send(AppEvent::AcpSessionConfigSnapshot {
                generation,
                config_options,
            });
        });
    }

    pub(super) fn cycle_acp_mode_config(&mut self) -> bool {
        let Some(handle) = self.acp_handle.clone() else {
            return false;
        };
        let generation = self.acp_mode_config_generation;
        let app_event_tx = self.app_event_tx.clone();

        if let Some(mode) = self.acp_mode_config.clone() {
            let next_mode = mode.advanced();
            let config_id = mode.config_id;
            let value = mode.next_value;
            let value_name = mode.next_label;
            self.apply_acp_mode_config_snapshot(generation, Some(next_mode));
            tokio::spawn(async move {
                match handle.set_session_config_option(config_id, value).await {
                    Ok(config_options) => {
                        app_event_tx.send(AppEvent::AcpModeConfigSnapshot {
                            generation,
                            mode: crate::nori::session_config_mode::acp_mode_config_from_options(
                                &config_options,
                            ),
                        });
                        app_event_tx.send(AppEvent::AcpSessionConfigSetResult {
                            success: true,
                            option_name: "Mode".to_string(),
                            value_name,
                            config_options: Some(config_options),
                            error: None,
                        });
                    }
                    Err(err) => {
                        app_event_tx.send(AppEvent::AcpSessionConfigSetResult {
                            success: false,
                            option_name: "Mode".to_string(),
                            value_name: value_name.clone(),
                            config_options: None,
                            error: Some(err.to_string()),
                        });
                        if let Some(config_options) = handle.get_session_config().await {
                            app_event_tx.send(AppEvent::AcpModeConfigSnapshot {
                                generation,
                                mode:
                                    crate::nori::session_config_mode::acp_mode_config_from_options(
                                        &config_options,
                                    ),
                            });
                        }
                    }
                }
            });
            return true;
        }

        tokio::spawn(async move {
            let Some(config_options) = handle.get_session_config().await else {
                return;
            };
            let Some(mode) =
                crate::nori::session_config_mode::acp_mode_config_from_options(&config_options)
            else {
                app_event_tx.send(AppEvent::AcpModeConfigSnapshot {
                    generation,
                    mode: None,
                });
                return;
            };

            match handle
                .set_session_config_option(mode.config_id.clone(), mode.next_value.clone())
                .await
            {
                Ok(config_options) => {
                    let value_name = mode.next_label.clone();
                    app_event_tx.send(AppEvent::AcpModeConfigSnapshot {
                        generation,
                        mode: crate::nori::session_config_mode::acp_mode_config_from_options(
                            &config_options,
                        ),
                    });
                    app_event_tx.send(AppEvent::AcpSessionConfigSetResult {
                        success: true,
                        option_name: "Mode".to_string(),
                        value_name,
                        config_options: Some(config_options),
                        error: None,
                    });
                }
                Err(err) => {
                    app_event_tx.send(AppEvent::AcpSessionConfigSetResult {
                        success: false,
                        option_name: "Mode".to_string(),
                        value_name: mode.next_label,
                        config_options: None,
                        error: Some(err.to_string()),
                    });
                }
            }
        });
        true
    }
}
