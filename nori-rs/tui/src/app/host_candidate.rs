//! `/agent` on the host this terminal owns. The host prepares the chosen ACP
//! agent server for this connection, lists that server's saved sessions, and
//! starts the picked row as a host conversation. The current session stays in
//! front until the host answers with the new conversation's live ID; the
//! widget then follows that conversation through the host's journal, which
//! still holds any history the server replayed before the answer.

use std::collections::HashMap;
use std::path::PathBuf;

use super::*;
use crate::host_client::HostCall;

/// Live events the widget may fall behind by before its subscription lags.
const ACTIVATION_REPLAY_CAPACITY: i32 = 4096;

/// A prepared agent server on the host, owned by this terminal's connection
/// until activated or discarded.
#[derive(Debug)]
pub(crate) struct HostSelection {
    id: String,
    cwd: PathBuf,
}

/// The saved session a switch restores, with the title its row showed.
#[derive(Debug)]
pub(crate) struct SavedSession {
    pub(crate) id: String,
    pub(crate) title: Option<String>,
}

/// A prepared server with its saved sessions, as the host listed them.
#[derive(Debug)]
pub(crate) struct PreparedCatalog {
    selection: HostSelection,
    sessions: Vec<nori_protocol::acp::v1::SessionInfo>,
}

fn host_message(error: anyhow::Error) -> String {
    format!("{error:#}")
}

impl App {
    /// Start preparing a switch candidate on the host. Replacing the candidate
    /// releases any older one without touching the active session.
    pub(crate) fn begin_agent_candidate(&mut self, agent_name: String, display_name: String) {
        if self.chat_widget.has_harness_session() && self.refuse_at_session_cap() {
            return;
        }
        self.discard_candidate();
        self.chat_widget
            .set_login_agent_override(Some(agent_name.clone()));
        let generation = crate::chatwidget::agent::next_session_generation();
        let cwd = self.config_for_agent(&agent_name).cwd;
        let tx = self.app_event_tx.clone();
        let prepared_agent_name = agent_name.clone();
        tokio::spawn(async move {
            let result = prepare_and_list(&tx, agent_name.clone(), cwd).await;
            tx.send(AppEvent::HostCandidatePrepared {
                generation,
                agent_name,
                display_name,
                result,
            });
        });
        self.candidate_agent = Some(CandidateAgent::Preparing {
            generation,
            agent_name: prepared_agent_name,
        });
    }

    pub(super) fn on_host_candidate_prepared(
        &mut self,
        generation: crate::app_event::SessionGeneration,
        agent_name: String,
        display_name: String,
        result: Result<PreparedCatalog, String>,
    ) {
        let is_current = matches!(
            &self.candidate_agent,
            Some(CandidateAgent::Preparing {
                generation: current,
                agent_name: current_agent,
                ..
            }) if *current == generation && *current_agent == agent_name
        );
        if !is_current {
            if let Ok(PreparedCatalog { selection, .. }) = result {
                self.discard_selection(selection);
            }
            return;
        }
        self.candidate_agent = None;
        match result {
            Ok(PreparedCatalog {
                selection,
                sessions,
            }) => {
                self.candidate_agent = Some(CandidateAgent::Prepared {
                    agent_name,
                    display_name,
                    selection,
                });
                self.chat_widget
                    .show_acp_resume_session_picker(sessions, true);
            }
            Err(error) => {
                self.chat_widget.set_login_agent_override(Some(agent_name));
                self.chat_widget
                    .add_error_message(format!("Couldn't prepare {display_name}: {error}"));
            }
        }
    }

