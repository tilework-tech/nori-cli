use super::*;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;

fn update(value: Value) -> acp::SessionUpdate {
    serde_json::from_value(value).expect("valid synthetic update")
}

fn status(meta: Value) -> acp::SessionUpdate {
    update(json!({"sessionUpdate":"session_info_update","_meta":meta}))
}

#[test]
fn matched_terminal_marker_ends_only_its_external_turn() {
    let mut turns = ExternalActivity::default();
    let sid = acp::SessionId::new("synthetic-session");
    let start = |id| status(json!({"nori/turnState":"active","nori/turnId":id}));
    let end = |id| {
        status(json!({"nori/turnState":"idle","nori/turnId":id,"nori/stopReason":"cancelled"}))
    };
    let first = turns.observe(&sid, &start("first")).remove(0);
    let replacement = turns.observe(&sid, &start("second"));
    assert_eq!(replacement[0].id, first.id);
    assert_eq!(replacement[0].state, ExternalTurnState::Superseded);
    let second = &replacement[1];
    assert_ne!(first.id, second.id);
    assert!(turns.observe(&sid, &end("first")).is_empty());
    assert!(turns.observe(&sid, &start("second")).is_empty());
    assert_eq!(
        turns.observe(&sid, &end("second")),
        [ExternalTurn {
            id: second.id.clone(),
            state: ExternalTurnState::Cancelled
        }]
    );
    assert!(turns.observe(&sid, &end("second")).is_empty());
}

#[test]
fn content_starts_one_observation_and_plain_idle_has_no_stop_reason() {
    let mut turns = ExternalActivity::default();
    let sid = acp::SessionId::new("synthetic-session");
    let content = update(
        json!({"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"Planning a trail."}}),
    );
    assert!(
        turns
            .observe(&sid, &status(json!({"unrelated":true})))
            .is_empty()
    );
    let start = turns.observe(&sid, &content).remove(0);
    assert!(turns.observe(&sid, &content).is_empty());
    let idle = status(json!({"nori":{"status":"idle"}}));
    assert_eq!(
        turns.observe(&sid, &idle),
        [ExternalTurn {
            id: start.id.clone(),
            state: ExternalTurnState::Idle
        }]
    );
    assert_ne!(turns.observe(&sid, &content)[0].id, start.id);
}

#[test]
fn a_tagged_echo_supplies_the_id_for_observer_completion() {
    let mut turns = ExternalActivity::default();
    let sid = acp::SessionId::new("synthetic-session");
    let echo = update(
        json!({"sessionUpdate":"user_message_chunk","_meta":{"nori/turnId":"trail"},"content":{"type":"text","text":"Map a trail."}}),
    );
    let first = turns.observe(&sid, &echo).remove(0);
    let idle =
        status(json!({"nori/turnState":"idle","nori/turnId":"trail","nori/stopReason":"end_turn"}));
    assert_eq!(
        turns.observe(&sid, &idle),
        [ExternalTurn {
            id: first.id,
            state: ExternalTurnState::Idle
        }]
    );
}

#[test]
fn a_turn_annotated_after_content_keeps_its_pending_cancellation() {
    let mut turns = ExternalActivity::default();
    let sid = acp::SessionId::new("synthetic-session");
    let content = update(
        json!({"sessionUpdate":"agent_thought_chunk","content":{"type":"text","text":"Sorting a route."}}),
    );
    let start = turns.observe(&sid, &content).remove(0);
    turns.set_state(&start.id, ExternalTurnState::CancelRequested);
    assert!(
        turns
            .observe(
                &sid,
                &status(json!({"nori/turnState":"active","nori/turnId":"ridge"}))
            )
            .is_empty()
    );
    assert_eq!(
        turns.current(&sid).unwrap().state,
        ExternalTurnState::CancelRequested
    );
    assert_eq!(
        turns.observe(
            &sid,
            &status(
                json!({"nori/turnState":"idle","nori/turnId":"ridge","nori/stopReason":"cancelled"})
            )
        )[0]
        .id,
        start.id
    );
}
