use nori_harness::ConversationId;
use nori_protocol::NoriEvent;
use nori_protocol::Notice;
use nori_protocol::SessionEnvelope;
use nori_protocol::SessionEvent;
use pretty_assertions::assert_eq;
use serde_json::Value;

use super::joined_feed::FeedOutput;
use super::joined_feed::JoinedFeed;
use crate::app_event::SessionGeneration;
use crate::chatwidget::agent::next_session_generation;
use crate::host_client::HostCall;

fn notice(text: &str) -> SessionEvent {
    SessionEvent::Nori(NoriEvent::Notice(Notice {
        message: text.to_string(),
    }))
}

fn envelope(id: ConversationId, seq: i64, event: SessionEvent) -> Value {
    serde_json::to_value(SessionEnvelope {
        conversation_id: id.to_string(),
        agent: "mock-model".into(),
        seq,
        event,
    })
    .unwrap()
}

fn said(id: ConversationId, seq: i64) -> Value {
    envelope(id, seq, notice(&format!("{id} #{seq}")))
}

/// What each widget was shown, as the notice text, in arrival order.
fn shown(outputs: Vec<FeedOutput>) -> Vec<(SessionGeneration, String)> {
    outputs
        .into_iter()
        .filter_map(|output| match output {
            FeedOutput::Event {
                generation,
                event: SessionEvent::Nori(NoriEvent::Notice(Notice { message })),
            } => Some((generation, message)),
            FeedOutput::Event { .. } | FeedOutput::Gap { .. } => None,
        })
        .collect()
}

fn subscription(call: Option<HostCall>) -> Option<(String, i64)> {
    match call {
        Some(HostCall::Subscribe { session_id, cursor }) => Some((session_id, cursor)),
        Some(other) => panic!("expected a subscription, got {other:?}"),
        None => None,
    }
}

#[test]
fn replayed_history_then_live_events_reach_the_widget_once_in_order() {
    let id = ConversationId::new();
    let generation = next_session_generation();
    let mut feed = JoinedFeed::default();
    feed.follow(id, generation);

    let mut outputs = Vec::new();
    for seq in [1, 2, 3, 2, 3, 4] {
        outputs.extend(feed.apply(said(id, seq)));
    }

    assert_eq!(
        shown(outputs),
        (1..=4)
            .map(|seq| (generation, format!("{id} #{seq}")))
            .collect::<Vec<_>>()
    );
}

#[test]
fn a_gap_is_reported_once_and_nothing_after_it_applies_until_resubscribed() {
    let id = ConversationId::new();
    let generation = next_session_generation();
    let mut feed = JoinedFeed::default();
    feed.follow(id, generation);
    assert_eq!(subscription(feed.subscribe(id)), Some((id.to_string(), 0)));

    feed.apply(said(id, 1));
    let gap = feed.apply(said(id, 3));
    let later = feed.apply(said(id, 4));

    assert!(matches!(
        gap.as_slice(),
        [FeedOutput::Gap { generation: shown_to, .. }] if *shown_to == generation
    ));
    assert!(later.is_empty());
    // Asking again replays from the last event this terminal applied.
    assert_eq!(subscription(feed.subscribe(id)), Some((id.to_string(), 1)));
    assert_eq!(
        shown(feed.apply(said(id, 2))),
        vec![(generation, format!("{id} #2"))]
    );
}

#[test]
fn late_events_from_before_a_resubscription_are_not_reported_as_another_gap() {
    let id = ConversationId::new();
    let generation = next_session_generation();
    let mut feed = JoinedFeed::default();
    feed.follow(id, generation);
    feed.subscribe(id);
    feed.apply(said(id, 1));
    assert_eq!(feed.apply(said(id, 3)).len(), 1);

    feed.subscribe(id);
    assert!(feed.apply(said(id, 4)).is_empty());
    let replayed: Vec<_> = [2, 3, 4]
        .into_iter()
        .flat_map(|seq| feed.apply(said(id, seq)))
        .collect();
    assert_eq!(
        shown(replayed),
        (2..=4)
            .map(|seq| (generation, format!("{id} #{seq}")))
            .collect::<Vec<_>>()
    );
}

#[test]
fn interleaved_sessions_keep_their_own_order_and_unfollowed_ones_are_ignored() {
    let (first, second, stranger) = (
        ConversationId::new(),
        ConversationId::new(),
        ConversationId::new(),
    );
    let (one, two) = (next_session_generation(), next_session_generation());
    let mut feed = JoinedFeed::default();
    feed.follow(first, one);
    feed.follow(second, two);

    let mut outputs = Vec::new();
    for message in [
        said(first, 1),
        said(second, 1),
        said(stranger, 1),
        said(second, 2),
        said(first, 2),
    ] {
        outputs.extend(feed.apply(message));
    }

    assert_eq!(
        shown(outputs),
        vec![
            (one, format!("{first} #1")),
            (two, format!("{second} #1")),
            (two, format!("{second} #2")),
            (one, format!("{first} #2")),
        ]
    );
}

#[test]
fn a_fork_moves_the_widget_to_the_new_conversation() {
    let (parent, child) = (ConversationId::new(), ConversationId::new());
    let generation = next_session_generation();
    let mut feed = JoinedFeed::default();
    feed.follow(parent, generation);
    // The host listed the child, with a widget of its own, first.
    feed.follow(child, next_session_generation());

    let fork = SessionEvent::Nori(NoriEvent::SessionForked(nori_protocol::SessionForked {
        previous_conversation_id: parent.to_string(),
        new_conversation_id: child.to_string(),
        new_acp_session_id: nori_protocol::acp::v1::SessionId::new("synthetic-acp-session"),
    }));
    feed.apply(envelope(parent, 1, fork));

    assert_eq!(
        shown(feed.apply(said(child, 1))),
        vec![(generation, format!("{child} #1"))]
    );
    assert_eq!(feed.generation_of(child), Some(generation));
}

#[test]
fn one_subscription_follows_the_session_on_screen_and_returns_after_a_reconnect() {
    let (first, second) = (ConversationId::new(), ConversationId::new());
    let mut feed = JoinedFeed::default();
    feed.follow(first, next_session_generation());
    feed.follow(second, next_session_generation());

    assert_eq!(
        subscription(feed.subscribe(first)),
        Some((first.to_string(), 0))
    );
    assert_eq!(subscription(feed.subscribe(first)), None);
    feed.apply(said(first, 1));
    assert_eq!(
        subscription(feed.subscribe(second)),
        Some((second.to_string(), 0))
    );
    // Back on the first session, the host replays what it missed.
    assert_eq!(
        subscription(feed.subscribe(first)),
        Some((first.to_string(), 1))
    );

    feed.connection_lost();
    assert_eq!(
        subscription(feed.subscribe(first)),
        Some((first.to_string(), 1))
    );
}
