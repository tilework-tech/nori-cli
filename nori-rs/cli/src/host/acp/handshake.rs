//! The ACP `initialize` answer of a local host.

use serde_json::Value;
use serde_json::json;

/// The only ACP protocol version the host speaks.
const PROTOCOL_VERSION: i64 = 1;

/// The `nori host` contract major version.
pub(in crate::host) const CONTRACT: i64 = 1;

/// Add the standard ACP fields to an `initialize` result that already
/// carries the managed-session keys, which the attach terminal still reads.
/// The contract version is in `host.json`, not in the handshake.
pub(in crate::host) fn initialize(result: &mut Value) {
    // ACP v1 negotiation: a host that supports only version 1 answers 1,
    // and a client that cannot speak it disconnects.
    result["protocolVersion"] = json!(PROTOCOL_VERSION);
    result["agentInfo"] = json!({"name": "nori-host", "version": env!("CARGO_PKG_VERSION")});
    // Prompt content and MCP transports beyond the ACP baseline depend on
    // each session's adapter, so the host promises only the baseline.
    result["agentCapabilities"] = json!({
        "loadSession": true,
        "promptCapabilities": {"image": false, "audio": false, "embeddedContext": false},
        "mcpCapabilities": {"http": false, "sse": false},
        "sessionCapabilities": {"list": {}, "resume": {}, "close": {}, "delete": {}},
    });
    result["authMethods"] = json!([]);
}
