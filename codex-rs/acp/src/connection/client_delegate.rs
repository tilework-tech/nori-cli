use super::*;

impl ClientDelegate {
    pub(super) fn new(cwd: PathBuf, approval_tx: mpsc::Sender<ApprovalRequest>) -> Self {
        Self {
            sessions: RefCell::new(HashMap::new()),
            persistent_tx: RefCell::new(None),
            cwd,
            approval_tx,
        }
    }

    pub(super) fn register_session(
        &self,
        session_id: acp::SessionId,
        tx: mpsc::Sender<acp::SessionUpdate>,
    ) {
        self.sessions.borrow_mut().insert(session_id, tx);
    }

    pub(super) fn unregister_session(&self, session_id: &acp::SessionId) {
        self.sessions.borrow_mut().remove(session_id);
    }

    /// Set a persistent fallback listener for inter-turn notifications.
    pub fn set_persistent_listener(&self, tx: mpsc::Sender<acp::SessionUpdate>) {
        *self.persistent_tx.borrow_mut() = Some(tx);
    }
}

#[async_trait::async_trait(?Send)]
impl acp::Client for ClientDelegate {
    async fn request_permission(
        &self,
        arguments: acp::RequestPermissionRequest,
    ) -> acp::Result<acp::RequestPermissionResponse> {
        // Translate ACP permission request to Codex approval event.
        // Use patch approval for Edit/Write/Delete operations for better TUI rendering.
        let event = if let Some(patch_event) =
            translator::permission_request_to_patch_approval_event(&arguments)
        {
            ApprovalEventType::Patch(patch_event)
        } else {
            let exec_event =
                translator::permission_request_to_approval_event(&arguments, &self.cwd);
            ApprovalEventType::Exec(exec_event)
        };

        // Create a response channel for the UI to send the decision
        let (response_tx, response_rx) = oneshot::channel();

        // Send the approval request to the UI layer
        let approval_request = ApprovalRequest {
            event,
            options: arguments.options.clone(),
            response_tx,
        };

        if self.approval_tx.send(approval_request).await.is_err() {
            // If the receiver is dropped (UI not listening), fall back to auto-approve
            warn!("Approval channel closed, auto-approving permission request");
            let option_id = arguments
                .options
                .first()
                .map(|opt| opt.option_id.clone())
                .unwrap_or_else(|| acp::PermissionOptionId::from("allow".to_string()));

            return Ok(acp::RequestPermissionResponse::new(
                acp::RequestPermissionOutcome::Selected(acp::SelectedPermissionOutcome::new(
                    option_id,
                )),
            ));
        }

        // Wait for the UI's decision
        match response_rx.await {
            Ok(decision) => {
                // Translate the Codex ReviewDecision back to ACP outcome
                let outcome =
                    translator::review_decision_to_permission_outcome(decision, &arguments.options);
                Ok(acp::RequestPermissionResponse::new(outcome))
            }
            Err(_) => {
                // Response channel was dropped (UI didn't respond), fall back to deny
                warn!("Approval response channel dropped, denying permission request");
                let option_id = arguments
                    .options
                    .iter()
                    .find(|opt| {
                        matches!(
                            opt.kind,
                            acp::PermissionOptionKind::RejectOnce
                                | acp::PermissionOptionKind::RejectAlways
                        )
                    })
                    .map(|opt| opt.option_id.clone())
                    .unwrap_or_else(|| acp::PermissionOptionId::from("deny".to_string()));

                Ok(acp::RequestPermissionResponse::new(
                    acp::RequestPermissionOutcome::Selected(acp::SelectedPermissionOutcome::new(
                        option_id,
                    )),
                ))
            }
        }
    }

    async fn write_text_file(
        &self,
        arguments: acp::WriteTextFileRequest,
    ) -> acp::Result<acp::WriteTextFileResponse> {
        // Emit synthetic ToolCall event for TUI rendering (Gemini compatibility)
        // Gemini agents use client capability methods instead of session/update notifications,
        // so we synthesize the events here to enable proper TUI display.
        let tool_call_id =
            acp::ToolCallId::from(format!("write_text_file-{}", arguments.path.display()));
        let title = format!("Writing {}", arguments.path.display());

        let tool_call = acp::ToolCall::new(tool_call_id, title)
            .kind(acp::ToolKind::Execute)
            .status(acp::ToolCallStatus::Pending);

        // Send the ToolCall update to the session if registered
        let sessions = self.sessions.borrow();
        if let Some(tx) = sessions.get(&arguments.session_id) {
            let _ = tx.try_send(acp::SessionUpdate::ToolCall(tool_call));
        }
        drop(sessions); // Release borrow before performing I/O

        let path = &arguments.path;

        // Resolve relative paths against the working directory
        let resolved_path = if path.is_relative() {
            self.cwd.join(path)
        } else {
            path.to_path_buf()
        };

        // TEMPORARY PATH RESTRICTION:
        // This application-level path check provides basic safety until the ACP agent
        // subprocess is launched with OS-level sandboxing (Seatbelt on macOS, Landlock
        // on Linux, restricted tokens on Windows) as implemented in codex-core's
        // sandboxing module. Once subprocess sandboxing is in place, these checks
        // should be removed as the OS will enforce write restrictions more robustly.
        //
        // For now, restrict writes to:
        // 1. Within the working directory (typical workspace operations)
        // 2. Within /tmp (temporary files, common for agent workflows)
        let allowed = if let Ok(canonical) = resolved_path.canonicalize() {
            let in_cwd = self
                .cwd
                .canonicalize()
                .map(|cwd| canonical.starts_with(&cwd))
                .unwrap_or(false);
            let in_tmp = canonical.starts_with("/tmp");
            in_cwd || in_tmp
        } else {
            // Path doesn't exist yet - check if parent is within allowed directories
            // This handles the case of creating new files
            if let Some(parent) = resolved_path.parent() {
                if let Ok(canonical_parent) = parent.canonicalize() {
                    let in_cwd = self
                        .cwd
                        .canonicalize()
                        .map(|cwd| canonical_parent.starts_with(&cwd))
                        .unwrap_or(false);
                    let in_tmp = canonical_parent.starts_with("/tmp");
                    in_cwd || in_tmp
                } else {
                    // Parent also doesn't exist - only allow if resolved path starts with cwd or /tmp
                    resolved_path.starts_with(&self.cwd) || resolved_path.starts_with("/tmp")
                }
            } else {
                false
            }
        };

        if !allowed {
            return Err(acp::Error::invalid_params().data(format!(
                "Write restricted to working directory ({}) or /tmp. Path: {}",
                self.cwd.display(),
                resolved_path.display()
            )));
        }
        // END TEMPORARY PATH RESTRICTION

        // Create parent directories if they don't exist
        if let Some(parent) = resolved_path.parent()
            && !parent.exists()
        {
            std::fs::create_dir_all(parent).map_err(acp::Error::into_internal_error)?;
        }

        std::fs::write(&resolved_path, &arguments.content)
            .map_err(acp::Error::into_internal_error)?;
        Ok(acp::WriteTextFileResponse::new())
    }

