//! How deep the alternate screen is nested.
//!
//! The full-screen view and the overlays drawn over it (the transcript, the
//! diff pager, a full-screen approval) all ask for the alternate screen. Only
//! the outermost request switches the terminal, so closing an overlay over
//! the full-screen view leaves the terminal on the alternate screen.

#[derive(Debug, Default)]
pub(crate) struct AltScreenNesting {
    depth: i32,
}

impl AltScreenNesting {
    /// Record a request for the alternate screen. Returns true when the
    /// terminal must switch to it now.
    pub(crate) fn enter(&mut self) -> bool {
        self.depth += 1;
        self.depth == 1
    }

    /// Record the end of a request. Returns true when the terminal must
    /// switch back to the normal screen now.
    pub(crate) fn leave(&mut self) -> bool {
        if self.depth == 0 {
            return false;
        }
        self.depth -= 1;
        self.depth == 0
    }

    /// Drop every request at once, as on exit. Returns true when the
    /// terminal must switch back to the normal screen now.
    pub(crate) fn leave_all(&mut self) -> bool {
        std::mem::take(&mut self.depth) > 0
    }
}

#[cfg(test)]
mod tests {
    use super::AltScreenNesting;
    use pretty_assertions::assert_eq;

    #[test]
    fn only_the_outermost_request_switches_the_terminal() {
        let mut nesting = AltScreenNesting::default();
        let switches = [
            nesting.enter(),
            nesting.enter(),
            nesting.leave(),
            nesting.leave(),
        ];
        assert_eq!(switches, [true, false, false, true]);
    }

    #[test]
    fn a_leave_without_an_enter_does_not_switch_or_unbalance_later_requests() {
        let mut nesting = AltScreenNesting::default();
        assert_eq!(nesting.leave(), false);
        assert_eq!([nesting.enter(), nesting.leave()], [true, true]);
    }

    #[test]
    fn leaving_all_drops_every_nested_request_at_once() {
        let mut nesting = AltScreenNesting::default();
        nesting.enter();
        nesting.enter();
        assert_eq!(nesting.leave_all(), true);
        assert_eq!(nesting.leave_all(), false, "already on the normal screen");
        assert_eq!(nesting.enter(), true, "the next request switches again");
    }
}
