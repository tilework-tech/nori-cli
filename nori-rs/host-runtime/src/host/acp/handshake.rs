//! The ACP `initialize` answer of a local host.

use serde_json::Value;
use serde_json::json;

/// The only ACP protocol version the host speaks.
const PROTOCOL_VERSION: i64 = 1;

/// Add the standard ACP fields to an `initialize` result that already
/// carries the managed-session keys, which the attach terminal still reads.
/// `agentInfo.version` is the `nori` version with the host contract as
/// semver build metadata (`0.31.0+contract.1.0.0`), the same pair
/// `host.json` publishes, so a client can check compatibility from standard
/// ACP fields alone.
pub(in crate::host) fn initialize(result: &mut Value, current_agent: &str) {
    // ACP v1 negotiation: a host that supports only version 1 answers 1,
    // and a client that cannot speak it disconnects.
    result["protocolVersion"] = json!(PROTOCOL_VERSION);
    result["agentInfo"] = json!({
        "name": "nori-host",
        "version": format!(
            "{}+contract.{}",
            env!("CARGO_PKG_VERSION"),
            nori_protocol::HOST_CONTRACT
        ),
    });
    // Prompt content and MCP transports beyond the ACP baseline depend on
    // each session's adapter, so the host promises only the baseline.
    result["agentCapabilities"] = json!({
        "loadSession": true,
        "_meta": {(super::picker::KEY): {"version": 1, "agentOption": super::config::agent_option(current_agent)}},
        "promptCapabilities": {"image": false, "audio": false, "embeddedContext": false},
        "mcpCapabilities": {"http": false, "sse": false},
        "sessionCapabilities": {"list": {}, "resume": {}, "close": {}, "delete": {}},
    });
    result["authMethods"] = json!([]);
}
