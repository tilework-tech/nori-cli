//! One refused `/close` is reported twice: by the adapter's error response on
//! the session stream and by the close command's own error, in either order.
//! The ledger lets a terminal show the refusal once, using the command's
//! error, which carries the adapter's text.

/// Close state of one session, as seen by one terminal.
#[derive(Debug, Default)]
pub(crate) struct CloseRefusal {
    state: Option<State>,
}

#[derive(Debug)]
enum State {
    /// The close is in flight. Adapter errors seen now wait for the
    /// command's verdict.
    Pending { held: Vec<AdapterError> },
    /// The command's error was shown first; the adapter's matching error is
    /// still to come.
    Reported { command: String },
}

#[derive(Debug)]
struct AdapterError {
    raw: String,
    display: String,
}

fn describes(command: &str, raw: &str) -> bool {
    !raw.is_empty() && command.contains(raw)
}

impl CloseRefusal {
    /// A close was sent.
    pub(crate) fn start(&mut self) {
        self.state = Some(State::Pending { held: Vec::new() });
    }

    /// Whether an adapter error that no prompt owns is held or dropped
    /// rather than shown now. `raw` is the adapter's message and `display`
    /// the text to show if it is not a duplicate of the refusal. A command
    /// error covers only the next adapter error, if it describes it.
    pub(crate) fn absorb(&mut self, raw: &str, display: &str) -> bool {
        match &mut self.state {
            Some(State::Pending { held }) => {
                held.push(AdapterError {
                    raw: raw.to_owned(),
                    display: display.to_owned(),
                });
                true
            }
            Some(State::Reported { command }) => {
                let duplicate = describes(command, raw);
                self.state = None;
                duplicate
            }
            None => false,
        }
    }

    /// The command reported that the close failed with `message`. Returns
    /// the held adapter errors that `message` does not already describe;
    /// show them before the refusal.
    pub(crate) fn refused(&mut self, message: &str) -> Vec<String> {
        let Some(State::Pending { held }) = self.state.take() else {
            return Vec::new();
        };
        let described = held.iter().any(|error| describes(message, &error.raw));
        if !described {
            self.state = Some(State::Reported {
                command: message.to_owned(),
            });
        }
        held.into_iter()
            .filter(|error| !describes(message, &error.raw))
            .map(|error| error.display)
            .collect()
    }

    /// Something other than an adapter error arrived, so a refusal still
    /// waiting for its stream copy will not get one: later errors are new.
    pub(crate) fn expire_report(&mut self) {
        if matches!(self.state, Some(State::Reported { .. })) {
            self.state = None;
        }
    }

    /// The close ended without a refusal (the adapter released the session,
    /// or the connection dropped). Returns the held errors, which belong to
    /// something else.
    pub(crate) fn released(&mut self) -> Vec<String> {
        match self.state.take() {
            Some(State::Pending { held }) => held.into_iter().map(|error| error.display).collect(),
            Some(State::Reported { .. }) | None => Vec::new(),
        }
    }
}
