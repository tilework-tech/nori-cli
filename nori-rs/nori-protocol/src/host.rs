//! Nori's additions to standard ACP v1 on `nori host`'s `/sessions`: the
//! host's discovery file, its `nori/agent` `_meta` key, and the
//! `_nori/*` extension methods, plus
//! [`host_wire_schema`], which publishes them with the ACP v1 types the host
//! uses.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use schemars::generate::SchemaSettings;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Map;
use serde_json::Value;
use serde_json::json;

use crate::Delegation;
use crate::SessionEnvelope;
use crate::acp::v1 as acp;

/// `_meta` on a `session/list` row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ListedSessionMeta {
    /// The slug of the agent the session runs, when known.
    #[serde(rename = "nori/agent", skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    /// Whether the host is running the session now.
    #[serde(rename = "nori/session", skip_serializing_if = "Option::is_none")]
    pub session: Option<ListedSessionState>,
}

/// `_meta["nori/session"]` on a `session/list` row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ListedSessionState {
    /// `true` while the host runs the session's agent; `false` for a closed,
    /// failed, or finished session and for a stored transcript, which
    /// `session/load` or `session/resume` restarts.
    pub live: bool,
}

/// `_meta` a client may put on `session/new`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct NewSessionMeta {
    /// Start this agent instead of the folder's default, so the right
    /// adapter starts first.
    #[serde(rename = "nori/agent")]
    pub agent: Option<String>,
}

/// `$NORI_HOME/host/host.json`, the host's discovery file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct HostDiscovery {
    /// `ws://127.0.0.1:<port>/sessions`.
    pub endpoint: String,
    /// The bearer token every upgrade carries.
    pub token: String,
    pub pid: i64,
    pub instance_id: String,
    /// The `nori` version serving the host. Informational: compatibility
    /// is decided by `contract` alone.
    pub version: String,
    /// The semver of the host wire contract the host serves ([`HOST_CONTRACT`]
    /// of its build). See [`host_serves_contract`].
    pub contract: String,
    /// Who started the host: `NORI_HOST_STARTED_BY`, else `nori-cli`.
    pub started_by: String,
}

/// The host wire contract this build serves and speaks as a client: ACP v1
/// on `/sessions`, the `nori/*` `_meta` keys, the `_nori/*` extension
/// methods, and `host.json`. A change that a client of the previous contract
/// could not use bumps the major; an addition such clients can ignore bumps
/// the minor; a fix that changes no message bumps the patch.
pub const HOST_CONTRACT: &str = "1.0.0";

/// Whether a host advertising contract `host` serves a client built for
/// contract `client`: `host` must satisfy the caret requirement `^client`.
/// For `1.x` and above that is the same major and a host contract no older
/// than the client's; for `0.y` it is the same minor. Pre-release and
/// unparsable versions are incompatible.
pub fn host_serves_contract(host: &str, client: &str) -> bool {
    let Ok(host) = semver::Version::parse(host) else {
        return false;
    };
    let Ok(client) = semver::Version::parse(client) else {
        return false;
    };
    if !host.pre.is_empty() || !client.pre.is_empty() {
        return false;
    }
    semver::Comparator {
        op: semver::Op::Caret,
        major: client.major,
        minor: Some(client.minor),
        patch: Some(client.patch),
        pre: semver::Prerelease::EMPTY,
    }
    .matches(&host)
}

/// A connection's role on the host.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ControlRole {
    Controller,
    Watcher,
}

/// Result of `_nori/control/status` and `_nori/control/take`, and
/// `result.control` of `initialize`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ControlStatus {
    pub role: ControlRole,
    pub controller_connected: bool,
}

/// Params of the `_nori/control/changed` notification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ControlChanged {
    pub role: ControlRole,
}

/// Params of `_nori/session/subscribe`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SubscribeParams {
    /// A conversation ID, `"*"` for every session, or absent for the
    /// host's selected session.
    pub session_id: Option<String>,
    /// The last sequence number processed, by conversation ID.
    #[serde(default)]
    pub cursors: BTreeMap<String, i64>,
}

/// Result of `_nori/session/subscribe`. Envelopes follow as
/// `_nori/session/event` notifications whose params are a
/// [`SessionEnvelope`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SubscribeResult {
    pub subscribed: bool,
}

/// Result of `_nori/host/stop`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct HostStopResult {
    pub stopping: bool,
}

/// Params of `_nori/session/delegate`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct DelegateParams {
    /// The parent session; the selected session when absent.
    pub session_id: Option<String>,
    pub agent: String,
    pub task: String,
}

/// Params of `_nori/session/delegation/resolve`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ResolveDelegationParams {
    pub session_id: Option<String>,
    pub request_id: String,
    pub approved: bool,
}

