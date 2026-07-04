//! Replacing the live ACP session while keeping the same agent connection.
//!
//! Two flows swap the runtime's session id: client-side compaction (summarize,
//! then continue in a fresh `session/new`) and `/fork`'s branch-from-current-
//! point (`session/fork`). The MCP re-registration and capability bookkeeping
//! around that swap is identical for both, so it lives here.

use super::*;

use nori_protocol::ClientEvent;

pub(super) enum SessionReplacement {
    /// `session/new` — used after client-side compaction.
    NewSession,
    /// `session/fork` from the given session — used by branching.
    Fork { from: acp::SessionId },
}

impl AcpBackend {
    /// Create the replacement session (new or forked), re-register the
    /// backend-owned `nori-client` MCP server, swap the live session id, and
    /// broadcast the refreshed session capabilities.
    ///
    /// On failure the previous session stays active and goal-MCP state is
    /// restored.
    pub(super) async fn replace_session(
        &self,
        replacement: SessionReplacement,
    ) -> anyhow::Result<acp::SessionId> {
        let cwd = self.cwd.clone();
        let mut mcp_servers = crate::connection::mcp::to_acp_mcp_servers(
            &self.mcp_servers,
            self.mcp_oauth_credentials_store_mode,
        );
        let previous_goal_mcp_connected = {
            let goal_mcp_http_server = self.goal_mcp_http_server.lock().await;
            if let Some(server) = goal_mcp_http_server.as_ref() {
                mcp_servers.push(server.as_mcp_server());
                Some(
                    self.goal_mcp_connected
                        .swap(false, std::sync::atomic::Ordering::Relaxed),
                )
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
                Arc::clone(&self.transcript_recorder_cell),
                Arc::clone(&self.goal_mcp_connected),
            )
            .await
            {
                Ok(server) => server,
                Err(err) => {
                    warn!("Failed to register goal MCP server for replacement session: {err}");
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

        let result = match &replacement {
            SessionReplacement::NewSession => {
                self.connection.create_session(&cwd, mcp_servers).await
            }
            SessionReplacement::Fork { from } => {
                self.connection.fork_session(from, &cwd, mcp_servers).await
            }
        };

        match result {
            Ok(new_session_id) => {
                if let Some(server) = pending_nori_client_server {
                    server.commit(&self.goal_mcp_http_server).await;
                }
                *self.session_id.write().await = new_session_id.clone();
                self.forward_and_record_client_event(ClientEvent::SessionCapabilitiesChanged(
                    nori_client_mcp::capabilities_update_for_nori_client(
                        &self.connection,
                        nori_client_advertised,
                        self.goal_mcp_connected
                            .load(std::sync::atomic::Ordering::Relaxed),
                    ),
                ))
                .await;
                Ok(new_session_id)
            }
            Err(err) => {
                if let Some(previous) = previous_goal_mcp_connected {
                    self.goal_mcp_connected
                        .store(previous, std::sync::atomic::Ordering::Relaxed);
                }
                Err(err)
            }
        }
    }

    /// Handle `Op::BranchSession`: fork the current session at its current
    /// state via ACP `session/fork` and switch the runtime to the fork. The
    /// original session is preserved on the agent side and stays resumable.
    pub(super) async fn handle_branch_session(&self) {
        if !self.connection.supports_fork() {
            self.send_error("This agent does not support branching sessions.")
                .await;
            return;
        }
        if !self.session_driver.lock().await.is_idle() {
            self.send_error("Cannot branch while a turn is in progress.")
                .await;
            return;
        }
        let from = self.session_id.read().await.clone();
        // `fork_session` applies the fork response's config options before it
        // returns, so the config snapshot is current when SessionBranched is
        // emitted (clients may re-read config on that event).
        match self
            .replace_session(SessionReplacement::Fork { from })
            .await
        {
            Ok(new_session_id) => {
                debug!("Branched to forked session: {new_session_id:?}");
                self.forward_and_record_client_event(ClientEvent::SessionBranched(
                    nori_protocol::SessionBranched {
                        new_session_id: new_session_id.to_string(),
                    },
                ))
                .await;
            }
            Err(err) => {
                self.send_error(&format!("Branching failed: {err:#}")).await;
            }
        }
    }
}
