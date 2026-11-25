//! The App on a host it joined: every session the host lists gets a widget
//! fed from the host's replay and live events, and the session on screen is
//! the one the connection subscribes to.
//!
//! No session runs in this process. Leaving closes only this terminal's
//! connection; the host and its sessions keep running.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use codex_login::AuthManager;
use color_eyre::eyre::Result;
use nori_config::NoriConfig;
use nori_harness::ConversationId;
use nori_harness::remote_agent::ManagedClient;
use nori_protocol::SessionStatus;
use serde::Deserialize;
use serde_json::Value;
use tokio::sync::mpsc::unbounded_channel;

use super::App;
use super::AppExitInfo;
use super::host_link::HostLink;
use super::joined_feed::FeedOutput;
use super::joined_feed::JoinedFeed;
use super::session_deck::ParkedSession;
use crate::app_event_sender::AppEventSender;
use crate::app_event_sender::HostReply;
use crate::chatwidget::ChatWidget;
use crate::host_client::HostConnection;
use crate::host_client::HostEndpoint;
use crate::tui;

pub(crate) struct JoinedSessions {
    feed: JoinedFeed,
    listed_once: bool,
    /// The session to show once the host first lists its sessions: a full
    /// ID or a unique prefix.
    open_session: Option<String>,
    /// The host's latest list, for sessions whose events are not followed.
    rows: HashMap<ConversationId, ListedSession>,
}

#[derive(Deserialize)]
struct SessionList {
    #[serde(default)]
    sessions: Vec<ListedSession>,
}

/// One row of `_nori/session/list`.
#[derive(Clone, Deserialize)]
pub(crate) struct ListedSession {
    session_id: String,
    agent: String,
    pub(crate) status: SessionStatus,
    pub(crate) family_status: Option<SessionStatus>,
    cwd: Option<PathBuf>,
    parent: Option<String>,
    /// `fork` when the session continues its parent after a fork.
    #[serde(default)]
    source: Option<String>,
    pub(crate) title: Option<String>,
    pub(crate) reply_preview: Option<String>,
    pub(crate) idle_seconds: Option<i64>,
}

impl ListedSession {
    pub(crate) fn parent(&self) -> Option<ConversationId> {
        ConversationId::from_string(self.parent.as_deref()?).ok()
    }
}

impl App {
    /// Runs the TUI on the running host `initial` is connected to. Leaving
    /// only closes this terminal's connection.
    pub(crate) async fn run_joined(
        tui: &mut tui::Tui,
        auth_manager: Arc<AuthManager>,
        config: NoriConfig,
        endpoint: HostEndpoint,
        initial: ManagedClient,
        open_session: Option<String>,
        analytics: Option<nori_installed::AnalyticsReporter>,
    ) -> Result<AppExitInfo> {
        let mut host = HostLink::join(endpoint, initial);
        let (app_event_tx, app_event_rx) = unbounded_channel();
        let app_event_tx = AppEventSender::new(app_event_tx);
        // Shown until the host lists its sessions; it never starts one.
        let mut placeholder = ChatWidget::new(crate::chatwidget::ChatWidgetInit {
            config: config.clone(),
            frame_requester: tui.frame_requester(),
            app_event_tx: app_event_tx.clone(),
            initial_prompt: None,
            initial_images: Vec::new(),
            enhanced_keys_supported: tui.enhanced_keys_supported(),
            auth_manager: auth_manager.clone(),
            vertical_footer: config.vertical_footer,
            footer_segment_config: config.footer_segment_config.clone(),
            footer_layout_config: config.footer_layout_config.clone(),
            cloud_mode: false,
            deferred_spawn: true,
            fork_context: None,
            prepared_agent: None,
            analytics: analytics.clone(),
            session_manager: None,
            clock: crate::answer_guard::Clock::system(),
        });
        placeholder.suppress_welcome_card();
        placeholder.set_host_connection(host.connection());
        let vertical_footer = config.vertical_footer;
        let mut app = Self::assemble(
            tui,
            app_event_tx,
            placeholder,
            auth_manager,
            config,
            None,
            None,
            vertical_footer,
            false,
            analytics,
        );
        app.joined = Some(JoinedSessions {
            feed: JoinedFeed::default(),
            listed_once: false,
            open_session,
            rows: HashMap::new(),
        });
        let ran = app.drive(tui, app_event_rx, &mut host).await;
        let left = host.stop_or_leave().await;
        let exit = ran?;
        left.map_err(|error| color_eyre::eyre::eyre!(error))?;
        Ok(exit)
    }

    /// What the host last listed about `id`.
    pub(super) fn listed_session(&self, id: ConversationId) -> Option<&ListedSession> {
        self.joined.as_ref()?.rows.get(&id)
    }

