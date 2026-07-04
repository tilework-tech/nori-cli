use nori_harness as acp;

#[derive(Debug, Clone, PartialEq, Eq)]
struct AcpModeConfigValue {
    value: String,
    label: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AcpModeConfig {
    pub(crate) config_id: String,
    pub(crate) current_label: String,
    pub(crate) next_value: String,
    pub(crate) next_label: String,
    values: Vec<AcpModeConfigValue>,
    current_idx: usize,
}

impl AcpModeConfig {
    #[cfg(test)]
    pub(crate) fn from_values(
        config_id: String,
        current_value: String,
        values: Vec<(String, String)>,
    ) -> Option<Self> {
        Self::new(
            config_id,
            current_value,
            values
                .into_iter()
                .map(|(value, label)| AcpModeConfigValue { value, label })
                .collect(),
        )
    }

    fn new(
        config_id: String,
        current_value: String,
        values: Vec<AcpModeConfigValue>,
    ) -> Option<Self> {
        if values.is_empty() {
            return None;
        }

        let current_idx = values
            .iter()
            .position(|value| value.value == current_value)
            .unwrap_or(0);
        Some(Self::from_current_idx(config_id, values, current_idx))
    }

    fn from_current_idx(
        config_id: String,
        values: Vec<AcpModeConfigValue>,
        current_idx: usize,
    ) -> Self {
        let current = &values[current_idx];
        let next = &values[(current_idx + 1) % values.len()];

        Self {
            config_id,
            current_label: current.label.clone(),
            next_value: next.value.clone(),
            next_label: next.label.clone(),
            values,
            current_idx,
        }
    }

    pub(crate) fn advanced(&self) -> Self {
        Self::from_current_idx(
            self.config_id.clone(),
            self.values.clone(),
            (self.current_idx + 1) % self.values.len(),
        )
    }

    pub(crate) fn label_for_value(&self, value: &str) -> Option<&str> {
        self.values
            .iter()
            .find(|entry| entry.value == value)
            .map(|entry| entry.label.as_str())
    }

    pub(crate) fn current_value(&self) -> &str {
        &self.values[self.current_idx].value
    }
}

pub(crate) fn acp_mode_config_from_options(
    config_options: &[acp::SessionConfigOption],
) -> Option<AcpModeConfig> {
    config_options
        .iter()
        .filter(|option| {
            option.category == Some(acp::SessionConfigOptionCategory::Mode)
                || option.id.to_string() == "mode"
        })
        .find_map(mode_config_from_option)
}

fn mode_config_from_option(option: &acp::SessionConfigOption) -> Option<AcpModeConfig> {
    let acp::SessionConfigKind::Select(select) = &option.kind else {
        return None;
    };

    AcpModeConfig::new(
        option.id.to_string(),
        select.current_value.to_string(),
        select_values(&select.options)
            .into_iter()
            .map(|value| AcpModeConfigValue {
                value: value.value.to_string(),
                label: value.name.clone(),
            })
            .collect(),
    )
}

fn select_values(
    options: &acp::SessionConfigSelectOptions,
) -> Vec<&acp::SessionConfigSelectOption> {
    match options {
        acp::SessionConfigSelectOptions::Ungrouped(options) => options.iter().collect(),
        acp::SessionConfigSelectOptions::Grouped(groups) => groups
            .iter()
            .flat_map(|group| group.options.iter())
            .collect(),
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn grouped_mode_option() -> acp::SessionConfigOption {
        acp::SessionConfigOption::select(
            "mode",
            "Mode",
            "plan",
            vec![
                acp::SessionConfigSelectGroup::new(
                    "safe",
                    "Safe",
                    vec![acp::SessionConfigSelectOption::new("ask", "Ask")],
                ),
                acp::SessionConfigSelectGroup::new(
                    "active",
                    "Active",
                    vec![
                        acp::SessionConfigSelectOption::new("plan", "Plan"),
                        acp::SessionConfigSelectOption::new("build", "Build"),
                    ],
                ),
            ],
        )
        .category(acp::SessionConfigOptionCategory::Mode)
    }

    #[test]
    fn mode_config_uses_current_label_and_next_value_from_grouped_options() {
        assert_eq!(
            acp_mode_config_from_options(&[grouped_mode_option()]),
            AcpModeConfig::new(
                "mode".to_string(),
                "plan".to_string(),
                vec![
                    AcpModeConfigValue {
                        value: "ask".to_string(),
                        label: "Ask".to_string()
                    },
                    AcpModeConfigValue {
                        value: "plan".to_string(),
                        label: "Plan".to_string()
                    },
                    AcpModeConfigValue {
                        value: "build".to_string(),
                        label: "Build".to_string()
                    },
                ],
            )
        );
    }

    #[test]
    fn mode_config_wraps_to_first_option() {
        let option = acp::SessionConfigOption::select(
            "mode",
            "Mode",
            "build",
            vec![
                acp::SessionConfigSelectOption::new("plan", "Plan"),
                acp::SessionConfigSelectOption::new("build", "Build"),
            ],
        )
        .category(acp::SessionConfigOptionCategory::Mode);

        assert_eq!(
            acp_mode_config_from_options(&[option]),
            AcpModeConfig::new(
                "mode".to_string(),
                "build".to_string(),
                vec![
                    AcpModeConfigValue {
                        value: "plan".to_string(),
                        label: "Plan".to_string(),
                    },
                    AcpModeConfigValue {
                        value: "build".to_string(),
                        label: "Build".to_string(),
                    },
                ],
            )
        );
    }

    #[test]
    fn mode_config_skips_unusable_matching_options() {
        let empty_mode = acp::SessionConfigOption::select(
            "mode",
            "Mode",
            "plan",
            Vec::<acp::SessionConfigSelectOption>::new(),
        )
        .category(acp::SessionConfigOptionCategory::Mode);

        assert_eq!(
            acp_mode_config_from_options(&[empty_mode, grouped_mode_option()]),
            AcpModeConfig::new(
                "mode".to_string(),
                "plan".to_string(),
                vec![
                    AcpModeConfigValue {
                        value: "ask".to_string(),
                        label: "Ask".to_string(),
                    },
                    AcpModeConfigValue {
                        value: "plan".to_string(),
                        label: "Plan".to_string(),
                    },
                    AcpModeConfigValue {
                        value: "build".to_string(),
                        label: "Build".to_string(),
                    },
                ],
            )
        );
    }
}
