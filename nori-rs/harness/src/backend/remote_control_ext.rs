//! Recognition of Nori's remote-control initialize extension.

use nori_protocol::acp::v1 as acp;
use serde::Deserialize;

const SUPPORTED_REMOTE_CONTROL_VERSION: i64 = 1;

#[derive(Deserialize)]
struct WireRemoteControl {
    version: i64,
    #[serde(rename = "activeSessionId")]
    active_session_id: String,
}

/// Returns the active outward session only for a well-formed, supported Nori
/// remote-control marker whose agent also advertises ACP `session/load`.
pub(super) fn automatic_session_id(
    meta: Option<&acp::Meta>,
    capabilities: &acp::AgentCapabilities,
) -> Option<acp::SessionId> {
    if !capabilities.load_session {
        return None;
    }
    let remote_control = meta?.get("nori")?.get("remoteControl")?;
    let wire: WireRemoteControl = serde_json::from_value(remote_control.clone()).ok()?;
    if wire.version != SUPPORTED_REMOTE_CONTROL_VERSION || wire.active_session_id.trim().is_empty()
    {
        return None;
    }
    Some(acp::SessionId::new(wire.active_session_id))
}
