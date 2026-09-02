//! Authoritative view of the agent's ACP session configuration.
//!
//! The agent advertises its configuration as an ordered list of options, each
//! with an id, a human label, an optional semantic category, and a current
//! value. [`AgentConfigState`] keeps that list *as advertised*: the order is
//! the agent's, the labels are the agent's, and the raw option payload is
//! retained so `/config` can drive its pickers from the same state the status
//! card renders. Nothing here interprets agent-specific option ids.

use nori_protocol::acp::v1 as acp;

/// The resolved current value of one advertised option.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AgentConfigValue {
    /// A select option resolved to the label the agent advertises for the
    /// current value (falling back to the raw value id when the agent lists a
    /// current value it did not describe).
    Select(String),
    /// A boolean toggle. Rendered by presence rather than by an invented
    /// on/off label wherever space is tight.
    Boolean(bool),
}

impl AgentConfigValue {
    /// The value as displayed in prose contexts (history lines, `/status`
    /// detail rows). Boolean toggles have no agent-supplied label, so they get
    /// the only sensible neutral pair.
    pub(crate) fn display(&self) -> String {
        match self {
            Self::Select(label) => label.clone(),
            Self::Boolean(true) => "On".to_string(),
            Self::Boolean(false) => "Off".to_string(),
        }
    }
}

/// One advertised configuration option and its resolved current value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AgentConfigOption {
    /// The agent's option id (e.g. `model`). Only used for identity, never
    /// for presentation decisions.
    pub(crate) id: String,
    /// The agent-supplied label (e.g. `Model`).
    pub(crate) name: String,
    /// The agent-supplied semantic category, when it declares one.
    pub(crate) category: Option<acp::SessionConfigOptionCategory>,
    /// The resolved current value.
    pub(crate) value: AgentConfigValue,
    /// The advertised option, kept verbatim so `/config` can open its picker
    /// without a second round trip to the agent.
    pub(crate) raw: acp::SessionConfigOption,
}

impl AgentConfigOption {
    fn from_acp(option: &acp::SessionConfigOption) -> Option<Self> {
        let value = match &option.kind {
            acp::SessionConfigKind::Select(select) => {
                AgentConfigValue::Select(select_label(select))
            }
            acp::SessionConfigKind::Boolean(boolean) => {
                AgentConfigValue::Boolean(boolean.current_value)
            }
            // Unknown future option kinds carry no value this client can
            // resolve, so they are omitted rather than guessed at.
            _ => return None,
        };

        Some(Self {
            id: option.id.to_string(),
            name: option.name.clone(),
            category: option.category.clone(),
            value,
            raw: option.clone(),
        })
    }

    /// Whether this option is the agent's selector for `category`.
    pub(crate) fn is_category(&self, category: &acp::SessionConfigOptionCategory) -> bool {
        self.category.as_ref() == Some(category)
    }

    /// The current value rendered for display.
    pub(crate) fn display_value(&self) -> String {
        self.value.display()
    }
}

/// The agent's advertised session configuration, in advertised order.
///
/// Empty until the agent advertises its options; callers must treat the empty
/// state as "not known yet" and render nothing rather than a default.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct AgentConfigState {
    options: Vec<AgentConfigOption>,
}

