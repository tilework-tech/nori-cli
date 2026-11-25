//! Queue snapshots and taking a queued prompt back before it is sent.
//!
//! The runtime queue is the only deliverable queue. Reclaiming removes one
//! entry by its submission event ID, so identical texts never collide, and a
//! prompt that has already left the queue to start a turn cannot be reclaimed.

use nori_protocol::QueueEntry;
use nori_protocol::acp::v1 as acp;

use crate::normalized::ClientEvent;
use crate::normalized::QueueChanged;
use crate::normalized::session_runtime::QueuedPrompt;
use crate::normalized::session_runtime::QueuedPromptKind;
use crate::normalized::session_runtime::SessionRuntime;

/// The queue as clients see it: user prompts only, oldest first.
pub(super) fn queue_changed(runtime: &SessionRuntime) -> ClientEvent {
    let entries: Vec<QueueEntry> = runtime
        .queue
        .iter()
        .filter(|prompt| prompt.kind == QueuedPromptKind::User)
        .map(|prompt| QueueEntry {
            id: prompt.event_id.clone(),
            text: prompt
                .display_text
                .clone()
                .unwrap_or_else(|| prompt.text.clone()),
            editable: editable_text(prompt).is_some(),
        })
        .collect();
    ClientEvent::QueueChanged(QueueChanged {
        prompts: entries.iter().map(|entry| entry.text.clone()).collect(),
        entries,
        paused: runtime.queue_pause,
    })
}

/// The exact text the user submitted, when the prompt is plain text that can
/// return to the composer without losing anything.
fn editable_text(prompt: &QueuedPrompt) -> Option<String> {
    if prompt.kind != QueuedPromptKind::User
        || prompt.reclaim.is_none()
        || prompt.original_content.is_empty()
    {
        return None;
    }
    let mut parts = Vec::with_capacity(prompt.original_content.len());
    for block in &prompt.original_content {
        let acp::ContentBlock::Text(text) = block else {
            return None;
        };
        parts.push(text.text.as_str());
    }
    Some(parts.join("\n"))
}

/// A prompt taken out of the queue, with the queue snapshot to publish.
#[derive(Debug)]
pub(crate) struct Reclaimed {
    pub prompt: QueuedPrompt,
    pub text: String,
    pub queue_changed: ClientEvent,
}

/// Remove the editable queued prompt `event_id`. Returns `None` when it is no
/// longer queued (already started, already reclaimed, or never existed) or is
/// not editable; the queue is then unchanged.
pub(crate) fn reclaim_queued(runtime: &mut SessionRuntime, event_id: &str) -> Option<Reclaimed> {
    let index = runtime
        .queue
        .iter()
        .position(|prompt| prompt.event_id == event_id)?;
    let text = editable_text(&runtime.queue[index])?;
    let prompt = runtime.queue.remove(index)?;
    if !super::queue_pause::has_user_prompt(runtime) {
        runtime.queue_pause = None;
    }
    Some(Reclaimed {
        prompt,
        text,
        queue_changed: queue_changed(runtime),
    })
}

/// Every queued user prompt, removed before it is sent, with the queue
/// snapshot to publish. Prompts the harness queued itself stay queued.
#[derive(Debug)]
pub(crate) struct Discarded {
    pub prompts: Vec<QueuedPrompt>,
    pub queue_changed: ClientEvent,
}

