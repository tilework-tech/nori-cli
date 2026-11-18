use nori_harness::FollowUpIntent;
use nori_harness::FollowUpStatus;
use nori_protocol::acp::v1::RequestId;

pub(crate) enum Delivery {
    Default,
    IfIdle,
    FollowUp {
        turn: RequestId,
        intent: FollowUpIntent,
    },
}

pub(crate) fn notice(status: FollowUpStatus) -> &'static str {
    match status {
        FollowUpStatus::Injected => "Delivered to the active turn.",
        FollowUpStatus::Queued => "Queued for the next turn.",
        FollowUpStatus::UnsupportedQueued => "Steering unavailable. Added to the next-turn queue.",
        FollowUpStatus::IdleQueued => "The turn ended before steering. Queued for the next turn.",
        FollowUpStatus::Rejected => "Follow-up rejected for the requested turn.",
        FollowUpStatus::Unknown => "Delivery unknown. Check the conversation before sending again.",
    }
}

pub(crate) fn delivery_notice(status: FollowUpStatus, id: &str, text: &str) -> String {
    let notice = notice(status);
    match status {
        FollowUpStatus::Unknown | FollowUpStatus::Rejected => format!("{notice} [{id}] {text}"),
        _ => notice.to_owned(),
    }
}
