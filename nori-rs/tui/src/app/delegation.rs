use super::session_deck::ParkedSession;
use super::*;

impl App {
    /// Adopt manager-started children without replacing the parent's widget.
    pub(super) fn adopt_delegated_sessions(&mut self, tui: &crate::tui::Tui) {
        let Some(manager) = self.session_manager.clone() else {
            return;
        };
        let known: std::collections::HashSet<_> = self
            .session_facts()
            .into_iter()
            .filter_map(|fact| fact.conversation_id)
            .collect();
        for record in manager.list() {
            if record.source != nori_harness::session_manager::SessionSource::Session
                || known.contains(&record.conversation_id)
                || self.adopted_delegations.contains(&record.conversation_id)
            {
                continue;
            }
            // The manager lists a child before its launch hands over the
            // session handle. Adopting it then would leave the child's view
            // unable to answer its requests, so wait for a later draw; a
            // child that already ended is adopted without one.
            let handle = manager.handle(&record.conversation_id);
            let ended =
                record.status.is_closed() || record.status == nori_protocol::SessionStatus::Failed;
            if handle.is_none() && !ended {
                continue;
            }
            // Follow forks of this process while skipping unrelated histories.
            let mut cursors = manager.replay_cursors();
            let records = manager.list();
            let mut lineage = std::collections::HashSet::from([record.conversation_id]);
            loop {
                let before = lineage.len();
                for descendant in &records {
                    if descendant.source == nori_harness::session_manager::SessionSource::Fork
                        && descendant
                            .parent
                            .is_some_and(|parent| lineage.contains(&parent))
                    {
                        lineage.insert(descendant.conversation_id);
                    }
                }
                if lineage.len() == before {
                    break;
                }
            }
            for id in lineage {
                cursors.insert(id.to_string(), 0);
            }
            let subscription = match manager.subscribe_from(None, &cursors, 4096) {
                Ok(subscription) => subscription,
                Err(error) => {
                    tracing::warn!(%error, "cannot replay delegated session");
                    continue;
                }
            };
            let mut init = self.chat_widget_init(
                tui.frame_requester(),
                None,
                Vec::new(),
                Some(record.agent.clone()),
                true,
                None,
            );
            init.config = self.config_for_agent(&record.agent);
            init.config.cwd = record.cwd;
            let widget =
                ChatWidget::from_managed_view(init, record.conversation_id, handle, subscription);
            self.adopted_delegations.insert(record.conversation_id);
            self.session_deck.park(
                widget.session_generation(),
                ParkedSession {
                    widget: Box::new(widget),
                    transcript: Vec::new(),
                },
            );
        }
    }
}
