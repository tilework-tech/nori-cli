//! The ACP agent servers a host can start, from the `agent` option its
//! `initialize` reply carries under `nori/preparedAgent`. The host checks
//! each server's launch command on its own machine, so a terminal attached
//! from elsewhere shows what that host can start, not what this one can.

use serde_json::Value;

/// One ACP agent server the host listed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HostAgent {
    pub slug: String,
    pub name: String,
    /// What the host said about starting it: "Installed", or why it cannot.
    pub note: String,
}

impl HostAgent {
    /// The host found the server's launch command.
    pub(crate) fn can_start(&self) -> bool {
        self.note == nori_protocol::AGENT_INSTALLED
    }
}

/// The agents an `initialize` result lists; `None` from a host too old to
/// list them.
pub(crate) fn host_agents(initialize: &Value) -> Option<Vec<HostAgent>> {
    let option = initialize
        .pointer("/agentCapabilities/_meta")?
        .get(super::PREPARED_AGENT)?
        .get("agentOption")?;
    let choices = option.get("options")?.as_array()?;
    Some(
        choices
            .iter()
            .filter_map(|choice| {
                let slug = choice.get("value")?.as_str()?.to_owned();
                Some(HostAgent {
                    name: choice
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or(&slug)
                        .to_owned(),
                    note: choice
                        .get("description")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                    slug,
                })
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    #[test]
    fn a_host_lists_each_agent_with_whether_it_can_start() {
        let initialize = json!({"agentCapabilities": {"_meta": {"nori/preparedAgent": {
            "version": 1,
            "agentOption": {"id": "agent", "type": "select", "currentValue": "amber", "options": [
                {"value": "amber", "name": "Amber", "description": "Installed"},
                {"value": "cedar", "name": "Cedar", "description": "Adapter not installed. Install cedar."},
            ]},
        }}}});
        let agents = host_agents(&initialize).expect("the host lists agents");
        assert_eq!(
            agents
                .iter()
                .map(|agent| (agent.name.as_str(), agent.can_start()))
                .collect::<Vec<_>>(),
            [("Amber", true), ("Cedar", false)]
        );
        assert_eq!(agents[1].note, "Adapter not installed. Install cedar.");
    }

    #[test]
    fn an_older_host_lists_no_agents() {
        assert_eq!(
            host_agents(&json!({"capabilities": {"nori_sessions": {}}})),
            None
        );
    }
}
