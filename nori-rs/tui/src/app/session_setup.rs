use super::*;

pub(super) fn onboarding_resume_event(
    sessions: Vec<nori_protocol::acp::v1::SessionInfo>,
) -> Option<AppEvent> {
    sessions.into_iter().find_map(|session| {
        let is_onboarding = session
            .meta
            .as_ref()
            .and_then(|meta| meta.get("nori"))
            .and_then(|nori| nori.get("purpose"))
            .and_then(serde_json::Value::as_str)
            == Some("onboarding");
        is_onboarding.then(|| AppEvent::ResumeAcpSession {
            acp_session_id: session.session_id.to_string(),
            title: session.title.filter(|title| !title.is_empty()),
        })
    })
}

impl App {
    /// Kick off the pre-session agent probe (spawn → initialize →
    /// `session/list` → teardown; never `session/new`) and report the result
    /// as [`AppEvent::AgentSessionListProbed`]. Used by picker-first entry
    /// (`nori cloud`), the post-`/close` return to the picker, and /resume
    /// retries on a deferred widget. Bounded by a wall-clock timeout so a
    /// hung broker can never wedge the boot with no way forward.
    pub(crate) fn begin_agent_session_probe(
        &mut self,
        intent: crate::app_event::AgentSessionProbeIntent,
    ) {
        if self.agent_session_probe_in_flight {
            return;
        }
        self.agent_session_probe_in_flight = true;
        let display_name = nori_harness::get_agent_display_name(&self.config.active_agent);
        self.chat_widget
            .add_info_message(format!("Listing {display_name} sessions…"), None);

        let agent = self.config.active_agent.clone();
        let cwd = self.config.cwd.clone();
        let acp_proxy = self.config.acp_proxy.clone();
        let tx = self.app_event_tx.clone();
        tokio::spawn(async move {
            const PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);
            let probe = match tokio::time::timeout(
                PROBE_TIMEOUT,
                nori_harness::probe_agent_sessions_for(&agent, &cwd, acp_proxy),
            )
            .await
            {
                Ok(result) => result,
                Err(_) => Err(nori_harness::ProbeError::Failed(format!(
                    "timed out listing sessions after {}s",
                    PROBE_TIMEOUT.as_secs()
                ))),
            };
            tx.send(AppEvent::AgentSessionListProbed { probe, intent });
        });
    }

    pub(super) fn shutdown_current_conversation(&mut self) {
        self.chat_widget.shutdown_harness_session();
    }

    /// Display a loaded transcript in the history view.
    pub(super) fn display_viewonly_transcript(
        &mut self,
        entries: Vec<crate::viewonly_transcript::ViewonlyEntry>,
    ) {
        use crate::history_cell::AgentMessageCell;
        use crate::markdown::append_markdown;
        use crate::viewonly_transcript::ViewonlyEntry;

        // Add a header
        self.chat_widget.add_info_message(
            "────────── Viewing Previous Session ──────────".to_string(),
            None,
        );

        let mut is_first_entry = true;
        for entry in entries {
            // Add a blank line separator between entries (except before the first)
            if !is_first_entry {
                self.chat_widget
                    .add_plain_history_lines(vec![Line::from("")]);
            }
            is_first_entry = false;

            match entry {
                ViewonlyEntry::User { content } => {
                    // Add user messages with a user prefix to distinguish them
                    self.chat_widget.add_boxed_history(Box::new(
                        crate::history_cell::UserHistoryCell { message: content },
                    ));
                }
                ViewonlyEntry::Assistant { content } => {
                    // Add assistant response with markdown rendering
                    let mut lines = Vec::new();
                    append_markdown(&content, None, &mut lines);
                    let cell = AgentMessageCell::new(lines, true);
                    self.chat_widget.add_boxed_history(Box::new(cell));
                }
                ViewonlyEntry::Thinking { content } => {
                    // Add thinking block with dimmed style (same pattern as reasoning display)
                    let mut lines = Vec::new();
                    append_markdown(&content, None, &mut lines);
                    // Dim all spans in the lines to indicate this is thinking content
                    let dimmed_lines: Vec<Line<'static>> = lines
                        .into_iter()
                        .map(|line| {
                            Line::from(
                                line.spans
                                    .into_iter()
                                    .map(ratatui::prelude::Stylize::dim)
                                    .collect::<Vec<_>>(),
                            )
                        })
                        .collect();
                    let cell = AgentMessageCell::new(dimmed_lines, true);
                    self.chat_widget.add_boxed_history(Box::new(cell));
                }
                ViewonlyEntry::Info { content } => {
                    // Add as an info message
                    self.chat_widget
                        .add_info_message(content, Some("transcript".to_string()));
                }
            }
        }

        self.chat_widget
            .add_info_message("────────── End of Transcript ──────────".to_string(), None);
    }

    pub(super) fn open_external_editor(&mut self, tui: &mut tui::Tui) {
        use crate::editor;

        let current_text = self.chat_widget.composer_text();
        let editor_cmd = editor::resolve_editor();

        let temp_path = match editor::write_temp_file(&current_text) {
            Ok(path) => path,
            Err(err) => {
                self.chat_widget
                    .add_error_message(format!("Failed to create temp file: {err}"));
                return;
            }
        };

        // Restore terminal to normal mode so the editor can take over
        let _ = tui::restore();

        let status = editor::spawn_editor(&editor_cmd, &temp_path);

        // Re-enable TUI mode
        let _ = tui::set_modes();
        tui.frame_requester().schedule_frame();

        match status {
            Ok(exit_status) if exit_status.success() => {
                match editor::read_and_cleanup_temp_file(&temp_path) {
                    Ok(content) => {
                        let trimmed = content.trim_end().to_string();
                        self.chat_widget.set_composer_text(trimmed);
                    }
                    Err(err) => {
                        self.chat_widget
                            .add_error_message(format!("Failed to read editor output: {err}"));
                    }
                }
            }
            Ok(_) => {
                // Editor exited with non-zero status; discard changes, clean up temp file
                let _ = std::fs::remove_file(&temp_path);
            }
            Err(err) => {
                let _ = std::fs::remove_file(&temp_path);
                self.chat_widget
                    .add_error_message(format!("Failed to launch editor '{editor_cmd}': {err}"));
            }
        }
    }

    /// Launch a terminal file manager in chooser mode, then open the selected
    /// file in the user's editor.
    pub(super) fn browse_files(&mut self, fm: nori_config::FileManager, tui: &mut tui::Tui) {
        use crate::editor;

        // Create a temp file for the file manager to write the chosen path into.
        let chooser_output = match tempfile::Builder::new()
            .prefix("nori-browse-")
            .suffix(".txt")
            .tempfile()
        {
            Ok(tmp) => match tmp.keep() {
                Ok((_, path)) => path,
                Err(e) => {
                    self.chat_widget
                        .add_error_message(format!("Failed to create temp file: {}", e.error));
                    return;
                }
            },
            Err(err) => {
                self.chat_widget
                    .add_error_message(format!("Failed to create temp file: {err}"));
                return;
            }
        };

        let chooser_args = fm.chooser_args(&chooser_output);

        // Restore terminal to normal mode so the file manager can take over.
        let _ = tui::restore();

        // Loop: launch file manager → open selected file in editor → re-launch
        // file manager. The user stays in the browse workflow until they exit
        // the file manager without selecting a file (or it fails).
        loop {
            // Clear the chooser output so a stale selection from a previous
            // iteration doesn't persist.
            let _ = std::fs::write(&chooser_output, "");

            let fm_status = std::process::Command::new(fm.command_name())
                .args(&chooser_args)
                .stdin(std::process::Stdio::inherit())
                .stdout(std::process::Stdio::inherit())
                .stderr(std::process::Stdio::inherit())
                .status();

            match fm_status {
                Ok(exit_status) if exit_status.success() => {
                    let chosen = std::fs::read_to_string(&chooser_output)
                        .unwrap_or_default()
                        .lines()
                        .next()
                        .unwrap_or("")
                        .trim()
                        .to_string();

                    if chosen.is_empty() {
                        // User exited file manager without selecting a file.
                        break;
                    }

                    let chosen_path = std::path::Path::new(&chosen);
                    if chosen_path.is_file() {
                        let editor_cmd = editor::resolve_editor();
                        let editor_status = editor::spawn_editor(&editor_cmd, chosen_path);
                        if let Err(err) = editor_status {
                            self.chat_widget.add_error_message(format!(
                                "Failed to launch editor '{editor_cmd}': {err}"
                            ));
                            break;
                        }
                        // After editor exits, loop back to re-launch the file manager.
                    } else {
                        self.chat_widget
                            .add_error_message(format!("Selected path is not a file: {chosen}"));
                        break;
                    }
                }
                Ok(_) => {
                    // File manager exited with non-zero status.
                    break;
                }
                Err(err) => {
                    self.chat_widget.add_error_message(format!(
                        "Failed to launch {}: {err}. Is it installed?",
                        fm.command_name()
                    ));
                    break;
                }
            }
        }

        // Always clean up temp file and restore TUI.
        let _ = std::fs::remove_file(&chooser_output);
        let _ = tui::set_modes();
        tui.frame_requester().schedule_frame();
    }

    #[cfg(target_os = "windows")]
    pub(super) fn spawn_world_writable_scan(
        cwd: PathBuf,
        env_map: std::collections::HashMap<String, String>,
        logs_base_dir: PathBuf,
        sandbox_policy: nori_config::SandboxPolicy,
        tx: AppEventSender,
    ) {
        tokio::task::spawn_blocking(move || {
            let result = codex_windows_sandbox::apply_world_writable_scan_and_denies(
                &logs_base_dir,
                &cwd,
                &env_map,
                &sandbox_policy,
                Some(logs_base_dir.as_path()),
            );
            if result.is_err() {
                // Scan failed: warn without examples.
                tx.send(AppEvent::OpenWorldWritableWarningConfirmation {
                    preset: None,
                    sample_paths: Vec::new(),
                    extra_count: 0usize,
                    failed_scan: true,
                });
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use nori_protocol::acp::v1::SessionInfo;

    use super::onboarding_resume_event;
    use crate::app_event::AppEvent;

    #[test]
    fn tagged_onboarding_session_emits_the_existing_resume_action() {
        let ordinary = SessionInfo::new("ordinary", PathBuf::from("/"));
        let mut onboarding = SessionInfo::new("onboarding", PathBuf::from("/"));
        onboarding.title = Some("Set up Nori".to_string());
        onboarding.meta = Some(
            serde_json::from_value(serde_json::json!({
                "nori": { "purpose": "onboarding" }
            }))
            .expect("valid metadata"),
        );

        assert!(matches!(
            onboarding_resume_event(vec![ordinary, onboarding]),
            Some(AppEvent::ResumeAcpSession { acp_session_id, title })
                if acp_session_id == "onboarding" && title.as_deref() == Some("Set up Nori")
        ));
    }

    #[test]
    fn no_tagged_onboarding_session_leaves_the_fresh_fallback_available() {
        let ordinary = SessionInfo::new("ordinary", PathBuf::from("/"));

        assert!(onboarding_resume_event(vec![ordinary]).is_none());
    }
}