    async fn read_text_file(
        &self,
        arguments: acp::ReadTextFileRequest,
    ) -> acp::Result<acp::ReadTextFileResponse> {
        // Emit synthetic ToolCall event for TUI rendering (Gemini compatibility)
        // Gemini agents use client capability methods instead of session/update notifications,
        // so we synthesize the events here to enable proper TUI display.
        let tool_call_id =
            acp::ToolCallId::from(format!("read_text_file-{}", arguments.path.display()));
        let title = format!("Reading {}", arguments.path.display());

        let tool_call = acp::ToolCall::new(tool_call_id, title)
            .kind(acp::ToolKind::Execute)
            .status(acp::ToolCallStatus::Pending);

        // Send the ToolCall update to the session if registered
        let sessions = self.sessions.borrow();
        if let Some(tx) = sessions.get(&arguments.session_id) {
            let _ = tx.try_send(acp::SessionUpdate::ToolCall(tool_call));
        }
        drop(sessions); // Release borrow before performing I/O

        // Read file content
        let content =
            std::fs::read_to_string(&arguments.path).map_err(acp::Error::into_internal_error)?;
        Ok(acp::ReadTextFileResponse::new(content))
    }

    async fn session_notification(
        &self,
        notification: acp::SessionNotification,
    ) -> acp::Result<()> {
        let sessions = self.sessions.borrow();
        if let Some(tx) = sessions.get(&notification.session_id) {
            // Non-blocking send - if channel is full or closed, we log and drop the update
            if let Err(e) = tx.try_send(notification.update) {
                debug!(
                    target: "acp_message_draining",
                    session_id = %notification.session_id,
                    error = %e,
                    "Session notification dropped (channel full or closed)"
                );
            }
        } else {
            // Session is not registered (inter-turn gap). Forward to persistent
            // listener if one exists, otherwise log and drop.
            drop(sessions);
            let persistent = self.persistent_tx.borrow();
            if let Some(tx) = persistent.as_ref() {
                if let Err(e) = tx.try_send(notification.update) {
                    debug!(
                        target: "acp_message_draining",
                        session_id = %notification.session_id,
                        error = %e,
                        "Persistent listener notification dropped (channel full or closed)"
                    );
                }
            } else {
                debug!(
                    target: "acp_message_draining",
                    session_id = %notification.session_id,
                    "Notification for unregistered session (no persistent listener)"
                );
            }
        }
        Ok(())
    }

    async fn create_terminal(
        &self,
        _args: acp::CreateTerminalRequest,
    ) -> acp::Result<acp::CreateTerminalResponse> {
        Err(acp::Error::method_not_found())
    }

    async fn kill_terminal_command(
        &self,
        _args: acp::KillTerminalCommandRequest,
    ) -> acp::Result<acp::KillTerminalCommandResponse> {
        Err(acp::Error::method_not_found())
    }

    async fn ext_method(&self, _args: acp::ExtRequest) -> acp::Result<acp::ExtResponse> {
        Err(acp::Error::method_not_found())
    }

    async fn ext_notification(&self, _args: acp::ExtNotification) -> acp::Result<()> {
        Ok(())
    }

    async fn release_terminal(
        &self,
        _args: acp::ReleaseTerminalRequest,
    ) -> acp::Result<acp::ReleaseTerminalResponse> {
        Err(acp::Error::method_not_found())
    }

    async fn terminal_output(
        &self,
        _args: acp::TerminalOutputRequest,
    ) -> acp::Result<acp::TerminalOutputResponse> {
        Err(acp::Error::method_not_found())
    }

    async fn wait_for_terminal_exit(
        &self,
        _args: acp::WaitForTerminalExitRequest,
    ) -> acp::Result<acp::WaitForTerminalExitResponse> {
        Err(acp::Error::method_not_found())
    }
}