    /// Ask the host to start the prepared server's new or saved session. The
    /// candidate waits for the live ID; a reply after the person moved on
    /// closes the conversation it started.
    pub(super) fn activate_host_candidate(&mut self, saved: Option<SavedSession>) {
        let Some(CandidateAgent::Prepared {
            agent_name,
            display_name,
            selection,
        }) = self.candidate_agent.take()
        else {
            return;
        };
        let generation = crate::chatwidget::agent::next_session_generation();
        let tx = self.app_event_tx.clone();
        let reply = tx.host_receipt(HostCall::ActivatePrepared {
            selection: selection.id.clone(),
            cwd: selection.cwd.clone(),
            saved_session_id: saved.as_ref().map(|saved| saved.id.clone()),
        });
        tokio::spawn(async move {
            let result = match reply.await {
                Ok(reply) => reply["sessionId"]
                    .as_str()
                    .and_then(|id| nori_harness::ConversationId::from_string(id).ok())
                    .ok_or_else(|| "the host did not name the new session".to_string()),
                Err(error) => {
                    // The host may have refused before consuming the
                    // selection; a consumed one answers not-found.
                    drop(tx.host_receipt(HostCall::DiscardPrepared {
                        selection: selection.id,
                    }));
                    Err(host_message(error))
                }
            };
            tx.send(AppEvent::HostCandidateActivated { generation, result });
        });
        self.candidate_agent = Some(CandidateAgent::Requested {
            generation,
            agent_name,
            display_name,
            saved,
        });
    }

    pub(super) fn on_host_candidate_activated(
        &mut self,
        frame_requester: crate::tui::FrameRequester,
        generation: crate::app_event::SessionGeneration,
        result: Result<nori_harness::ConversationId, String>,
    ) {
        let is_current = matches!(
            &self.candidate_agent,
            Some(CandidateAgent::Requested { generation: current, .. }) if *current == generation
        );
        if !is_current {
            if let Ok(id) = result {
                drop(self.app_event_tx.host_receipt(HostCall::Close {
                    session_id: id.to_string(),
                }));
            }
            return;
        }
        let Some(CandidateAgent::Requested {
            agent_name,
            display_name,
            saved,
            ..
        }) = self.candidate_agent.take()
        else {
            return;
        };
        let id = match result {
            Ok(id) => id,
            Err(error) => {
                self.chat_widget.set_login_agent_override(Some(agent_name));
                self.chat_widget
                    .add_error_message(format!("Couldn't activate {display_name}: {error}"));
                return;
            }
        };
        let subscription = match self.session_manager.subscribe_from(
            Some(id),
            &HashMap::new(),
            ACTIVATION_REPLAY_CAPACITY,
        ) {
            Ok(subscription) => subscription,
            Err(error) => {
                drop(self.app_event_tx.host_receipt(HostCall::Close {
                    session_id: id.to_string(),
                }));
                self.chat_widget.set_login_agent_override(Some(agent_name));
                self.chat_widget
                    .add_error_message(format!("Couldn't show {display_name}: {error}"));
                return;
            }
        };
        let config = self.config_for_agent(&agent_name);
        let mut init = self.chat_widget_init(
            frame_requester,
            None,
            Vec::new(),
            Some(config.active_agent.clone()),
            true,
            None,
        );
        init.config = config;
        let resumes = saved.is_some();
        let widget = ChatWidget::from_host_activation(
            init,
            id,
            self.session_manager.handle(&id),
            subscription,
            saved.map(|saved| (saved.id, saved.title)),
        );
        self.candidate_agent = Some(CandidateAgent::Activating {
            agent_name,
            display_name,
            widget: Box::new(widget),
            activation_error: None,
            resumes,
        });
    }

    pub(super) fn discard_selection(&self, selection: HostSelection) {
        drop(self.app_event_tx.host_receipt(HostCall::DiscardPrepared {
            selection: selection.id,
        }));
    }
}

async fn prepare_and_list(
    tx: &AppEventSender,
    agent: String,
    cwd: PathBuf,
) -> Result<PreparedCatalog, String> {
    let prepared = tx
        .host_receipt(HostCall::PrepareAgent { agent, cwd })
        .await
        .map_err(host_message)?;
    let nori_protocol::PrepareAgentResult { id, cwd, .. } =
        serde_json::from_value(prepared).map_err(|error| error.to_string())?;
    let listed = tx
        .host_receipt(HostCall::ListPrepared {
            selection: id.clone(),
            cwd: cwd.clone(),
        })
        .await;
    let sessions = listed.map_err(host_message).and_then(|catalog| {
        serde_json::from_value(catalog["sessions"].clone()).map_err(|error| error.to_string())
    });
    match sessions {
        Ok(sessions) => Ok(PreparedCatalog {
            selection: HostSelection { id, cwd },
            sessions,
        }),
        Err(error) => {
            drop(tx.host_receipt(HostCall::DiscardPrepared { selection: id }));
            Err(error)
        }
    }
}
