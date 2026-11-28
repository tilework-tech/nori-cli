//! The first-launch screen: the onigiri welcome above the setup form.

use nori_harness::AgentKind;

use super::setup_form::AgentChoice;
use super::setup_form::SetupChoices;
use super::setup_form::SetupForm;
use super::welcome_step::Welcome;
use super::welcome_step::WelcomeStep;

pub(crate) type NoriSetupStep = WelcomeStep<SetupForm>;

impl NoriSetupStep {
    pub(crate) fn setup(
        agents: Vec<AgentChoice>,
        configured: Option<AgentKind>,
        welcome: Welcome,
    ) -> Self {
        WelcomeStep::new(SetupForm::new(agents, configured), welcome, false)
    }

    pub(crate) fn choices(&self) -> Option<SetupChoices> {
        self.form.choices()
    }
}
