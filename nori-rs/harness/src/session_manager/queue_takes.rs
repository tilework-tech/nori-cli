//! Answers to `_nori/session/queue/take`, remembered by the caller's request
//! key so a terminal that lost the reply can ask again and get the same text.
//! Without this, a reply lost after the host withdrew the prompt would lose
//! the prompt: it is no longer queued, and no composer holds it.

use std::collections::VecDeque;
use std::sync::PoisonError;

use super::SessionManager;
use crate::ConversationId;

/// How many answers the host keeps; a retry comes right after a reconnect.
const REMEMBERED: i32 = 256;

#[derive(Default)]
pub(super) struct QueueTakes {
    answers: VecDeque<(ConversationId, String, Option<String>)>,
}

impl SessionManager {
    /// The answer already given to `key` for a take in `conversation`.
    pub fn remembered_take(
        &self,
        conversation: ConversationId,
        key: &str,
    ) -> Option<Option<String>> {
        let takes = self
            .shared
            .queue_takes
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        takes
            .answers
            .iter()
            .find(|(id, remembered, _)| *id == conversation && remembered == key)
            .map(|(_, _, text)| text.clone())
    }

    /// Remember the answer given to `key`, dropping the oldest past the limit.
    pub fn remember_take(&self, conversation: ConversationId, key: String, text: Option<String>) {
        let mut takes = self
            .shared
            .queue_takes
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        takes.answers.push_back((conversation, key, text));
        while takes.answers.len() > REMEMBERED as usize {
            takes.answers.pop_front();
        }
    }
}
