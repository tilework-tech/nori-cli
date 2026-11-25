//! Session events from a joined host, in order and once each.
//!
//! The host numbers each conversation's events from 1 with no gaps and
//! replays everything after the cursor a subscription names. A connection
//! holds one subscription, so the feed follows the session on screen and
//! keeps the cursor of every session it shows: returning to a session, or
//! reconnecting, replays exactly what this terminal has not applied. An
//! event at or before the cursor was already applied and is dropped; one
//! past the next number means events were lost, and that conversation
//! applies nothing more until it is subscribed again from its cursor.

use std::collections::HashMap;

use nori_harness::ConversationId;
use nori_protocol::NoriEvent;
use nori_protocol::SessionEnvelope;
use nori_protocol::SessionEvent;
use serde_json::Value;

use crate::app_event::SessionGeneration;
use crate::host_client::HostCall;

#[allow(clippy::large_enum_variant)]
pub(crate) enum FeedOutput {
    Event {
        generation: SessionGeneration,
        event: SessionEvent,
    },
    /// Events are missing before the next one; shown once per gap.
    Gap {
        generation: SessionGeneration,
        message: String,
    },
}

struct Followed {
    generation: SessionGeneration,
    /// The last event applied.
    cursor: i64,
    /// Events after the cursor were lost; wait for a new subscription.
    gap: bool,
    /// Subscribed again after a gap: events from the old subscription may
    /// still arrive, so later ones wait silently for the replay.
    replaying: bool,
}

#[derive(Default)]
pub(crate) struct JoinedFeed {
    conversations: HashMap<ConversationId, Followed>,
    subscribed: Option<ConversationId>,
}

impl JoinedFeed {
    /// Show `id`'s events in the widget `generation`, from its first event.
    pub fn follow(&mut self, id: ConversationId, generation: SessionGeneration) {
        self.conversations.entry(id).or_insert(Followed {
            generation,
            cursor: 0,
            gap: false,
            replaying: false,
        });
    }

    pub fn generation_of(&self, id: ConversationId) -> Option<SessionGeneration> {
        self.conversations
            .get(&id)
            .map(|followed| followed.generation)
    }

    /// The call that makes `id` the subscribed session, or `None` when it
    /// already is and nothing was lost.
    pub fn subscribe(&mut self, id: ConversationId) -> Option<HostCall> {
        let followed = self.conversations.get_mut(&id)?;
        if self.subscribed == Some(id) && !followed.gap {
            return None;
        }
        followed.replaying = std::mem::take(&mut followed.gap);
        self.subscribed = Some(id);
        Some(HostCall::Subscribe {
            session_id: id.to_string(),
            cursor: followed.cursor,
        })
    }

    /// The connection's subscription ended with it.
    pub fn connection_lost(&mut self) {
        self.subscribed = None;
    }

    /// One `_nori/session/event` notification.
    pub fn apply(&mut self, params: Value) -> Vec<FeedOutput> {
        let envelope: SessionEnvelope = match serde_json::from_value(params) {
            Ok(envelope) => envelope,
            Err(error) => {
                tracing::warn!(
                    "skipping a session event the host sent in an unknown shape: {error}"
                );
                return Vec::new();
            }
        };
        let Ok(id) = ConversationId::from_string(&envelope.conversation_id) else {
            return Vec::new();
        };
        let Some(followed) = self.conversations.get_mut(&id) else {
            return Vec::new();
        };
        if followed.gap || envelope.seq <= followed.cursor {
            return Vec::new();
        }
        let generation = followed.generation;
        if followed.replaying && envelope.seq != followed.cursor + 1 {
            return Vec::new();
        }
        followed.replaying = false;
        if envelope.seq != followed.cursor + 1 {
            followed.gap = true;
            return vec![FeedOutput::Gap {
                generation,
                message: format!(
                    "Missed session events {}–{} from the host; showing them once it replays them.",
                    followed.cursor + 1,
                    envelope.seq - 1
                ),
            }];
        }
        followed.cursor = envelope.seq;
        if let SessionEvent::Nori(NoriEvent::SessionForked(fork)) = &envelope.event
            && fork.previous_conversation_id == envelope.conversation_id
            && let Ok(child) = ConversationId::from_string(&fork.new_conversation_id)
        {
            // The fork continues this widget's session, even when the host
            // listed the child before this event arrived.
            self.conversations.insert(
                child,
                Followed {
                    generation,
                    cursor: 0,
                    gap: false,
                    replaying: false,
                },
            );
            if self.subscribed == Some(id) {
                self.subscribed = Some(child);
            }
        }
        vec![FeedOutput::Event {
            generation,
            event: envelope.event,
        }]
    }
}
