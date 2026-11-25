//! Verifier-only checks of JoinedFeed ordering, gaps, switching and forks.
//! Injected into a verifier tree as `app::joined_feed_verifier`; never shipped.

use nori_harness::ConversationId;
use nori_protocol::NoriEvent;
use nori_protocol::Notice;
use nori_protocol::SessionEnvelope;
use nori_protocol::SessionEvent;
use serde_json::Value;

use super::joined_feed::FeedOutput;
use super::joined_feed::JoinedFeed;
use crate::app_event::SessionGeneration;
use crate::chatwidget::agent::next_session_generation;
use crate::host_client::HostCall;

fn env(id: ConversationId, seq: i64, text: String) -> Value {
    serde_json::to_value(SessionEnvelope {
        conversation_id: id.to_string(),
        agent: "verifier-agent".into(),
        seq,
        event: SessionEvent::Nori(NoriEvent::Notice(Notice { message: text })),
    })
    .unwrap()
}

#[derive(Debug, PartialEq)]
enum Out { E(SessionGeneration, String), Gap(SessionGeneration) }

fn run(feed: &mut JoinedFeed, v: Value) -> Vec<Out> {
    feed.apply(v).into_iter().map(|o| match o {
        FeedOutput::Event { generation, event: SessionEvent::Nori(NoriEvent::Notice(n)) } => Out::E(generation, n.message),
        FeedOutput::Event { generation, .. } => Out::E(generation, "<other>".into()),
        FeedOutput::Gap { generation, .. } => Out::Gap(generation),
    }).collect()
}

fn cursor_of(call: Option<HostCall>) -> Option<(String, i64)> {
    match call? {
        HostCall::Subscribe { session_id, cursor } => Some((session_id, cursor)),
        _ => panic!("not a subscribe"),
    }
}

#[test]
fn f1_shuffled_duplicates_over_many_resubscribes_apply_each_seq_exactly_once_in_order() {
    let a = ConversationId::new();
    let ga = next_session_generation();
    let mut feed = JoinedFeed::default();
    feed.follow(a, ga);
    let mut applied: Vec<i64> = Vec::new();
    let mut gaps = 0;
    // Deterministic pseudo-random delivery: replays restart at the cursor,
    // repeat old seqs, and sometimes skip ahead.
    let mut x: u64 = 0x9e3779b97f4a7c15;
    let mut next = || { x ^= x << 13; x ^= x >> 7; x ^= x << 17; x };
    let total = 3000i64;
    let mut sent_hi = 0i64;
    while applied.len() < total as usize {
        let (_, cursor) = cursor_of(feed.subscribe(a)).unwrap_or((a.to_string(), *applied.last().unwrap_or(&0)));
        let mut seq = cursor - (next() % 5) as i64; // stale overlap
        if seq < 1 { seq = 1; }
        let burst = 1 + (next() % 40) as i64;
        for _ in 0..burst {
            if seq > total { break; }
            if next() % 23 == 0 { seq += 1 + (next() % 3) as i64; } // lost events
            if seq > total { break; }
            for out in run(&mut feed, env(a, seq, format!("a{seq}"))) {
                match out { Out::E(g, t) => { assert_eq!(g, ga); applied.push(t[1..].parse().unwrap()); } Out::Gap(_) => gaps += 1 }
            }
            sent_hi = sent_hi.max(seq);
            seq += 1;
        }
        feed.connection_lost();
    }
    let want: Vec<i64> = (1..=total).collect();
    assert_eq!(applied, want);
    assert!(gaps > 0, "the stream must have exercised gaps");
    let _ = sent_hi;
}

#[test]
fn f2_switching_away_and_back_replays_only_what_was_not_shown() {
    let (a, b) = (ConversationId::new(), ConversationId::new());
    let (ga, gb) = (next_session_generation(), next_session_generation());
    let mut feed = JoinedFeed::default();
    feed.follow(a, ga);
    feed.follow(b, gb);
    assert_eq!(cursor_of(feed.subscribe(a)), Some((a.to_string(), 0)));
    for s in 1..=5 { run(&mut feed, env(a, s, format!("a{s}"))); }
    assert_eq!(feed.subscribe(a).is_none(), true, "already subscribed and complete");
    assert_eq!(cursor_of(feed.subscribe(b)), Some((b.to_string(), 0)));
    // Late events from A's old subscription still apply to A in order.
    assert_eq!(run(&mut feed, env(a, 6, "a6".into())), vec![Out::E(ga, "a6".into())]);
    for s in 1..=3 { run(&mut feed, env(b, s, format!("b{s}"))); }
    assert_eq!(cursor_of(feed.subscribe(a)), Some((a.to_string(), 6)));
    // The host replays from 7; a duplicate 6 is dropped.
    assert!(run(&mut feed, env(a, 6, "a6".into())).is_empty());
    assert_eq!(run(&mut feed, env(a, 7, "a7".into())), vec![Out::E(ga, "a7".into())]);
    assert_eq!(cursor_of(feed.subscribe(b)), Some((b.to_string(), 3)));
}

#[test]
fn f3_a_gap_is_reported_once_even_across_many_late_events() {
    let a = ConversationId::new();
    let ga = next_session_generation();
    let mut feed = JoinedFeed::default();
    feed.follow(a, ga);
    feed.subscribe(a);
    run(&mut feed, env(a, 1, "a1".into()));
    assert_eq!(run(&mut feed, env(a, 4, "a4".into())), vec![Out::Gap(ga)]);
    for s in 5..50 { assert!(run(&mut feed, env(a, s, format!("a{s}"))).is_empty()); }
    assert_eq!(cursor_of(feed.subscribe(a)), Some((a.to_string(), 1)));
    // Old subscription still trickling in: silent.
    for s in 50..60 { assert!(run(&mut feed, env(a, s, format!("a{s}"))).is_empty()); }
    for s in 2..=60 { assert_eq!(run(&mut feed, env(a, s, format!("a{s}"))), vec![Out::E(ga, format!("a{s}"))]); }
}

#[test]
fn f4_events_for_a_session_nobody_follows_are_ignored() {
    let (a, z) = (ConversationId::new(), ConversationId::new());
    let ga = next_session_generation();
    let mut feed = JoinedFeed::default();
    feed.follow(a, ga);
    feed.subscribe(a);
    assert!(run(&mut feed, env(z, 1, "z1".into())).is_empty());
    assert!(feed.subscribe(z).is_none());
    let mut bad = env(a, 1, "x".into());
    bad["seq"] = Value::String("one".into());
    assert!(feed.apply(bad).is_empty(), "malformed envelopes are skipped");
    assert_eq!(run(&mut feed, env(a, 1, "a1".into())), vec![Out::E(ga, "a1".into())]);
}

#[test]
fn f5_reconnect_resubscribes_the_same_session_from_its_cursor() {
    let a = ConversationId::new();
    let ga = next_session_generation();
    let mut feed = JoinedFeed::default();
    feed.follow(a, ga);
    feed.subscribe(a);
    for s in 1..=9 { run(&mut feed, env(a, s, format!("a{s}"))); }
    feed.connection_lost();
    assert_eq!(cursor_of(feed.subscribe(a)), Some((a.to_string(), 9)));
    assert!(feed.subscribe(a).is_none());
}
