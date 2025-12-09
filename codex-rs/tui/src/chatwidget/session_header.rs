pub(crate) struct SessionHeader {
    model: String,
    pending_agent: Option<String>,
}

impl SessionHeader {
    pub(crate) fn new(model: String) -> Self {
        Self {
            model,
            pending_agent: None,
        }
    }

    /// Updates the header's model text.
    pub(crate) fn set_model(&mut self, model: &str) {
        if self.model != model {
            self.model = model.to_string();
        }
    }

    /// Set the pending agent for display in the header.
    pub(crate) fn set_pending_agent(&mut self, agent: Option<String>) {
        self.pending_agent = agent;
    }

}
