pub(crate) struct SessionHeader {
    agent: String,
}

impl SessionHeader {
    pub(crate) fn new(agent: String) -> Self {
        Self { agent }
    }

    /// Updates the header's agent text.
    pub(crate) fn set_agent(&mut self, agent: &str) {
        if self.agent != agent {
            self.agent = agent.to_string();
        }
    }
}
