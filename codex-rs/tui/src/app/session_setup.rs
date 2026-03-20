use super::*;

impl App {
    pub(super) fn shutdown_current_conversation(&mut self) {
        if self.chat_widget.conversation_id().is_some() {
            self.suppress_shutdown_complete = true;
            self.chat_widget.submit_op(Op::Shutdown);
        }
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
    #[cfg(feature = "nori-config")]
    pub(super) fn browse_files(&mut self, fm: codex_acp::config::FileManager, tui: &mut tui::Tui) {
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
        sandbox_policy: codex_core::protocol::SandboxPolicy,
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
