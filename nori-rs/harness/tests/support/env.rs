pub(super) struct EnvGuard(&'static str, Option<String>);

impl EnvGuard {
    pub(super) fn set(name: &'static str, value: &str) -> Self {
        let previous = std::env::var(name).ok();
        // SAFETY: tests that mutate the mock-agent environment run serially.
        unsafe { std::env::set_var(name, value) };
        Self(name, previous)
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        // SAFETY: tests that mutate the mock-agent environment run serially.
        unsafe {
            match &self.1 {
                Some(previous) => std::env::set_var(self.0, previous),
                None => std::env::remove_var(self.0),
            }
        }
    }
}
