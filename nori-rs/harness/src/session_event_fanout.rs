//! Ordered fan-out for public session events: one primary unbounded consumer
//! (the launching frontend) plus bounded subscribers such as the remote ACP
//! transport. A subscriber whose queue overflows is dropped so it can never
//! block the harness or the primary frontend; its receiver closing tells that
//! consumer to tear down.

use std::sync::Arc;

use nori_protocol::SessionEvent;
use tokio::sync::mpsc;
use tokio::sync::mpsc::UnboundedSender;

/// Bounded queue capacity for each additional [`SessionEvent`] subscriber.
const SUBSCRIBER_QUEUE_EVENTS: usize = 1024;

#[derive(Clone)]
pub(crate) struct SessionEventFanout {
    primary: UnboundedSender<SessionEvent>,
    subscribers: Arc<std::sync::Mutex<Vec<mpsc::Sender<SessionEvent>>>>,
}

impl SessionEventFanout {
    pub(crate) fn new(primary: UnboundedSender<SessionEvent>) -> Self {
        Self {
            primary,
            subscribers: Arc::new(std::sync::Mutex::new(Vec::new())),
        }
    }

    /// Deliver to every subscriber, then the primary receiver. Errors only
    /// when the primary receiver is gone, mirroring `UnboundedSender::send`
    /// (boxed: a `SessionEvent` is too large to carry in an `Err` directly).
    pub(crate) fn send(
        &self,
        event: SessionEvent,
    ) -> Result<(), Box<mpsc::error::SendError<SessionEvent>>> {
        if let Ok(mut subscribers) = self.subscribers.lock() {
            subscribers.retain(|subscriber| match subscriber.try_send(event.clone()) {
                Ok(()) => true,
                Err(mpsc::error::TrySendError::Full(_)) => {
                    tracing::warn!("dropping session event subscriber that fell behind");
                    false
                }
                Err(mpsc::error::TrySendError::Closed(_)) => false,
            });
        }
        self.primary.send(event).map_err(Box::new)
    }

    pub(crate) fn subscribe(&self) -> mpsc::Receiver<SessionEvent> {
        let (subscriber_tx, subscriber_rx) = mpsc::channel(SUBSCRIBER_QUEUE_EVENTS);
        if let Ok(mut subscribers) = self.subscribers.lock() {
            subscribers.push(subscriber_tx);
        }
        subscriber_rx
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nori_protocol::NoriEvent;
    use nori_protocol::Notice;
    use pretty_assertions::assert_eq;

    fn notice(index: usize) -> SessionEvent {
        SessionEvent::Nori(NoriEvent::Notice(Notice {
            message: format!("event {index}"),
        }))
    }

    fn notice_message(event: Option<SessionEvent>) -> String {
        match event {
            Some(SessionEvent::Nori(NoriEvent::Notice(notice))) => notice.message,
            other => panic!("expected a notice event, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn subscribers_receive_ordered_events_without_disturbing_the_primary() {
        let (primary_tx, mut primary_rx) = mpsc::unbounded_channel();
        let fanout = SessionEventFanout::new(primary_tx);
        let mut subscriber = fanout.subscribe();

        for index in 0..3 {
            fanout.send(notice(index)).expect("primary receiver alive");
        }

        for index in 0..3 {
            assert_eq!(
                notice_message(subscriber.recv().await),
                format!("event {index}")
            );
            assert_eq!(
                notice_message(primary_rx.recv().await),
                format!("event {index}")
            );
        }
    }

    #[tokio::test]
    async fn an_overflowing_subscriber_is_dropped_and_its_receiver_closes() {
        let (primary_tx, mut primary_rx) = mpsc::unbounded_channel();
        let fanout = SessionEventFanout::new(primary_tx);
        let mut stalled = fanout.subscribe();

        // One event past capacity: delivery must drop the stalled subscriber
        // instead of blocking, while the primary receives everything.
        for index in 0..=SUBSCRIBER_QUEUE_EVENTS {
            fanout.send(notice(index)).expect("primary receiver alive");
        }

        for index in 0..SUBSCRIBER_QUEUE_EVENTS {
            assert_eq!(
                notice_message(stalled.recv().await),
                format!("event {index}")
            );
        }
        assert!(stalled.recv().await.is_none());

        for index in 0..=SUBSCRIBER_QUEUE_EVENTS {
            assert_eq!(
                notice_message(primary_rx.recv().await),
                format!("event {index}")
            );
        }
    }
}
