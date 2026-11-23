//! Adapter permission modes, for tests of how the host maps its own modes
//! onto an adapter's.
//!
//! `MOCK_AGENT_MODES` is a comma-separated list of mode ids. When set, the
//! mock advertises them as a `mode` config option (category `mode`) whose
//! first id is the starting mode, the way real adapters start in their own
//! default. `MOCK_AGENT_ASKING_MODES` lists the ids in which every turn asks
//! for permission before its tool call, as a strict adapter mode would.

use nori_protocol::acp::v1 as acp;

fn listed(variable: &str) -> Vec<String> {
    std::env::var(variable)
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(str::to_owned)
        .collect()
}

/// The mode a new or loaded session starts in.
pub(crate) fn starting_mode() -> Option<String> {
    listed("MOCK_AGENT_MODES").into_iter().next()
}

/// Whether `id` is one of the advertised modes.
pub(crate) fn is_advertised(id: &str) -> bool {
    listed("MOCK_AGENT_MODES").iter().any(|mode| mode == id)
}

pub(crate) fn option(current: &str) -> acp::SessionConfigOption {
    let choices = listed("MOCK_AGENT_MODES")
        .into_iter()
        .map(|id| acp::SessionConfigSelectOption::new(id.clone(), id))
        .collect::<Vec<_>>();
    acp::SessionConfigOption::select("mode", "Mode", current.to_owned(), choices)
        .category(acp::SessionConfigOptionCategory::Mode)
}

/// Whether a turn in `current` asks before its tool call.
pub(crate) fn asks(current: Option<&str>) -> bool {
    current.is_some_and(|current| {
        listed("MOCK_AGENT_ASKING_MODES")
            .iter()
            .any(|mode| mode == current)
    })
}
