//! The sessions one TUI holds at once.
//!
//! One session is active: its widget is `App::chat_widget` and its
//! transcript is `App::transcript_cells`. Every other session is parked here
//! with its own widget and transcript, so its history, unsent input and
//! waiting requests survive while another session is on screen. The deck also
//! owns the navigation order, which never changes when a session's status
//! does.

use std::collections::HashMap;
use std::sync::Arc;

use crate::app_event::SessionGeneration;
use crate::history_cell::HistoryCell;

/// A session that is not on screen.
pub(crate) struct ParkedSession<W> {
    pub(crate) widget: W,
    pub(crate) transcript: Vec<Arc<dyn HistoryCell>>,
}

/// Which way a session move goes in the navigation order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Direction {
    Previous,
    Next,
}

pub(crate) struct SessionDeck<W> {
    order: Vec<SessionGeneration>,
    parked: HashMap<SessionGeneration, ParkedSession<W>>,
}

impl<W> Default for SessionDeck<W> {
    fn default() -> Self {
        Self {
            order: Vec::new(),
            parked: HashMap::new(),
        }
    }
}

impl<W> SessionDeck<W> {
    /// Every session in navigation order, the active one included.
    pub(crate) fn order(&self) -> &[SessionGeneration] {
        &self.order
    }

    /// Record a session at the end of the order. Adding a session that is
    /// already listed does nothing.
    pub(crate) fn add(&mut self, generation: SessionGeneration) {
        if !self.order.contains(&generation) {
            self.order.push(generation);
        }
    }

    /// Put `replacement` where `previous` was, keeping its place in the order.
    /// A parked `previous` is dropped from the deck and returned.
    pub(crate) fn replace(
        &mut self,
        previous: SessionGeneration,
        replacement: SessionGeneration,
    ) -> Option<ParkedSession<W>> {
        match self.order.iter().position(|g| *g == previous) {
            Some(at) => self.order[at] = replacement,
            None => self.add(replacement),
        }
        self.parked.remove(&previous)
    }

    /// Drop a session from the order, and its parked widget if any.
    pub(crate) fn remove(&mut self, generation: SessionGeneration) -> Option<ParkedSession<W>> {
        self.order.retain(|g| *g != generation);
        self.parked.remove(&generation)
    }

    pub(crate) fn park(&mut self, generation: SessionGeneration, session: ParkedSession<W>) {
        self.add(generation);
        self.parked.insert(generation, session);
    }

    pub(crate) fn unpark(&mut self, generation: SessionGeneration) -> Option<ParkedSession<W>> {
        self.parked.remove(&generation)
    }

    pub(crate) fn parked(&self, generation: SessionGeneration) -> Option<&ParkedSession<W>> {
        self.parked.get(&generation)
    }

    pub(crate) fn parked_mut(
        &mut self,
        generation: SessionGeneration,
    ) -> Option<&mut ParkedSession<W>> {
        self.parked.get_mut(&generation)
    }

    pub(crate) fn parked_sessions_mut(
        &mut self,
    ) -> impl Iterator<Item = (SessionGeneration, &mut ParkedSession<W>)> {
        self.parked.iter_mut().map(|(g, s)| (*g, s))
    }

    /// The first session after `from`, in `direction` and wrapping, for which
    /// `wanted` holds. `from` itself is never returned.
    pub(crate) fn next_where(
        &self,
        from: SessionGeneration,
        direction: Direction,
        wanted: impl Fn(SessionGeneration) -> bool,
    ) -> Option<SessionGeneration> {
        let len = self.order.len();
        let start = self.order.iter().position(|g| *g == from)?;
        (1..len)
            .map(|offset| match direction {
                Direction::Next => self.order[(start + offset) % len],
                Direction::Previous => self.order[(start + len - offset) % len],
            })
            .find(|g| wanted(*g))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn parked(name: &'static str) -> ParkedSession<&'static str> {
        ParkedSession {
            widget: name,
            transcript: Vec::new(),
        }
    }

    fn deck_of(generations: &[SessionGeneration]) -> SessionDeck<&'static str> {
        let mut deck = SessionDeck::default();
        for generation in generations {
            deck.add(*generation);
        }
        deck
    }

    #[test]
    fn moves_wrap_around_in_both_directions() {
        let deck = deck_of(&[1, 2, 3]);
        assert_eq!(
            [
                deck.next_where(1, Direction::Next, |_| true),
                deck.next_where(3, Direction::Next, |_| true),
                deck.next_where(1, Direction::Previous, |_| true),
                deck.next_where(2, Direction::Previous, |_| true),
            ],
            [Some(2), Some(1), Some(3), Some(1)]
        );
    }

    #[test]
    fn a_lone_session_has_nowhere_to_move() {
        let deck = deck_of(&[7]);
        assert_eq!(deck.next_where(7, Direction::Next, |_| true), None);
        assert_eq!(deck.next_where(7, Direction::Previous, |_| true), None);
    }

    #[test]
    fn next_where_skips_sessions_that_do_not_match_and_never_returns_the_start() {
        let deck = deck_of(&[1, 2, 3, 4]);
        let waiting = [1, 4];
        assert_eq!(
            deck.next_where(2, Direction::Next, |g| waiting.contains(&g)),
            Some(4)
        );
        assert_eq!(
            deck.next_where(4, Direction::Next, |g| waiting.contains(&g)),
            Some(1)
        );
        assert_eq!(deck.next_where(1, Direction::Next, |g| g == 1), None);
    }

    #[test]
    fn a_replacement_keeps_its_place_and_parking_does_not_reorder() {
        let mut deck = deck_of(&[1, 2, 3]);
        deck.park(2, parked("two"));
        let dropped = deck.replace(2, 9).map(|session| session.widget);
        deck.park(1, parked("one"));
        assert_eq!((deck.order(), dropped), (&[1, 9, 3][..], Some("two")));
    }

    #[test]
    fn a_removed_session_leaves_the_order_and_gives_back_its_widget() {
        let mut deck = deck_of(&[1, 3]);
        deck.park(2, parked("two"));
        let removed = deck.remove(2).map(|session| session.widget);
        assert_eq!((deck.order(), removed), (&[1, 3][..], Some("two")));
        assert_eq!(deck.next_where(1, Direction::Next, |_| true), Some(3));
    }

    #[test]
    fn parked_sessions_come_back_with_their_widget() {
        let mut deck = deck_of(&[1]);
        deck.park(2, parked("two"));
        assert_eq!(deck.order(), &[1, 2]);
        assert_eq!(deck.unpark(2).map(|session| session.widget), Some("two"));
        assert!(deck.unpark(2).is_none());
        assert_eq!(deck.order(), &[1, 2]);
    }
}