/// JSON Schema (draft 2020-12) of the types exchanged with `nori host`: the
/// ACP v1 messages it serves and Nori's additions, with every type under
/// `$defs` and keys sorted so the output is stable.
pub fn host_wire_schema() -> Value {
    let mut generator = SchemaSettings::draft2020_12().into_generator();
    let roots = vec![
        // ACP v1 as the host serves it.
        generator.subschema_for::<acp::InitializeRequest>(),
        generator.subschema_for::<acp::InitializeResponse>(),
        generator.subschema_for::<acp::NewSessionRequest>(),
        generator.subschema_for::<acp::NewSessionResponse>(),
        generator.subschema_for::<acp::ListSessionsRequest>(),
        generator.subschema_for::<acp::ListSessionsResponse>(),
        generator.subschema_for::<acp::LoadSessionRequest>(),
        generator.subschema_for::<acp::LoadSessionResponse>(),
        generator.subschema_for::<acp::ResumeSessionRequest>(),
        generator.subschema_for::<acp::ResumeSessionResponse>(),
        generator.subschema_for::<acp::PromptRequest>(),
        generator.subschema_for::<acp::PromptResponse>(),
        generator.subschema_for::<acp::CancelNotification>(),
        generator.subschema_for::<acp::CloseSessionRequest>(),
        generator.subschema_for::<acp::CloseSessionResponse>(),
        generator.subschema_for::<acp::DeleteSessionRequest>(),
        generator.subschema_for::<acp::DeleteSessionResponse>(),
        generator.subschema_for::<acp::SetSessionModeRequest>(),
        generator.subschema_for::<acp::SetSessionModeResponse>(),
        generator.subschema_for::<acp::SetSessionConfigOptionRequest>(),
        generator.subschema_for::<acp::SetSessionConfigOptionResponse>(),
        generator.subschema_for::<acp::SessionNotification>(),
        generator.subschema_for::<acp::RequestPermissionRequest>(),
        generator.subschema_for::<acp::RequestPermissionResponse>(),
        // Nori's `_meta` keys and discovery file.
        generator.subschema_for::<ListedSessionMeta>(),
        generator.subschema_for::<NewSessionMeta>(),
        generator.subschema_for::<HostDiscovery>(),
        // The `_nori/*` extension.
        generator.subschema_for::<ControlStatus>(),
        generator.subschema_for::<ControlChanged>(),
        generator.subschema_for::<SubscribeParams>(),
        generator.subschema_for::<SubscribeResult>(),
        generator.subschema_for::<SessionEnvelope>(),
        generator.subschema_for::<HostStopResult>(),
        generator.subschema_for::<DelegateParams>(),
        generator.subschema_for::<ResolveDelegationParams>(),
        generator.subschema_for::<Delegation>(),
    ];
    let definitions = generator.take_definitions(true);
    sorted(json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "title": "NoriHostWire",
        "description": "Types exchanged with `nori host` on /sessions: the ACP v1 messages it serves, its `nori/agent` _meta key, its host.json discovery file, and the `_nori/*` extension. Generated from nori-protocol; do not edit.",
        "anyOf": roots,
        "$defs": definitions,
    }))
}

fn sorted(value: Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut entries: Vec<(String, Value)> = map.into_iter().collect();
            entries.sort_by(|(left, _), (right, _)| left.cmp(right));
            Value::Object(
                entries
                    .into_iter()
                    .map(|(key, value)| (key, sorted(value)))
                    .collect::<Map<String, Value>>(),
            )
        }
        Value::Array(items) => Value::Array(items.into_iter().map(sorted).collect()),
        other => other,
    }
}

#[cfg(test)]
mod contract_tests {
    use super::*;

    #[test]
    fn a_host_serves_clients_of_its_contract_major_up_to_its_own_minor() {
        let cases = [
            ("1.0.0", "1.0.0", true),
            ("1.0.3", "1.0.0", true),
            ("1.0.0", "1.0.3", false),
            ("1.4.0", "1.2.0", true),
            ("1.2.0", "1.4.0", false),
            ("2.0.0", "1.9.0", false),
            ("1.9.0", "2.0.0", false),
            ("0.3.1", "0.3.0", true),
            ("0.4.0", "0.3.0", false),
            ("1.0.0", "not-a-version", false),
            ("not-a-version", "1.0.0", false),
            ("1.0.0+build.7", "1.0.0", true),
            ("1.0.0-rc.2", "1.0.0-rc.1", false),
            ("1.0.0", "1.0.0-rc.1", false),
            ("1.1.0-rc.1", "1.0.0", false),
        ];
        for (host, client, expected) in cases {
            assert_eq!(
                host_serves_contract(host, client),
                expected,
                "host {host} serving a client built for {client}"
            );
        }
    }
}