/// Drop every queued user prompt, editable or not, and clear the pause.
pub(crate) fn discard_queued(runtime: &mut SessionRuntime) -> Discarded {
    let (prompts, kept): (Vec<_>, Vec<_>) = std::mem::take(&mut runtime.queue)
        .into_iter()
        .partition(|prompt| prompt.kind == QueuedPromptKind::User);
    runtime.queue = kept.into();
    runtime.queue_pause = None;
    Discarded {
        prompts,
        queue_changed: queue_changed(runtime),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::session_reducer::InboundEvent;
    use crate::backend::session_reducer::SideEffect;
    use crate::backend::session_reducer::reduce;
    use crate::normalized::ClientEventNormalizer;
    use pretty_assertions::assert_eq;

    fn prompt(event_id: &str, text: &str) -> QueuedPrompt {
        let content = vec![acp::ContentBlock::Text(acp::TextContent::new(text))];
        QueuedPrompt {
            target_session: None,
            event_id: event_id.to_string(),
            kind: QueuedPromptKind::User,
            text: text.to_string(),
            prompt_meta: None,
            original_content: content.clone(),
            content,
            display_text: Some(text.to_string()),
            reclaim: Some(Box::default()),
        }
    }

    fn entry(id: &str, text: &str) -> QueueEntry {
        QueueEntry {
            id: id.to_string(),
            text: text.to_string(),
            editable: true,
        }
    }

    /// A runtime with a running turn and `queued` waiting behind it.
    fn busy_with(queued: Vec<QueuedPrompt>) -> (SessionRuntime, ClientEventNormalizer) {
        let mut runtime = SessionRuntime::new();
        let mut normalizer = ClientEventNormalizer::default();
        reduce(
            &mut runtime,
            InboundEvent::PromptSubmit(prompt("running", "running")),
            &mut normalizer,
        );
        for queued in queued {
            reduce(
                &mut runtime,
                InboundEvent::PromptSubmit(queued),
                &mut normalizer,
            );
        }
        (runtime, normalizer)
    }

    fn end_turn(
        runtime: &mut SessionRuntime,
        normalizer: &mut ClientEventNormalizer,
    ) -> Vec<String> {
        let out = reduce(
            runtime,
            InboundEvent::PromptResponse {
                stop_reason: acp::StopReason::EndTurn,
            },
            normalizer,
        );
        out.side_effects
            .into_iter()
            .filter_map(|effect| match effect {
                SideEffect::SendPrompt { prompt, .. } => Some(
                    prompt
                        .into_iter()
                        .filter_map(|block| match block {
                            acp::ContentBlock::Text(text) => Some(text.text),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join("\n"),
                ),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn reclaiming_one_duplicate_leaves_the_other_and_the_order_intact() {
        let (mut runtime, mut normalizer) = busy_with(vec![
            prompt("a", "alpha"),
            prompt("b", "dup"),
            prompt("c", "dup"),
        ]);

        let reclaimed = reclaim_queued(&mut runtime, "c").expect("queued");

        assert_eq!(reclaimed.text, "dup");
        assert_eq!(
            reclaimed.queue_changed,
            ClientEvent::QueueChanged(QueueChanged {
                prompts: vec!["alpha".to_string(), "dup".to_string()],
                entries: vec![entry("a", "alpha"), entry("b", "dup")],
                paused: None,
            })
        );
        assert_eq!(end_turn(&mut runtime, &mut normalizer), ["alpha"]);
        assert_eq!(end_turn(&mut runtime, &mut normalizer), ["dup"]);
        assert_eq!(
            end_turn(&mut runtime, &mut normalizer),
            Vec::<String>::new()
        );
    }

    #[test]
    fn a_prompt_is_reclaimed_at_most_once() {
        let (mut runtime, _) = busy_with(vec![prompt("a", "alpha")]);

        assert!(reclaim_queued(&mut runtime, "a").is_some());
        assert!(reclaim_queued(&mut runtime, "a").is_none());
    }

    #[test]
    fn a_prompt_that_started_its_turn_cannot_be_reclaimed() {
        let (mut runtime, mut normalizer) = busy_with(vec![prompt("a", "alpha")]);
        assert_eq!(end_turn(&mut runtime, &mut normalizer), ["alpha"]);

        assert!(reclaim_queued(&mut runtime, "a").is_none());
    }

    #[test]
    fn prompts_with_images_stay_queued_and_are_marked_not_editable() {
        let mut with_image = prompt("img", "look at this");
        with_image
            .original_content
            .push(acp::ContentBlock::Image(acp::ImageContent::new(
                "aGVsbG8=",
                "image/png",
            )));
        let (mut runtime, _) = busy_with(vec![with_image]);

        assert!(reclaim_queued(&mut runtime, "img").is_none());
        assert_eq!(
            queue_changed(&runtime),
            ClientEvent::QueueChanged(QueueChanged {
                prompts: vec!["look at this".to_string()],
                entries: vec![QueueEntry {
                    editable: false,
                    ..entry("img", "look at this")
                }],
                paused: None,
            })
        );
    }

    #[test]
    fn prompts_the_local_user_did_not_type_are_never_editable() {
        let mut child_result = prompt("child", "result from a delegated session");
        child_result.reclaim = None;
        let (mut runtime, _) = busy_with(vec![child_result]);

        assert!(reclaim_queued(&mut runtime, "child").is_none());
        assert_eq!(
            queue_changed(&runtime),
            ClientEvent::QueueChanged(QueueChanged {
                prompts: vec!["result from a delegated session".to_string()],
                entries: vec![QueueEntry {
                    editable: false,
                    ..entry("child", "result from a delegated session")
                }],
                paused: None,
            })
        );
    }

    #[test]
    fn reclaimed_text_is_what_the_user_typed_not_the_injected_context() {
        let mut with_context = prompt("ctx", "fix the bug");
        with_context.text = "hook context\nfix the bug".to_string();
        with_context.content.insert(
            0,
            acp::ContentBlock::Text(acp::TextContent::new("hook context\n")),
        );
        let (mut runtime, _) = busy_with(vec![with_context]);

        let reclaimed = reclaim_queued(&mut runtime, "ctx").expect("queued");

        assert_eq!(reclaimed.text, "fix the bug");
    }
}
