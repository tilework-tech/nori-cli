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
use crate::QueueSnapshot;
use crate::SessionEnvelope;
use crate::ThreadGoal;
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
    #[serde(rename = "nori/preparedAgent", skip_serializing_if = "Option::is_none")]
    pub prepared_agent: Option<PreparedAgentActivation>,
    /// Start this agent instead of the folder's default, so the right
    /// adapter starts first.
    #[serde(rename = "nori/agent")]
    pub agent: Option<String>,
}

/// Choose a registered downstream ACP server. Omitted fields use the host launch.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PrepareAgentParams {
    pub agent: Option<String>,
    pub cwd: Option<std::path::PathBuf>,
}

/// A pre-session connection scoped to the host client that opened it.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PrepareAgentResult {
    pub id: String,
    pub agent: String,
    pub cwd: std::path::PathBuf,
}

/// Scope a session/list request to a prepared catalog, or discard that selection.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PreparedAgentRef {
    pub id: String,
}

/// Use this selection exactly once to activate a host conversation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PreparedAgentActivation {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub saved_session_id: Option<String>,
}

/// The description a host gives an agent in its `agent` option when it found
/// the agent's launch command; any other description says why it cannot
/// start.
pub const AGENT_INSTALLED: &str = "Installed";

/// The host's prepared-agent capability on initialize.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PreparedAgentCapability {
    pub version: i32,
    pub agent_option: acp::SessionConfigOption,
}

/// Metadata accompanying the prepared adapter's saved-session rows.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PreparedAgentCatalogInfo {
    pub agent_capabilities: acp::AgentCapabilities,
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
pub const HOST_CONTRACT: &str = "1.10.0";

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

/// What `_nori/session/find_paths` looks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PathKind {
    /// Files under the root whose paths fuzzy-match the query.
    Files,
    /// Folders completing the folder the query names.
    Folders,
}

/// Params of `_nori/session/find_paths`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FindPathsParams {
    /// Search this session's folder; the host's own folder when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    /// For files, the fuzzy pattern. For folders, the folder as typed:
    /// relative to the root, absolute, or under `~`.
    pub query: String,
    pub kind: PathKind,
    /// At most this many matches, clamped to 1..=50. Defaults to 20.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<i32>,
}

/// One `_nori/session/find_paths` match.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PathMatch {
    /// For files, the path relative to the root. For folders, the query
    /// completed to one folder, in the form it was typed, ending in `/`.
    pub path: String,
    /// Character positions in `path` that matched the query, for files.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub indices: Option<Vec<i32>>,
}

/// Result of `_nori/session/find_paths`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct FindPathsResult {
    /// The absolute folder on the host that was searched.
    pub root: std::path::PathBuf,
    pub matches: Vec<PathMatch>,
}

/// Params of `_nori/session/queue/resume` and `_nori/session/queue/discard`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QueueParams {
    pub session_id: String,
}

/// Params of `_nori/session/queue/take`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QueueTakeParams {
    pub session_id: String,
    /// A [`crate::QueueEntry`] ID from the session's latest queue snapshot.
    pub entry_id: String,
    /// Chosen by the caller for this take. Asking again with the same key
    /// returns the same answer, so a reply lost to a disconnect can be
    /// fetched again; a different key gets null once the prompt is taken.
    pub request_key: String,
}

/// Result of `_nori/session/queue/take`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct QueueTakeResult {
    /// The text the user queued, returned to exactly one caller. Null when
    /// the prompt already started, was already taken, or cannot return to a
    /// composer without losing content.
    pub text: Option<String>,
}

/// Result of `_nori/session/queue/discard`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct QueueDiscardResult {
    /// How many queued prompts were dropped before being sent.
    pub discarded: i32,
}

/// Params of `_nori/host/history/add`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HistoryAddParams {
    /// The session the prompt was sent to.
    pub session_id: String,
    pub text: String,
}

/// Params of `_nori/host/history/entry`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HistoryEntryParams {
    /// [`HistoryMetadata::log_id`] of the history the client counted in.
    pub log_id: i64,
    /// Zero-based, oldest first.
    pub offset: i64,
}

/// One prompt in the host's history.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct HistoryRecord {
    pub session_id: String,
    /// Seconds since the Unix epoch.
    pub ts: i64,
    pub text: String,
}

/// Result of `_nori/host/history/entry`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct HistoryEntryResult {
    /// Null when `offset` is past the end, or `logId` names a history the
    /// host no longer keeps.
    pub entry: Option<HistoryRecord>,
}

/// Params of `_nori/host/history/search`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HistorySearchParams {
    /// At most this many distinct prompts, newest first.
    pub max_results: i32,
}

/// Result of `_nori/host/history/search`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct HistorySearchResult {
    /// Distinct prompts, newest first; the client filters them as the user
    /// types.
    pub entries: Vec<HistoryRecord>,
}

/// Result of `_nori/host/history/metadata`: what a client counts in when it
/// recalls prompts with `_nori/host/history/entry`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct HistoryMetadata {
    /// Identifies the history file; it changes when the file is replaced.
    pub log_id: i64,
    pub entry_count: i64,
}

