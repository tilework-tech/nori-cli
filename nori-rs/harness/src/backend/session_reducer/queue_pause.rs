//! Holding queued prompts after a turn that did not complete.
//!
//! When a turn is interrupted, fails, or stops early while prompts wait
//! behind it, the queue keeps every entry in order but stops sending on its
//! own. Only an explicit resume starts the next queued prompt; after that the
//! queue drains one prompt per completed turn as usual.

use nori_protocol::QueuePause;

use super::ReduceOutput;
use super::drain_queue;
use super::reclaim::queue_changed;
use crate::normalized::session_runtime::SessionPhase;
use crate::normalized::session_runtime::SessionRuntime;
use crate::normalized::session_runtime::SteerFence;

/// Hold whatever is queued after a turn that did not complete.
pub(super) fn pause(runtime: &mut SessionRuntime, reason: QueuePause, out: &mut ReduceOutput) {
    if runtime.queue.is_empty() {
        return;
    }
    runtime.queue_pause = Some(reason);
    out.events.push(queue_changed(runtime));
}

/// Clear a pause. An idle session starts the next queued prompt now; a busy
/// one drains when its running turn completes.
pub(super) fn resume(runtime: &mut SessionRuntime, out: &mut ReduceOutput) {
    if runtime.queue_pause.take().is_none() {
        return;
    }
    if runtime.phase == SessionPhase::Idle {
        if runtime.steer_fence == SteerFence::Open {
            drain_queue(runtime, out);
            return;
        }
        runtime.steer_fence = SteerFence::DrainAfterReply;
    }
    out.events.push(queue_changed(runtime));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::session_reducer::InboundEvent;
    use crate::backend::session_reducer::SideEffect;
    use crate::backend::session_reducer::reduce;
    use crate::normalized::ClientEvent;
    use crate::normalized::ClientEventNormalizer;
    use crate::normalized::TurnFailure;
    use crate::normalized::session_runtime::QueuedPrompt;
    use crate::normalized::session_runtime::QueuedPromptKind;
    use nori_protocol::acp::v1 as acp;
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

    struct Session {
        runtime: SessionRuntime,
        normalizer: ClientEventNormalizer,
    }

    /// What one reducer step sent to the agent and the last queue snapshot
    /// it published.
    #[derive(Debug, PartialEq)]
    struct Step {
        sent: Vec<String>,
        queue: Option<(Vec<String>, Option<QueuePause>)>,
    }

    impl Session {
        fn running_with(queued: &[(&str, &str)]) -> Self {
            let mut session = Self {
                runtime: SessionRuntime::new(),
                normalizer: ClientEventNormalizer::default(),
            };
            session.apply(InboundEvent::PromptSubmit(prompt("running", "running")));
            for (id, text) in queued {
                session.apply(InboundEvent::PromptSubmit(prompt(id, text)));
            }
            session
        }

        fn apply(&mut self, event: InboundEvent) -> Step {
            let out = reduce(&mut self.runtime, event, &mut self.normalizer);
            Step {
                sent: out
                    .side_effects
                    .into_iter()
                    .filter_map(|effect| match effect {
                        SideEffect::SendPrompt { prompt, .. } => Some(
                            prompt
                                .into_iter()
                                .filter_map(|block| match block {
                                    acp::ContentBlock::Text(text) => Some(text.text),
                                    _ => None,
                                })
                                .collect::<String>(),
                        ),
                        _ => None,
                    })
                    .collect(),
                queue: out.events.into_iter().rev().find_map(|event| match event {
                    ClientEvent::QueueChanged(queue) => Some((queue.prompts, queue.paused)),
                    _ => None,
                }),
            }
        }

        fn end(&mut self, stop_reason: acp::StopReason) -> Step {
            self.apply(InboundEvent::PromptResponse { stop_reason })
        }

        fn interrupt(&mut self) -> Step {
            self.apply(InboundEvent::CancelSubmit);
            self.end(acp::StopReason::Cancelled)
        }
    }

    fn texts(items: &[&str]) -> Vec<String> {
        items.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn an_interrupted_turn_keeps_the_queue_in_order_and_sends_nothing() {
        let mut session = Session::running_with(&[("a", "alpha"), ("b", "dup"), ("c", "dup")]);

        let step = session.interrupt();

        assert_eq!(
            step,
            Step {
                sent: Vec::new(),
                queue: Some((
                    texts(&["alpha", "dup", "dup"]),
                    Some(QueuePause::Interrupted)
                )),
            }
        );
    }

    #[test]
    fn a_failed_turn_pauses_the_queue() {
        let mut session = Session::running_with(&[("a", "alpha")]);

        let step = session.apply(InboundEvent::PromptFailed {
            failure: Some(TurnFailure::Fatal),
        });

        assert_eq!(
            step,
            Step {
                sent: Vec::new(),
                queue: Some((texts(&["alpha"]), Some(QueuePause::TurnFailed))),
            }
        );
    }

    #[test]
    fn a_turn_that_stops_early_pauses_the_queue() {
        let mut session = Session::running_with(&[("a", "alpha")]);

        let step = session.end(acp::StopReason::MaxTokens);

        assert_eq!(step.sent, Vec::<String>::new());
        assert_eq!(
            step.queue,
            Some((texts(&["alpha"]), Some(QueuePause::TurnFailed)))
        );
    }

    #[test]
    fn an_interrupt_with_nothing_queued_does_not_pause() {
        let mut session = Session::running_with(&[]);

        let step = session.interrupt();

        assert_eq!(step.queue, None);
        let next = session.apply(InboundEvent::PromptSubmitIfIdle(prompt("n", "next")));
        assert_eq!(next.sent, texts(&["next"]));
    }

    #[test]
    fn resume_sends_each_surviving_prompt_once_in_order() {
        let mut session = Session::running_with(&[("a", "alpha"), ("b", "dup"), ("c", "dup")]);
        session.interrupt();

        let resumed = session.apply(InboundEvent::ResumeQueue);
        assert_eq!(
            resumed,
            Step {
                sent: texts(&["alpha"]),
                queue: Some((texts(&["dup", "dup"]), None)),
            }
        );
        assert_eq!(
            session.apply(InboundEvent::ResumeQueue).sent,
            Vec::<String>::new(),
            "a second resume while the first prompt runs sends nothing extra"
        );
        assert_eq!(session.end(acp::StopReason::EndTurn).sent, texts(&["dup"]));
        assert_eq!(session.end(acp::StopReason::EndTurn).sent, texts(&["dup"]));
        assert_eq!(
            session.end(acp::StopReason::EndTurn).sent,
            Vec::<String>::new()
        );
    }

    #[test]
    fn a_prompt_sent_while_paused_runs_and_the_queue_stays_paused() {
        let mut session = Session::running_with(&[("a", "alpha")]);
        session.interrupt();

        let fresh = session.apply(InboundEvent::PromptSubmitIfIdle(prompt("n", "fresh")));
        assert_eq!(fresh.sent, texts(&["fresh"]));

        let ended = session.end(acp::StopReason::EndTurn);
        assert_eq!(ended.sent, Vec::<String>::new());
        assert_eq!(
            session.apply(InboundEvent::ResumeQueue).sent,
            texts(&["alpha"])
        );
    }

    #[test]
    fn resume_during_a_running_turn_drains_after_it_completes() {
        let mut session = Session::running_with(&[("a", "alpha")]);
        session.interrupt();
        session.apply(InboundEvent::PromptSubmitIfIdle(prompt("n", "fresh")));

        let resumed = session.apply(InboundEvent::ResumeQueue);
        assert_eq!(resumed.sent, Vec::<String>::new());
        assert_eq!(resumed.queue, Some((texts(&["alpha"]), None)));

        assert_eq!(
            session.end(acp::StopReason::EndTurn).sent,
            texts(&["alpha"])
        );
    }

    #[test]
    fn resume_without_a_pause_changes_nothing() {
        let mut session = Session::running_with(&[("a", "alpha")]);

        assert_eq!(
            session.apply(InboundEvent::ResumeQueue),
            Step {
                sent: Vec::new(),
                queue: None,
            }
        );
        assert_eq!(
            session.end(acp::StopReason::EndTurn).sent,
            texts(&["alpha"])
        );
    }

    #[test]
    fn taking_back_the_last_paused_prompt_clears_the_pause() {
        let mut session = Session::running_with(&[("a", "alpha"), ("b", "beta")]);
        session.interrupt();

        let snapshot = |event: ClientEvent| match event {
            ClientEvent::QueueChanged(queue) => (queue.prompts, queue.paused),
            other => panic!("expected a queue snapshot, got {other:?}"),
        };
        let first = crate::backend::session_reducer::reclaim_queued(&mut session.runtime, "a")
            .expect("queued");
        assert_eq!(
            snapshot(first.queue_changed),
            (texts(&["beta"]), Some(QueuePause::Interrupted))
        );
        let last = crate::backend::session_reducer::reclaim_queued(&mut session.runtime, "b")
            .expect("queued");
        assert_eq!(snapshot(last.queue_changed), (Vec::new(), None));

        session.apply(InboundEvent::PromptSubmitIfIdle(prompt("n", "fresh")));
        session.apply(InboundEvent::PromptSubmit(prompt("q", "queued later")));
        assert_eq!(
            session.end(acp::StopReason::EndTurn).sent,
            texts(&["queued later"]),
            "a queue emptied by the user sends new follow-ups normally"
        );
    }

    #[test]
    fn an_interrupt_after_resume_pauses_what_is_left() {
        let mut session = Session::running_with(&[("a", "alpha"), ("b", "beta")]);
        session.interrupt();
        assert_eq!(
            session.apply(InboundEvent::ResumeQueue).sent,
            texts(&["alpha"])
        );

        let step = session.interrupt();

        assert_eq!(
            step,
            Step {
                sent: Vec::new(),
                queue: Some((texts(&["beta"]), Some(QueuePause::Interrupted))),
            }
        );
    }
}
