use super::*;

impl HasLegacyEvent for ItemStartedEvent {
    fn as_legacy_events(&self, _: bool) -> Vec<EventMsg> {
        match &self.item {
            TurnItem::WebSearch(item) => vec![EventMsg::WebSearchBegin(WebSearchBeginEvent {
                call_id: item.id.clone(),
            })],
            _ => Vec::new(),
        }
    }
}

impl HasLegacyEvent for ItemCompletedEvent {
    fn as_legacy_events(&self, show_raw_agent_reasoning: bool) -> Vec<EventMsg> {
        self.item.as_legacy_events(show_raw_agent_reasoning)
    }
}

impl HasLegacyEvent for AgentMessageContentDeltaEvent {
    fn as_legacy_events(&self, _: bool) -> Vec<EventMsg> {
        vec![EventMsg::AgentMessageDelta(AgentMessageDeltaEvent {
            delta: self.delta.clone(),
        })]
    }
}

impl HasLegacyEvent for ReasoningContentDeltaEvent {
    fn as_legacy_events(&self, _: bool) -> Vec<EventMsg> {
        vec![EventMsg::AgentReasoningDelta(AgentReasoningDeltaEvent {
            delta: self.delta.clone(),
        })]
    }
}

impl HasLegacyEvent for ReasoningRawContentDeltaEvent {
    fn as_legacy_events(&self, _: bool) -> Vec<EventMsg> {
        vec![EventMsg::AgentReasoningRawContentDelta(
            AgentReasoningRawContentDeltaEvent {
                delta: self.delta.clone(),
            },
        )]
    }
}

impl HasLegacyEvent for EventMsg {
    fn as_legacy_events(&self, show_raw_agent_reasoning: bool) -> Vec<EventMsg> {
        match self {
            EventMsg::ItemCompleted(event) => event.as_legacy_events(show_raw_agent_reasoning),
            EventMsg::AgentMessageContentDelta(event) => {
                event.as_legacy_events(show_raw_agent_reasoning)
            }
            EventMsg::ReasoningContentDelta(event) => {
                event.as_legacy_events(show_raw_agent_reasoning)
            }
            EventMsg::ReasoningRawContentDelta(event) => {
                event.as_legacy_events(show_raw_agent_reasoning)
            }
            _ => Vec::new(),
        }
    }
}