/// Params of `_nori/host/prompts`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PromptsListParams {
    /// The session the list is for. The list is the host's either way.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
}

/// How a custom prompt is used.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HostPromptKind {
    /// `content` is sent as the prompt once the client fills in its
    /// arguments.
    Markdown,
    /// Run on the host with `_nori/session/prompts/run`; its output is sent
    /// as the prompt.
    Script { interpreter: String },
}

/// One custom prompt from the host's `$NORI_HOME/commands`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct HostPrompt {
    /// Invoked as `/prompts:<name>`.
    pub name: String,
    pub description: Option<String>,
    pub argument_hint: Option<String>,
    pub kind: HostPromptKind,
    /// The prompt's body without its frontmatter; empty for a script.
    pub content: String,
}

/// Result of `_nori/host/prompts`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PromptsListResult {
    /// Sorted by name.
    pub prompts: Vec<HostPrompt>,
}

/// Params of `_nori/session/prompts/run`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PromptRunParams {
    /// The session whose folder the script runs in.
    pub session_id: String,
    /// A script prompt's name from `_nori/host/prompts`.
    pub name: String,
    /// Positional arguments passed to the script.
    pub args: Vec<String>,
}

/// Result of `_nori/session/prompts/run`: a script that exited zero.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PromptRunResult {
    pub stdout: String,
}

/// Params of `_nori/session/goal/get`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GoalGetParams {
    pub session_id: String,
}

/// One change to a session's goal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GoalOp {
    /// Replace the goal with an active one.
    Set {
        objective: String,
    },
    Pause,
    Resume,
    Clear,
}

/// Params of `_nori/session/goal/set`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GoalSetParams {
    pub session_id: String,
    pub op: GoalOp,
}

/// Result of `_nori/session/goal/get` and `_nori/session/goal/set`: the
/// session's goal afterwards, null when it has none.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct GoalResult {
    pub goal: Option<ThreadGoal>,
}

/// Params of `_nori/session/compact`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CompactParams {
    pub session_id: String,
}

/// The error code of `_nori/session/shell` from a client the host cannot
/// prove runs on its own machine. The host runs nothing for it.
pub const REMOTE_SHELL_UNSUPPORTED: i32 = -32018;

/// The message that accompanies [`REMOTE_SHELL_UNSUPPORTED`].
pub const REMOTE_SHELL_UNSUPPORTED_MESSAGE: &str = "Remote shell commands are not yet supported.";

/// The upgrade header with which a client on the host's machine presents
/// the host's private local token, `host/local-token`.
pub const LOCAL_TOKEN_HEADER: &str = "x-nori-local-token";

/// Params of `_nori/session/shell`: a `!` command for the host to run in
/// that session's folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ShellParams {
    pub session_id: String,
    pub command: String,
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
        generator.subschema_for::<PrepareAgentParams>(),
        generator.subschema_for::<PrepareAgentResult>(),
        generator.subschema_for::<PreparedAgentRef>(),
        generator.subschema_for::<PreparedAgentCapability>(),
        generator.subschema_for::<PreparedAgentCatalogInfo>(),
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
        generator.subschema_for::<FindPathsParams>(),
        generator.subschema_for::<FindPathsResult>(),
        generator.subschema_for::<QueueParams>(),
        generator.subschema_for::<QueueTakeParams>(),
        generator.subschema_for::<QueueTakeResult>(),
        generator.subschema_for::<QueueDiscardResult>(),
        generator.subschema_for::<QueueSnapshot>(),
        generator.subschema_for::<HistoryAddParams>(),
        generator.subschema_for::<HistoryEntryParams>(),
        generator.subschema_for::<HistoryEntryResult>(),
        generator.subschema_for::<HistorySearchParams>(),
        generator.subschema_for::<HistorySearchResult>(),
        generator.subschema_for::<HistoryMetadata>(),
        generator.subschema_for::<PromptsListParams>(),
        generator.subschema_for::<PromptsListResult>(),
        generator.subschema_for::<PromptRunParams>(),
        generator.subschema_for::<PromptRunResult>(),
        generator.subschema_for::<GoalGetParams>(),
        generator.subschema_for::<GoalSetParams>(),
        generator.subschema_for::<GoalResult>(),
        generator.subschema_for::<crate::ContextParams>(),
        generator.subschema_for::<crate::ContextReport>(),
        generator.subschema_for::<CompactParams>(),
        generator.subschema_for::<ShellParams>(),
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
            ("1.5.0", "1.4.0", true),
            ("1.4.0", "1.5.0", false),
            ("1.6.0", "1.5.0", true),
            ("1.5.0", "1.6.0", false),
            ("1.7.0", "1.6.0", true),
            ("1.6.0", "1.7.0", false),
            ("1.8.0", "1.7.0", true),
            ("1.7.0", "1.8.0", false),
            ("1.9.0", "1.8.0", true),
            ("1.8.0", "1.9.0", false),
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