impl AgentConfigState {
    /// Build the state from an advertised option list, preserving its order.
    pub(crate) fn from_options(config_options: &[acp::SessionConfigOption]) -> Self {
        Self {
            options: config_options
                .iter()
                .filter_map(AgentConfigOption::from_acp)
                .collect(),
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.options.is_empty()
    }

    /// Every advertised option, in the agent's order.
    pub(crate) fn options(&self) -> &[AgentConfigOption] {
        &self.options
    }

    /// The advertised options verbatim, for the `/config` pickers.
    pub(crate) fn raw_options(&self) -> Vec<acp::SessionConfigOption> {
        self.options
            .iter()
            .map(|option| option.raw.clone())
            .collect()
    }

    /// The option the agent declares for `category`, if any.
    pub(crate) fn option_for_category(
        &self,
        category: &acp::SessionConfigOptionCategory,
    ) -> Option<&AgentConfigOption> {
        self.options
            .iter()
            .find(|option| option.is_category(category))
    }

    /// The mode cycle derived from the agent's mode selector, used by the
    /// footer label and the mode hotkey.
    pub(crate) fn mode_config(&self) -> Option<super::session_config_mode::AcpModeConfig> {
        super::session_config_mode::acp_mode_config_from_options(&self.raw_options())
    }

    /// Options whose resolved value differs from `previous`, in advertised
    /// order. Options that appear for the first time are not reported as
    /// changes; the initial announcement covers them.
    pub(crate) fn changes_since(&self, previous: &Self) -> Vec<AgentConfigOption> {
        self.options
            .iter()
            .filter(|option| {
                previous
                    .options
                    .iter()
                    .find(|prior| prior.id == option.id)
                    .is_some_and(|prior| prior.value != option.value)
            })
            .cloned()
            .collect()
    }
}

/// Resolve a select option's current value to the label the agent advertises
/// for it, falling back to the raw value id.
fn select_label(select: &acp::SessionConfigSelect) -> String {
    let advertised = match &select.options {
        acp::SessionConfigSelectOptions::Ungrouped(options) => options
            .iter()
            .find(|value| value.value == select.current_value)
            .map(|value| value.name.clone()),
        acp::SessionConfigSelectOptions::Grouped(groups) => groups
            .iter()
            .flat_map(|group| group.options.iter())
            .find(|value| value.value == select.current_value)
            .map(|value| value.name.clone()),
        _ => None,
    };

    advertised.unwrap_or_else(|| select.current_value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    pub(crate) fn select(
        id: &str,
        name: &str,
        current: &str,
        values: &[(&str, &str)],
    ) -> acp::SessionConfigOption {
        acp::SessionConfigOption::select(
            id.to_string(),
            name.to_string(),
            current.to_string(),
            values
                .iter()
                .map(|(value, label)| {
                    acp::SessionConfigSelectOption::new(value.to_string(), label.to_string())
                })
                .collect::<Vec<_>>(),
        )
    }

    #[test]
    fn state_preserves_advertised_order_and_resolves_labels() {
        let state = AgentConfigState::from_options(&[
            select(
                "mode",
                "Mode",
                "plan",
                &[("plan", "Plan"), ("build", "Build")],
            ),
            select("model", "Model", "opus-5", &[("opus-5", "Opus 5")]),
        ]);

        let names: Vec<&str> = state
            .options()
            .iter()
            .map(|option| option.name.as_str())
            .collect();
        assert_eq!(names, vec!["Mode", "Model"]);
        assert_eq!(state.options()[0].display_value(), "Plan");
        assert_eq!(state.options()[1].display_value(), "Opus 5");
    }

    #[test]
    fn unresolvable_select_values_fall_back_to_the_value_id() {
        let state = AgentConfigState::from_options(&[select(
            "model",
            "Model",
            "opus-6",
            &[("opus-5", "Opus 5")],
        )]);

        assert_eq!(state.options()[0].display_value(), "opus-6");
    }

    #[test]
    fn boolean_options_are_retained_with_their_toggle_state() {
        let state = AgentConfigState::from_options(&[acp::SessionConfigOption::new(
            "fast-mode",
            "Fast mode",
            acp::SessionConfigKind::Boolean(acp::SessionConfigBoolean::new(true)),
        )]);

        assert_eq!(
            state.options()[0].value,
            AgentConfigValue::Boolean(true),
            "boolean toggles stay booleans so views can render by presence"
        );
        assert_eq!(state.options()[0].display_value(), "On");
    }

    #[test]
    fn changes_report_updated_values_but_not_new_options() {
        let previous = AgentConfigState::from_options(&[select(
            "model",
            "Model",
            "opus-5",
            &[("opus-5", "Opus 5"), ("sonnet-5", "Sonnet 5")],
        )]);
        let next = AgentConfigState::from_options(&[
            select(
                "model",
                "Model",
                "sonnet-5",
                &[("opus-5", "Opus 5"), ("sonnet-5", "Sonnet 5")],
            ),
            select("mode", "Mode", "plan", &[("plan", "Plan")]),
        ]);

        let changes = next.changes_since(&previous);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].name, "Model");
        assert_eq!(changes[0].display_value(), "Sonnet 5");
    }

    #[test]
    fn category_lookup_uses_the_advertised_category() {
        let state = AgentConfigState::from_options(&[
            select("mode", "Mode", "plan", &[("plan", "Plan")]),
            select("model", "Model", "opus-5", &[("opus-5", "Opus 5")])
                .category(acp::SessionConfigOptionCategory::Model),
        ]);

        assert_eq!(
            state
                .option_for_category(&acp::SessionConfigOptionCategory::Model)
                .map(|option| option.name.as_str()),
            Some("Model")
        );
        assert_eq!(
            state.option_for_category(&acp::SessionConfigOptionCategory::ThoughtLevel),
            None
        );
    }
}
