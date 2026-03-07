use super::*;
use codex_protocol::plan_tool::StepStatus;
use pretty_assertions::assert_eq;

/// Test that translate_session_update_to_events correctly translates
/// an ACP Plan with mixed statuses to a PlanUpdate event.
#[test]
fn test_translate_plan_to_plan_update() {
    let plan = acp::Plan::new(vec![
        acp::PlanEntry::new(
            "Analyze the codebase",
            acp::PlanEntryPriority::High,
            acp::PlanEntryStatus::Completed,
        ),
        acp::PlanEntry::new(
            "Implement the feature",
            acp::PlanEntryPriority::Medium,
            acp::PlanEntryStatus::InProgress,
        ),
        acp::PlanEntry::new(
            "Write tests",
            acp::PlanEntryPriority::Low,
            acp::PlanEntryStatus::Pending,
        ),
    ]);
    let update = acp::SessionUpdate::Plan(plan);

    let mut pending_patches = std::collections::HashMap::new();
    let mut pending_tool_calls = std::collections::HashMap::new();
    let events =
        translate_session_update_to_events(&update, &mut pending_patches, &mut pending_tool_calls);

    assert_eq!(events.len(), 1, "Plan should produce exactly one event");

    match &events[0] {
        EventMsg::PlanUpdate(args) => {
            assert_eq!(args.plan.len(), 3);

            assert_eq!(args.plan[0].step, "Analyze the codebase");
            assert!(matches!(args.plan[0].status, StepStatus::Completed));

            assert_eq!(args.plan[1].step, "Implement the feature");
            assert!(matches!(args.plan[1].status, StepStatus::InProgress));

            assert_eq!(args.plan[2].step, "Write tests");
            assert!(matches!(args.plan[2].status, StepStatus::Pending));
        }
        other => panic!("Expected PlanUpdate event, got: {other:?}"),
    }
}

/// Test that an empty ACP Plan translates to a PlanUpdate with empty entries.
#[test]
fn test_translate_empty_plan() {
    let plan = acp::Plan::new(vec![]);
    let update = acp::SessionUpdate::Plan(plan);

    let mut pending_patches = std::collections::HashMap::new();
    let mut pending_tool_calls = std::collections::HashMap::new();
    let events =
        translate_session_update_to_events(&update, &mut pending_patches, &mut pending_tool_calls);

    assert_eq!(events.len(), 1, "Empty plan should still produce one event");

    match &events[0] {
        EventMsg::PlanUpdate(args) => {
            assert!(args.plan.is_empty(), "Plan entries should be empty");
        }
        other => panic!("Expected PlanUpdate event, got: {other:?}"),
    }
}
