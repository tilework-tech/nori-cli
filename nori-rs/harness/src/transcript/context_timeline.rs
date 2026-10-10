use crate::context_timeline::ContextTimeline;
use crate::context_timeline::ContextTimelineState;
use crate::context_timeline::is_timeline_evidence;

use super::SessionEventEntry;
use super::Transcript;
use super::TranscriptEntry;

impl Transcript {
    /// Reconstruct context observations from their original recorded boundaries.
    pub fn context_timeline(&self) -> ContextTimeline {
        self.context_timeline_state().timeline
    }

    pub(crate) fn context_timeline_state(&self) -> ContextTimelineState {
        let mut state = ContextTimelineState::with_session(self.meta.acp_session_id.clone());
        let mut legacy_messages = false;
        for (index, line) in self.entries.iter().enumerate() {
            match &line.entry {
                TranscriptEntry::SessionEvent(entry) => {
                    state.timeline.incomplete_history |= entry.history_incomplete;
                    let fallback_id = format!("{}:{index}", self.meta.session_id);
                    state.observe(
                        &entry.event,
                        entry.observation_id.as_deref().unwrap_or(&fallback_id),
                        entry.observed_at.as_deref().unwrap_or(&line.ts),
                    );
                }
                TranscriptEntry::User(_)
                | TranscriptEntry::Assistant(_)
                | TranscriptEntry::ClientEvent(_) => legacy_messages = true,
                _ => {}
            }
        }
        if legacy_messages && state.timeline.turns.is_empty() {
            state.timeline.incomplete_history = true;
        }
        state
    }

    /// Raw timeline evidence survives resume without becoming conversation replay.
    pub(crate) fn archived_context_evidence(&self) -> Vec<SessionEventEntry> {
        let mut replaying = false;
        self.entries
            .iter()
            .enumerate()
            .filter_map(|(index, line)| {
                let TranscriptEntry::SessionEvent(entry) = &line.entry else {
                    return None;
                };
                match &entry.event {
                    nori_protocol::SessionEvent::Nori(nori_protocol::NoriEvent::ReplayStarted(
                        _,
                    )) => {
                        replaying = true;
                        return None;
                    }
                    nori_protocol::SessionEvent::Nori(nori_protocol::NoriEvent::ReplayFinished) => {
                        replaying = false;
                        return None;
                    }
                    _ if replaying => return None,
                    _ => {}
                }
                if !is_timeline_evidence(&entry.event) {
                    return None;
                }
                let mut entry = entry.clone();
                entry
                    .observation_id
                    .get_or_insert_with(|| format!("{}:{index}", self.meta.session_id));
                entry.observed_at.get_or_insert_with(|| line.ts.clone());
                entry.historical_evidence = true;
                Some(entry)
            })
            .collect()
    }
}
