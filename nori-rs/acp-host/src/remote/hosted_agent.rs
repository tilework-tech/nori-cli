//! Downward-facing control interface between the remote ACP transport and
//! the process hosting a harness session.
//!
//! `nori-acp-host` owns the WebSocket server and the outward ACP Agent
//! surface; `nori-harness` implements this interface over `HarnessHandle`.
//! Keeping the interface here preserves the crate layering: the harness
//! depends on this crate, never the reverse
//! (see `docs/specs/remote-acp-transport.md` §3).

use std::future::Future;

use nori_protocol::SessionEvent;
use nori_protocol::acp::v1 as acp;

/// Replay batch returned by [`HostedAgent::load_session`].
///
/// The transport delivers every notification as a `session/update` before it
/// answers the `session/load` request, so a reconnecting client sees the
/// recorded history ahead of the response.
#[derive(Debug)]
pub struct LoadedSession {
    /// Recorded `session/update` notifications, already carrying the outward
    /// (stable) session id.
    pub replay: Vec<acp::SessionNotification>,
}

/// Bounded stream of outward-facing session events for one remote consumer.
///
/// Implementations send only these event shapes, in post-harness stream
/// order:
///
/// - `SessionEvent::Acp(AcpEvent::Notification(..))` — forwarded verbatim as
///   agent-to-client notifications (session ids already rewritten outward);
/// - `SessionEvent::Acp(AcpEvent::Request { .. })` — delegated agent-to-client
///   requests the remote controller must answer via [`HostedAgent::respond`];
/// - `SessionEvent::Acp(AcpEvent::Response { .. })` — the outcome of a
///   harness request this controller issued (see [`HostedAgent::prompt`]);
///   delivering it in stream order keeps a turn's updates ahead of its final
///   response;
/// - `SessionEvent::Nori(NoriEvent::RequestFailed(..))` — failure of a
///   harness request this controller issued;
/// - `SessionEvent::Nori(NoriEvent::SessionEnded(..))` — the hosted session is
///   gone; the transport closes the connection.
///
/// The channel closing (without `SessionEnded`) means the host dropped this
/// consumer — for example its bounded queue overflowed or a newer connection
/// replaced it — and the transport must also close the connection.
pub type HostedEventReceiver = tokio::sync::mpsc::Receiver<SessionEvent>;

/// One registered remote consumer.
///
/// The `id` scopes [`HostedAgent::detach`]: a connection that was already
/// replaced by a newer subscription detaches with a stale id, which the host
/// ignores instead of tearing down its successor.
#[derive(Debug)]
pub struct HostedSubscription {
    /// Monotonic identity of this subscription.
    pub id: i64,
    /// Bounded event stream for this consumer.
    pub events: HostedEventReceiver,
}

/// Control surface the remote ACP Agent drives instead of talking to the
/// downstream `AcpConnection` directly.
///
/// All session ids crossing this interface are the outward stable ids (Nori
/// conversation ids); downstream agent session swaps stay invisible here.
pub trait HostedAgent: Send + Sync + 'static {
    /// Sessions this host currently exposes.
    fn list_sessions(
        &self,
    ) -> impl Future<Output = Result<Vec<acp::SessionInfo>, acp::Error>> + Send;

    /// Attach to a session and return its recorded history for replay.
    fn load_session(
        &self,
        session_id: &acp::SessionId,
    ) -> impl Future<Output = Result<LoadedSession, acp::Error>> + Send;

    /// Attach to a session without replaying history.
    fn resume_session(
        &self,
        session_id: &acp::SessionId,
    ) -> impl Future<Output = Result<(), acp::Error>> + Send;

    /// Submit a prompt turn, returning the harness-issued request id. The
    /// turn's final outcome arrives later on the event stream as an
    /// `AcpEvent::Response` (or `NoriEvent::RequestFailed`) carrying the same
    /// id; the transport answers the remote client's own request with it.
    fn prompt(
        &self,
        session_id: &acp::SessionId,
        prompt: Vec<acp::ContentBlock>,
    ) -> impl Future<Output = Result<acp::RequestId, acp::Error>> + Send;

    /// Cancel the active turn of the given session.
    fn cancel(
        &self,
        session_id: &acp::SessionId,
    ) -> impl Future<Output = Result<(), acp::Error>> + Send;

    /// Close the given session; terminal for the hosted harness session.
    fn close_session(
        &self,
        session_id: &acp::SessionId,
    ) -> impl Future<Output = Result<(), acp::Error>> + Send;

    /// Answer a delegated agent-to-client request previously surfaced through
    /// the event stream.
    fn respond(
        &self,
        request_id: acp::RequestId,
        response: Result<acp::ClientResponse, acp::Error>,
    ) -> impl Future<Output = Result<(), acp::Error>> + Send;

    /// Register the calling connection as the single remote consumer,
    /// replacing any previous one (last connect wins). Replacing a consumer
    /// cancels the replaced controller's unanswered delegated requests.
    fn subscribe(&self) -> impl Future<Output = HostedSubscription> + Send;

    /// The remote controller behind `subscription_id` detached; cancel its
    /// unanswered delegated requests so they cannot wedge the agent. A stale
    /// id (already replaced by a newer subscription) is ignored.
    fn detach(&self, subscription_id: i64) -> impl Future<Output = ()> + Send;
}
