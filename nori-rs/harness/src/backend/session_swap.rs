//! Active-session swapping shared by summarize-and-swap `/compact` and
//! branch-at-head `/fork`.
//!
//! Both operations replace the active ACP session with a fresh one obtained
//! from the agent (a brand-new session after compaction, or a fork of the
//! current head), re-registering the backend-owned MCP servers and
//! rebroadcasting capabilities for the new session.

use super::*;

use crate::normalized::ClientEvent;

/// How [`AcpBackend::swap_active_session`] obtains the replacement session id.
pub(super) enum SessionSwapMode {
    /// Create a brand-new session (summarize-and-swap `/compact`).
    NewAfterCompact,
    /// Fork the current head into a new session (branch-at-head `/fork`).
    ForkFromHead { from: acp::SessionId },
}

impl AcpBackend {
    /// Replace the active ACP session with a new one, assembling MCP servers,
    /// re-registering the backend-owned `nori-client` server, committing it,
    /// swapping the active session id, and rebroadcasting capabilities.
    ///
    /// On failure the goal-MCP connected flag is rolled back and the error is
    /// returned to the caller.
    pub(super) async fn swap_active_session(
        &self,
        mode: SessionSwapMode,
    ) -> Result<acp::SessionId> {
        let cwd = self.cwd.clone();
        let mut mcp_servers = crate::connection::mcp::to_acp_mcp_servers(
            &self.mcp_servers,
            self.mcp_oauth_credentials_store_mode,
        );
        let previous_goal_mcp_connected = {
            let goal_mcp_http_server = self.goal_mcp_http_server.lock().await;
            if let Some(server) = goal_mcp_http_server.as_ref() {
                mcp_servers.push(server.as_mcp_server());
                Some(self.goal_mcp_connected.swap(false, Ordering::Relaxed))
            } else {
                None
            }
        };
        let pending_nori_client_server = if previous_goal_mcp_connected.is_some() {
            None
        } else {
            match nori_client_mcp::register_for_session(
                &self.connection,
                &mut mcp_servers,
                Arc::clone(&self.thread_goal_state),
                self.backend_event_tx.clone(),
                Arc::clone(&self.goal_mcp_connected),
            )
            .await
            {
                Ok(server) => server,
                Err(err) => {
                    warn!("Failed to register goal MCP server during session swap: {err}");
                    None
                }
            }
        };
        let nori_client_advertised = mcp_servers.iter().any(|server| {
            matches!(
                server,
                acp::McpServer::Http(http) if http.name == "nori-client"
            )
        });

        let swap_result = match &mode {
            SessionSwapMode::NewAfterCompact => {
                self.connection.create_session(&cwd, mcp_servers).await
            }
            SessionSwapMode::ForkFromHead { from } => {
                self.connection
                    .fork_session(&from.to_string(), &cwd, mcp_servers)
                    .await
            }
        };

        match swap_result {
            Ok(new_session_id) => {
                if let Some(server) = pending_nori_client_server {
                    server.commit(&self.goal_mcp_http_server).await;
                }
                debug!("Swapped active ACP session: {:?}", new_session_id);
                *self.session_id.write().await = new_session_id.clone();
                if matches!(mode, SessionSwapMode::ForkFromHead { .. }) {
                    self.fork_transcript(&new_session_id).await;
                }
                self.forward_client_event(ClientEvent::SessionCapabilitiesChanged(
                    nori_client_mcp::capabilities_update_for_nori_client(
                        &self.connection,
                        nori_client_advertised,
                        self.goal_mcp_connected.load(Ordering::Relaxed),
                    ),
                ))
                .await;
                Ok(new_session_id)
            }
            Err(err) => {
                if let Some(previous) = previous_goal_mcp_connected {
                    self.goal_mcp_connected.store(previous, Ordering::Relaxed);
                }
                Err(err)
            }
        }
    }

    /// Fork the active transcript into a fresh conversation seeded from the
    /// parent, freezing the parent on disk and swapping the active recorder +
    /// conversation id to the child.
    ///
    /// Best-effort: transcript persistence is non-fatal, so any failure is
    /// logged and the ACP session swap still stands. Emits the public
    /// `SessionForked` event after the swap so the event and all subsequent
    /// entries record into the child transcript.
    async fn fork_transcript(&self, new_acp_session_id: &acp::SessionId) {
        let Some(parent_recorder) = self.transcript_recorder.read().await.clone() else {
            return;
        };
        let previous_conversation_id = parent_recorder.session_id().to_string();

        // Freeze the parent before seeding from its file on disk.
        if let Err(error) = parent_recorder.flush().await {
            warn!("Failed to flush parent transcript before fork: {error}");
            return;
        }
        let seed_entries =
            match crate::transcript::read_seed_entries(parent_recorder.transcript_path()).await {
                Ok(entries) => entries,
                Err(error) => {
                    warn!("Failed to read parent transcript for fork: {error}");
                    return;
                }
            };

        let forked = match TranscriptRecorder::new_forked(
            &self.nori_home,
            &self.cwd,
            Some(self.agent_name.clone()),
            &self.cli_version,
            new_acp_session_id.to_string(),
            previous_conversation_id.clone(),
            seed_entries,
        )
        .await
        {
            Ok(recorder) => recorder,
            Err(error) => {
                warn!("Failed to create forked transcript: {error}");
                return;
            }
        };
        let new_conversation_id = forked.session_id().to_string();

        // Swap the active recorder + conversation id BEFORE emitting the event
        // so the event and every later entry record into the child.
        *self.transcript_recorder.write().await = Some(Arc::new(forked));
        if let Ok(conversation_id) = ConversationId::from_string(&new_conversation_id) {
            *self.conversation_id.write().await = conversation_id;
        }

        self.backend_event_tx
            .send(BackendEvent::Public(SessionEvent::Nori(
                nori_protocol::NoriEvent::SessionForked(nori_protocol::SessionForked {
                    previous_conversation_id,
                    new_conversation_id,
                    new_acp_session_id: new_acp_session_id.clone(),
                }),
            )))
            .await
            .ok();
    }

    /// Branch the conversation at its current head via ACP `session/fork`,
    /// swapping the active session to the forked id.
    ///
    /// Gated on the agent advertising the fork capability and on the session
    /// being idle.
    pub(crate) async fn branch(&self) -> Result<()> {
        if self
            .connection
            .capabilities()
            .session_capabilities
            .fork
            .is_none()
        {
            anyhow::bail!("This agent does not support branching");
        }

        let is_idle = matches!(
            self.session_driver.lock().await.public_phase(),
            nori_protocol::SessionPhase::Idle
        );
        if !is_idle {
            anyhow::bail!("Cannot branch during an active turn");
        }

        let from = self.session_id.read().await.clone();
        self.swap_active_session(SessionSwapMode::ForkFromHead { from })
            .await?;
        Ok(())
    }
}
