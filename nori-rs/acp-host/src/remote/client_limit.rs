//! The background host's limit on concurrent WebSocket clients.

use std::sync::Arc;
use std::sync::atomic::AtomicI64;
use std::sync::atomic::Ordering;

/// One client's place under the host's client limit, given back when the
/// connection ends or its upgrade fails.
pub(super) struct ClientSlot(Arc<AtomicI64>);

impl ClientSlot {
    pub(super) fn claim(clients: &Arc<AtomicI64>, limit: i64) -> Option<Self> {
        clients
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                (count < limit).then_some(count + 1)
            })
            .ok()
            .map(|_| Self(clients.clone()))
    }
}

impl Drop for ClientSlot {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}