    /// Give every newly listed session a parked widget, and on the first
    /// list show the requested session or the session list.
    pub(super) fn on_joined_sessions(
        &mut self,
        list: Value,
        tui: &mut tui::Tui,
        connection: &HostConnection,
    ) {
        let Some(joined) = self.joined.as_mut() else {
            return;
        };
        let list: SessionList = match serde_json::from_value(list) {
            Ok(list) => list,
            Err(error) => {
                tracing::warn!(
                    "skipping a session list the host sent in an unknown shape: {error}"
                );
                return;
            }
        };
        let listed: Vec<(ConversationId, ListedSession)> = list
            .sessions
            .into_iter()
            .filter_map(|row| Some((ConversationId::from_string(&row.session_id).ok()?, row)))
            .collect();
        let first = !std::mem::replace(&mut joined.listed_once, true);
        // A session the host no longer lists has ended there.
        for (id, row) in &mut joined.rows {
            if !listed.iter().any(|(listed_id, _)| listed_id == id) {
                row.status = SessionStatus::Closed;
                row.family_status = Some(SessionStatus::Closed);
            }
        }
        let new: Vec<(ConversationId, ListedSession)> = listed
            .iter()
            .filter(|(id, row)| {
                // A fork continues its parent's widget, which follows it.
                let continues_parent = row.source.as_deref() == Some("fork")
                    && row
                        .parent()
                        .is_some_and(|parent| joined.feed.generation_of(parent).is_some());
                joined.feed.generation_of(*id).is_none() && !continues_parent
            })
            .cloned()
            .collect();
        joined.rows.extend(listed.iter().cloned());

        for (id, row) in new {
            let mut init =
                self.chat_widget_init(tui.frame_requester(), None, Vec::new(), None, true, None);
            if let Some(cwd) = row.cwd {
                init.config.cwd = cwd;
            }
            init.config.active_agent.clone_from(&row.agent);
            init.config.agent = row.agent;
            let mut widget = ChatWidget::from_host_feed(init, id, row.title);
            widget.set_host_connection(connection.clone());
            widget.set_foreground(false);
            widget.seed_system_info();
            widget.update_approval_mode_label();
            widget.set_hotkey_config(self.hotkey_config.clone());
            widget.set_vim_mode(self.vim_mode);
            widget.set_plan_drawer_mode(self.plan_drawer_mode);
            let generation = widget.session_generation();
            if let Some(joined) = self.joined.as_mut() {
                joined.feed.follow(id, generation);
            }
            self.session_deck.park(
                generation,
                ParkedSession {
                    widget: Box::new(widget),
                    transcript: Vec::new(),
                },
            );
        }

        if !first {
            return;
        }
        let requested = self
            .joined
            .as_mut()
            .and_then(|joined| joined.open_session.take());
        let target = requested.as_deref().map(|wanted| {
            let matching: Vec<ConversationId> = listed
                .iter()
                .filter(|(_, row)| row.session_id.starts_with(wanted))
                .map(|(id, _)| *id)
                .collect();
            match matching.as_slice() {
                [id] => Ok(*id),
                [] => Err(format!("No session on this host matches “{wanted}”.")),
                [_, _, ..] => Err(format!(
                    "Several sessions on this host match “{wanted}”; pick one."
                )),
            }
        });
        match target {
            Some(Ok(id)) => {
                if let Some(generation) = self
                    .joined
                    .as_ref()
                    .and_then(|joined| joined.feed.generation_of(id))
                {
                    self.show_session(generation);
                }
            }
            Some(Err(message)) => {
                self.chat_widget.add_error_message(message);
                self.open_session_list();
            }
            None if listed.is_empty() => self
                .chat_widget
                .add_info_message("This host has no sessions.".to_string(), None),
            None => self.open_session_list(),
        }
    }

    /// One `_nori/session/event` notification, shown in its session's widget.
    pub(super) fn on_joined_session_event(&mut self, params: Value) {
        let Some(joined) = self.joined.as_mut() else {
            return;
        };
        for output in joined.feed.apply(params) {
            match output {
                FeedOutput::Event { generation, event } => {
                    self.session_previews.observe(generation, &event);
                    self.deliver_session_event(generation, event);
                }
                FeedOutput::Gap {
                    generation,
                    message,
                } => {
                    if generation == self.chat_widget.session_generation() {
                        self.chat_widget.add_error_message(message);
                    } else if let Some(parked) = self.session_deck.parked_mut(generation) {
                        parked.widget.add_error_message(message);
                    }
                }
            }
        }
    }

    pub(super) fn joined_connection_lost(&mut self) {
        if let Some(joined) = self.joined.as_mut() {
            joined.feed.connection_lost();
        }
    }

    /// Subscribe the connection to the session on screen when it is not
    /// already, replaying what this terminal has not shown.
    pub(super) fn follow_foreground_session(&mut self, host: &mut HostLink) {
        let connection = host.connection();
        if !connection.connected || connection.stopped.is_some() {
            return;
        }
        let Some(id) = self.chat_widget.managed_conversation_id() else {
            return;
        };
        let Some(call) = self
            .joined
            .as_mut()
            .and_then(|joined| joined.feed.subscribe(id))
        else {
            return;
        };
        host.request(call, HostReply::ReportError(self.app_event_tx.clone()));
    }
}
