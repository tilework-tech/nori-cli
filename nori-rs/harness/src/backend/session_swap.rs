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
