use std::collections::HashMap;
use std::path::PathBuf;

use sacp::schema as acp;
use serde::Deserialize;
use serde::Serialize;

pub mod session_runtime;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event_type", rename_all = "snake_case")]
pub enum ClientEvent {
    ToolSnapshot(ToolSnapshot),
    ApprovalRequest(ApprovalRequest),
    MessageDelta(MessageDelta),
    PlanSnapshot(PlanSnapshot),
    TurnLifecycle(TurnLifecycle),
    ReplayEntry(ReplayEntry),
    AgentCommandsUpdate(AgentCommandsUpdate),
    Warning(WarningInfo),
}

/// A warning emitted by the session runtime (e.g. out-of-phase content).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct WarningInfo {
    pub message: String,
}

/// A set of commands advertised by the ACP agent.
/// Each update fully replaces the previous set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct AgentCommandsUpdate {
    pub commands: Vec<AgentCommandInfo>,
}

/// Information about a single agent-provided slash command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct AgentCommandInfo {
    pub name: String,
    pub description: String,
    pub input_hint: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnLifecycle {
    Started,
    Completed {
        last_agent_message: Option<String>,
    },
    Aborted {
        reason: TurnAbortReason,
    },
    ContextCompacted {
        summary: Option<String>,
    },
    /// `session/cancel` has been sent but the prompt response has not yet
    /// arrived. The turn is still active per ACP protocol.
    Cancelling,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnAbortReason {
    Interrupted,
    Replaced,
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "replay_type", rename_all = "snake_case")]
pub enum ReplayEntry {
    UserMessage { text: String },
    AssistantMessage { text: String },
    ReasoningMessage { text: String },
    PlanSnapshot { snapshot: PlanSnapshot },
    ToolSnapshot { snapshot: Box<ToolSnapshot> },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct MessageDelta {
    pub stream: MessageStream,
    pub delta: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageStream {
    Answer,
    Reasoning,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct PlanSnapshot {
    pub entries: Vec<PlanEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct PlanEntry {
    pub step: String,
    pub status: PlanStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanStatus {
    Pending,
    InProgress,
    Completed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ToolSnapshot {
    pub call_id: String,
    pub title: String,
    pub kind: ToolKind,
    pub phase: ToolPhase,
    pub locations: Vec<ToolLocation>,
    pub invocation: Option<Invocation>,
    pub artifacts: Vec<Artifact>,
    pub raw_input: Option<serde_json::Value>,
    pub raw_output: Option<serde_json::Value>,
    /// The request that created this tool call. Used for cancellation
    /// and request-local rendering.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_request_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ApprovalRequest {
    pub call_id: String,
    pub title: String,
    pub kind: ToolKind,
    pub options: Vec<ApprovalOption>,
    pub subject: ApprovalSubject,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "subject_type", rename_all = "snake_case")]
pub enum ApprovalSubject {
    ToolSnapshot(ToolSnapshot),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ApprovalOption {
    pub option_id: String,
    pub name: String,
    pub kind: ApprovalOptionKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalOptionKind {
    AllowAlways,
    AllowOnce,
    RejectOnce,
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolKind {
    Read,
    Search,
    Execute,
    Create,
    Edit,
    Delete,
    Move,
    Fetch,
    Think,
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolPhase {
    Pending,
    PendingApproval,
    InProgress,
    Completed,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ToolLocation {
    pub path: PathBuf,
    pub line: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "invocation_type", rename_all = "snake_case")]
pub enum Invocation {
    FileChanges {
        changes: Vec<FileChange>,
    },
    FileOperations {
        operations: Vec<FileOperation>,
    },
    Command {
        command: String,
    },
    Read {
        path: PathBuf,
    },
    Search {
        query: Option<String>,
        path: Option<PathBuf>,
    },
    ListFiles {
        path: Option<PathBuf>,
    },
    Tool {
        tool_name: String,
        input: Option<serde_json::Value>,
    },
    RawJson(serde_json::Value),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "artifact_type", rename_all = "snake_case")]
pub enum Artifact {
    Diff(FileChange),
    Text { text: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct FileChange {
    pub path: PathBuf,
    pub old_text: Option<String>,
    pub new_text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "operation_type", rename_all = "snake_case")]
pub enum FileOperation {
    Create {
        path: PathBuf,
        new_text: String,
    },
    Update {
        path: PathBuf,
        old_text: String,
        new_text: String,
    },
    Delete {
        path: PathBuf,
        old_text: Option<String>,
    },
    Move {
        from_path: PathBuf,
        to_path: PathBuf,
        old_text: Option<String>,
        new_text: Option<String>,
    },
}

#[derive(Debug, Default)]
pub struct ClientEventNormalizer {
    tool_calls: HashMap<String, acp::ToolCall>,
}

impl ClientEventNormalizer {
    pub fn push_session_update(&mut self, update: &acp::SessionUpdate) -> Vec<ClientEvent> {
        match update {
            acp::SessionUpdate::AgentMessageChunk(chunk) => {
                message_delta_from_chunk(chunk, MessageStream::Answer)
                    .into_iter()
                    .map(ClientEvent::MessageDelta)
                    .collect()
            }
            acp::SessionUpdate::AgentThoughtChunk(chunk) => {
                message_delta_from_chunk(chunk, MessageStream::Reasoning)
                    .into_iter()
                    .map(ClientEvent::MessageDelta)
                    .collect()
            }
            acp::SessionUpdate::Plan(plan) => {
                vec![ClientEvent::PlanSnapshot(plan_snapshot_from_acp(plan))]
            }
            acp::SessionUpdate::ToolCall(tool_call) => {
                let call_id = tool_call.tool_call_id.to_string();
                self.tool_calls.insert(call_id, tool_call.clone());

                if is_generic_tool_call(tool_call) {
                    return Vec::new();
                }

                vec![ClientEvent::ToolSnapshot(tool_snapshot_from_tool_call(
                    tool_call,
                    ToolPhase::from_status(tool_call.status),
                ))]
            }
            acp::SessionUpdate::ToolCallUpdate(update) => {
                let call_id = update.tool_call_id.to_string();
                let entry = self.tool_calls.entry(call_id).or_insert_with(|| {
                    acp::ToolCall::new(update.tool_call_id.clone(), String::new())
                });
                entry.update(update.fields.clone());

                let phase = update
                    .fields
                    .status
                    .map(ToolPhase::from_status)
                    .unwrap_or_else(|| ToolPhase::from_status(entry.status));

                vec![ClientEvent::ToolSnapshot(tool_snapshot_from_tool_call(
                    entry, phase,
                ))]
            }
            acp::SessionUpdate::AvailableCommandsUpdate(update) => {
                let commands = update
                    .available_commands
                    .iter()
                    .map(|cmd| {
                        let input_hint = cmd.input.as_ref().map(|input| match input {
                            acp::AvailableCommandInput::Unstructured(u) => u.hint.clone(),
                            _ => String::new(),
                        });
                        AgentCommandInfo {
                            name: cmd.name.clone(),
                            description: cmd.description.clone(),
                            input_hint,
                        }
                    })
                    .collect();
                vec![ClientEvent::AgentCommandsUpdate(AgentCommandsUpdate {
                    commands,
                })]
            }
            _ => Vec::new(),
        }
    }

    pub fn push_permission_request(
        &mut self,
        request: &acp::RequestPermissionRequest,
    ) -> Vec<ClientEvent> {
        let call_id = request.tool_call.tool_call_id.to_string();
        let entry = self.tool_calls.entry(call_id.clone()).or_insert_with(|| {
            acp::ToolCall::new(request.tool_call.tool_call_id.clone(), String::new())
        });
        entry.update(request.tool_call.fields.clone());

        let snapshot = tool_snapshot_from_tool_call(entry, ToolPhase::PendingApproval);
        let approval = ApprovalRequest {
            call_id,
            title: snapshot.title.clone(),
            kind: snapshot.kind.clone(),
            options: request
                .options
                .iter()
                .map(ApprovalOption::from_acp)
                .collect(),
            subject: ApprovalSubject::ToolSnapshot(snapshot),
        };

        vec![ClientEvent::ApprovalRequest(approval)]
    }
}

impl ToolPhase {
    fn from_status(status: acp::ToolCallStatus) -> Self {
        match status {
            acp::ToolCallStatus::Pending => ToolPhase::Pending,
            acp::ToolCallStatus::InProgress => ToolPhase::InProgress,
            acp::ToolCallStatus::Completed => ToolPhase::Completed,
            acp::ToolCallStatus::Failed => ToolPhase::Failed,
            _ => ToolPhase::Pending,
        }
    }
}

impl ToolKind {
    fn from_acp(kind: acp::ToolKind) -> Self {
        match kind {
            acp::ToolKind::Read => ToolKind::Read,
            acp::ToolKind::Search => ToolKind::Search,
            acp::ToolKind::Execute => ToolKind::Execute,
            acp::ToolKind::Edit => ToolKind::Edit,
            acp::ToolKind::Delete => ToolKind::Delete,
            acp::ToolKind::Move => ToolKind::Move,
            acp::ToolKind::Fetch => ToolKind::Fetch,
            acp::ToolKind::Think => ToolKind::Think,
            other => ToolKind::Other(format!("{other:?}")),
        }
    }
}

impl ApprovalOption {
    fn from_acp(option: &acp::PermissionOption) -> Self {
        Self {
            option_id: option.option_id.to_string(),
            name: option.name.clone(),
            kind: match option.kind {
                acp::PermissionOptionKind::AllowAlways => ApprovalOptionKind::AllowAlways,
                acp::PermissionOptionKind::AllowOnce => ApprovalOptionKind::AllowOnce,
                acp::PermissionOptionKind::RejectOnce => ApprovalOptionKind::RejectOnce,
                other => ApprovalOptionKind::Other(format!("{other:?}")),
            },
        }
    }
}

impl PlanStatus {
    fn from_acp(status: acp::PlanEntryStatus) -> Self {
        match status {
            acp::PlanEntryStatus::Pending => PlanStatus::Pending,
            acp::PlanEntryStatus::InProgress => PlanStatus::InProgress,
            acp::PlanEntryStatus::Completed => PlanStatus::Completed,
            _ => PlanStatus::Pending,
        }
    }
}

fn is_generic_tool_call(tool_call: &acp::ToolCall) -> bool {
    tool_call.raw_input.is_none()
        && tool_call.locations.is_empty()
        && tool_call.content.is_empty()
        && !tool_call.title.contains('/')
}

/// Some ACP agents (e.g. Codex) send `kind: "edit"` for all file mutations.
/// The actual operation type is in `rawInput.changes.{path}.type`:
///   - `"add"` → file creation
///   - `"delete"` → file deletion
///   - `"update"` with non-null `move_path` → file rename/move
///   - `"update"` with null/absent `move_path` → normal edit (no refinement)
fn refine_edit_kind(kind: ToolKind, raw_input: &Option<serde_json::Value>) -> ToolKind {
    if kind != ToolKind::Edit {
        return kind;
    }
    let Some(input) = raw_input else { return kind };
    let Some(changes) = input.get("changes").and_then(|c| c.as_object()) else {
        return kind;
    };
    for (_path, change) in changes {
        if let Some(change_type) = change.get("type").and_then(|t| t.as_str()) {
            match change_type {
                "add" => return ToolKind::Create,
                "delete" => return ToolKind::Delete,
                _ => {
                    if change
                        .get("move_path")
                        .and_then(|v| v.as_str())
                        .is_some_and(|s| !s.is_empty())
                    {
                        return ToolKind::Move;
                    }
                }
            }
        }
    }
    kind
}

fn tool_snapshot_from_tool_call(tool_call: &acp::ToolCall, phase: ToolPhase) -> ToolSnapshot {
    let artifacts = artifacts_from_tool_call(tool_call);
    let invocation = invocation_from_tool_call(tool_call, &artifacts);

    ToolSnapshot {
        call_id: tool_call.tool_call_id.to_string(),
        title: sanitize_title(&tool_call.title),
        kind: refine_edit_kind(ToolKind::from_acp(tool_call.kind), &tool_call.raw_input),
        phase,
        locations: tool_call
            .locations
            .iter()
            .map(|location| ToolLocation {
                path: location.path.clone(),
                line: location.line,
            })
            .collect(),
        invocation,
        artifacts,
        raw_input: tool_call.raw_input.clone(),
        raw_output: tool_call.raw_output.clone(),
        owner_request_id: None,
    }
}

fn invocation_from_tool_call(
    tool_call: &acp::ToolCall,
    artifacts: &[Artifact],
) -> Option<Invocation> {
    let diff_changes: Vec<FileChange> = artifacts
        .iter()
        .filter_map(|artifact| match artifact {
            Artifact::Diff(change) => Some(change.clone()),
            Artifact::Text { .. } => None,
        })
        .collect();

    if !diff_changes.is_empty() {
        return Some(Invocation::FileChanges {
            changes: diff_changes,
        });
    }

    if let Some(invocation) = structured_invocation_from_tool_call(tool_call) {
        return Some(invocation);
    }

    if let Some(json) = &tool_call.raw_input {
        return Some(Invocation::RawJson(json.clone()));
    }

    location_fallback_invocation(tool_call)
}

fn location_fallback_invocation(tool_call: &acp::ToolCall) -> Option<Invocation> {
    let location = tool_call.locations.first()?;
    let path = location.path.clone();

    match tool_call.kind {
        acp::ToolKind::Read => Some(Invocation::Read { path }),
        acp::ToolKind::Search => Some(Invocation::Search {
            query: None,
            path: Some(path),
        }),
        // Edit/Delete/Move require more context (old_text/new_text) than a bare
        // location provides. They fall through to the TUI's location-path fallback.
        _ => None,
    }
}

fn sanitize_title(title: &str) -> String {
    const CWD_MARKER: &str = " [current working directory ";

    let Some(cwd_start) = title.find(CWD_MARKER) else {
        return title.to_string();
    };

    let before_cwd = &title[..cwd_start];
    let after_marker = cwd_start + CWD_MARKER.len();

    let remainder = if let Some(bracket_end) = title[after_marker..].find(']') {
        let after_bracket = after_marker + bracket_end + 1;
        title[after_bracket..].trim()
    } else {
        return before_cwd.trim().to_string();
    };

    if remainder.is_empty() {
        return before_cwd.trim().to_string();
    }

    // Strip trailing (description text) from the remainder — Gemini appends these
    // after the cwd bracket. Apply stripping to the remainder only, not the
    // reconstituted string, to avoid removing parentheses in the command itself.
    let remainder = if remainder.starts_with('(') && remainder.ends_with(')') {
        ""
    } else if let Some(paren_start) = remainder.rfind(" (")
        && remainder.ends_with(')')
    {
        remainder[..paren_start].trim()
    } else {
        remainder
    };

    if remainder.is_empty() {
        before_cwd.trim().to_string()
    } else {
        format!("{} {remainder}", before_cwd.trim())
    }
}

fn artifacts_from_tool_call(tool_call: &acp::ToolCall) -> Vec<Artifact> {
    let mut artifacts = tool_call
        .content
        .iter()
        .filter_map(|content| match content {
            acp::ToolCallContent::Diff(diff) => Some(Artifact::Diff(FileChange {
                path: diff.path.clone(),
                old_text: diff.old_text.clone(),
                new_text: diff.new_text.clone(),
            })),
            acp::ToolCallContent::Content(content) => match &content.content {
                acp::ContentBlock::Text(text) if !text.text.is_empty() => Some(Artifact::Text {
                    text: text.text.clone(),
                }),
                _ => None,
            },
            _ => None,
        })
        .collect::<Vec<_>>();

    if !artifacts
        .iter()
        .any(|artifact| matches!(artifact, Artifact::Text { .. }))
        && let Some(text) = raw_output_text(tool_call.raw_output.as_ref())
    {
        artifacts.push(Artifact::Text { text });
    }

    artifacts
}

fn structured_invocation_from_tool_call(tool_call: &acp::ToolCall) -> Option<Invocation> {
    let raw_input = tool_call.raw_input.as_ref()?;
    match tool_call.kind {
        acp::ToolKind::Edit => file_operations_from_edit_input(raw_input)
            .map(|operations| Invocation::FileOperations { operations }),
        acp::ToolKind::Delete => file_operation_from_delete_input(raw_input).map(|operation| {
            Invocation::FileOperations {
                operations: vec![operation],
            }
        }),
        acp::ToolKind::Move => {
            file_operation_from_move_input(raw_input).map(|operation| Invocation::FileOperations {
                operations: vec![operation],
            })
        }
        acp::ToolKind::Execute => {
            extract_command(raw_input).map(|command| Invocation::Command { command })
        }
        acp::ToolKind::Read => {
            let path = extract_path(raw_input, &["path", "file_path", "file"]).or_else(|| {
                parsed_command_path_for(raw_input, |parsed_command| {
                    matches!(parsed_command_type_value(parsed_command), Some("read"))
                })
            })?;
            Some(Invocation::Read { path })
        }
        acp::ToolKind::Search => {
            let mut path = raw_input
                .get("path")
                .or_else(|| raw_input.get("directory"))
                .and_then(serde_json::Value::as_str)
                .map(PathBuf::from);
            let mut query = raw_input
                .get("pattern")
                .or_else(|| raw_input.get("query"))
                .or_else(|| raw_input.get("glob"))
                .and_then(serde_json::Value::as_str)
                .map(str::to_string);

            if path.is_none()
                && (parsed_command_is_listing(raw_input) || parsed_command_is_search(raw_input))
            {
                path = parsed_command_path_for(raw_input, |parsed_command| {
                    matches!(
                        parsed_command_type_value(parsed_command),
                        Some("list_files")
                    ) || parsed_command_type_value(parsed_command).is_some_and(|type_| {
                        type_.contains("search")
                            || type_.contains("grep")
                            || type_.contains("rg")
                            || type_.contains("glob")
                    })
                });
            }

            if query.is_none() && parsed_command_is_search(raw_input) {
                query = parsed_command_query_for_search(raw_input);
            }

            if parsed_command_is_listing(raw_input)
                || (query.is_none() && title_looks_like_listing(&tool_call.title))
            {
                Some(Invocation::ListFiles { path })
            } else {
                Some(Invocation::Search { query, path })
            }
        }
        acp::ToolKind::Fetch | acp::ToolKind::Think | acp::ToolKind::Other => {
            Some(Invocation::Tool {
                tool_name: tool_call.title.clone(),
                input: Some(raw_input.clone()),
            })
        }
        _ => None,
    }
}

fn file_operations_from_edit_input(raw_input: &serde_json::Value) -> Option<Vec<FileOperation>> {
    let path = extract_path(raw_input, &["file_path", "path", "file"])?;

    if let Some(new_text) = raw_input.get("content").and_then(serde_json::Value::as_str) {
        return Some(vec![FileOperation::Create {
            path,
            new_text: new_text.to_string(),
        }]);
    }

    let old_text = raw_input
        .get("old_string")
        .and_then(serde_json::Value::as_str)?;
    let new_text = raw_input
        .get("new_string")
        .and_then(serde_json::Value::as_str)?;
    Some(vec![FileOperation::Update {
        path,
        old_text: old_text.to_string(),
        new_text: new_text.to_string(),
    }])
}

fn file_operation_from_delete_input(raw_input: &serde_json::Value) -> Option<FileOperation> {
    let path = extract_path(raw_input, &["file_path", "path", "file"])?;
    let old_text = raw_input
        .get("content")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string);
    Some(FileOperation::Delete { path, old_text })
}

fn file_operation_from_move_input(raw_input: &serde_json::Value) -> Option<FileOperation> {
    let from_path = extract_path(raw_input, &["from", "from_path", "path", "file"])?;
    let to_path = extract_path(raw_input, &["to", "to_path", "destination", "new_path"])?;
    let old_text = raw_input
        .get("old_string")
        .or_else(|| raw_input.get("content"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_string);
    let new_text = raw_input
        .get("new_string")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
        .or_else(|| old_text.clone());

    Some(FileOperation::Move {
        from_path,
        to_path,
        old_text,
        new_text,
    })
}

fn extract_path(raw_input: &serde_json::Value, keys: &[&str]) -> Option<PathBuf> {
    keys.iter()
        .find_map(|key| raw_input.get(*key))
        .and_then(serde_json::Value::as_str)
        .map(PathBuf::from)
}

fn extract_command(raw_input: &serde_json::Value) -> Option<String> {
    let command = raw_input.get("command").or_else(|| raw_input.get("cmd"));

    command
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
        .or_else(|| {
            command
                .and_then(serde_json::Value::as_array)
                .and_then(|command| command_from_array(command))
        })
        .or_else(|| parsed_command_query(raw_input))
}

fn command_from_array(command: &[serde_json::Value]) -> Option<String> {
    let command = command
        .iter()
        .map(serde_json::Value::as_str)
        .collect::<Option<Vec<_>>>()?;

    match command.as_slice() {
        [shell, flag, script] if is_shell_wrapper(shell, flag) => Some((*script).to_string()),
        _ => Some(command.join(" ")),
    }
}

fn is_shell_wrapper(shell: &str, flag: &str) -> bool {
    let shell_name = shell.rsplit('/').next().unwrap_or(shell);
    matches!(flag, "-c" | "-lc")
        && matches!(
            shell_name,
            "bash" | "sh" | "zsh" | "fish" | "pwsh" | "powershell"
        )
}

fn parsed_commands(raw_input: &serde_json::Value) -> impl Iterator<Item = &serde_json::Value> {
    raw_input
        .get("parsed_cmd")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
}

fn find_parsed_command<'a, F>(
    raw_input: &'a serde_json::Value,
    predicate: F,
) -> Option<&'a serde_json::Value>
where
    F: FnMut(&&'a serde_json::Value) -> bool,
{
    parsed_commands(raw_input).find(predicate)
}

fn parsed_command_type_value(parsed_command: &serde_json::Value) -> Option<&str> {
    parsed_command.get("type")?.as_str()
}

fn parsed_command_path_value(parsed_command: &serde_json::Value) -> Option<PathBuf> {
    parsed_command.get("path")?.as_str().map(PathBuf::from)
}

fn parsed_command_query_value(parsed_command: &serde_json::Value) -> Option<String> {
    ["pattern", "query", "cmd"]
        .iter()
        .find_map(|key| parsed_command.get(*key)?.as_str())
        .map(str::to_string)
}

fn parsed_command_query(raw_input: &serde_json::Value) -> Option<String> {
    find_parsed_command(raw_input, |_| true).and_then(parsed_command_query_value)
}

fn parsed_command_is_listing(raw_input: &serde_json::Value) -> bool {
    find_parsed_command(raw_input, |parsed_command| {
        matches!(
            parsed_command_type_value(parsed_command),
            Some("list_files")
        )
    })
    .is_some()
}

fn parsed_command_is_search(raw_input: &serde_json::Value) -> bool {
    find_parsed_command(raw_input, |parsed_command| {
        parsed_command_type_value(parsed_command).is_some_and(|type_| {
            type_.contains("search")
                || type_.contains("grep")
                || type_.contains("rg")
                || type_.contains("glob")
        })
    })
    .is_some()
}

fn parsed_command_path_for(
    raw_input: &serde_json::Value,
    predicate: impl FnMut(&&serde_json::Value) -> bool,
) -> Option<PathBuf> {
    find_parsed_command(raw_input, predicate).and_then(parsed_command_path_value)
}

fn parsed_command_query_for_search(raw_input: &serde_json::Value) -> Option<String> {
    find_parsed_command(raw_input, |parsed_command| {
        parsed_command_type_value(parsed_command).is_some_and(|type_| {
            type_.contains("search")
                || type_.contains("grep")
                || type_.contains("rg")
                || type_.contains("glob")
        })
    })
    .and_then(parsed_command_query_value)
}

fn raw_output_text(raw_output: Option<&serde_json::Value>) -> Option<String> {
    let raw_output = raw_output?;

    if let Some(stdout) = raw_output.get("stdout").and_then(serde_json::Value::as_str)
        && !stdout.is_empty()
    {
        return Some(stdout.to_string());
    }

    if let Some(formatted) = raw_output
        .get("formatted_output")
        .and_then(serde_json::Value::as_str)
        && !formatted.is_empty()
    {
        return Some(formatted.to_string());
    }

    if let Some(aggregated) = raw_output
        .get("aggregated_output")
        .and_then(serde_json::Value::as_str)
        && !aggregated.is_empty()
    {
        return Some(aggregated.to_string());
    }

    if let Some(lines) = raw_output.get("lines").and_then(serde_json::Value::as_i64) {
        return Some(format!("Read {lines} lines"));
    }

    if let Some(count) = raw_output.get("count").and_then(serde_json::Value::as_i64) {
        return Some(format!("{count} matches"));
    }

    None
}

fn title_looks_like_listing(title: &str) -> bool {
    let title = title.to_lowercase();
    title.contains("list") || title.contains("glob") || title.contains("ls")
}

fn message_delta_from_chunk(
    chunk: &acp::ContentChunk,
    stream: MessageStream,
) -> Option<MessageDelta> {
    match &chunk.content {
        acp::ContentBlock::Text(text) if !text.text.is_empty() => Some(MessageDelta {
            stream,
            delta: text.text.clone(),
        }),
        _ => None,
    }
}

fn plan_snapshot_from_acp(plan: &acp::Plan) -> PlanSnapshot {
    PlanSnapshot {
        entries: plan
            .entries
            .iter()
            .map(|entry| PlanEntry {
                step: entry.content.clone(),
                status: PlanStatus::from_acp(entry.status.clone()),
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    fn sample_permission_options() -> Vec<acp::PermissionOption> {
        vec![
            acp::PermissionOption::new(
                acp::PermissionOptionId::new("allow-once"),
                "Allow",
                acp::PermissionOptionKind::AllowOnce,
            ),
            acp::PermissionOption::new(
                acp::PermissionOptionId::new("reject-once"),
                "Reject",
                acp::PermissionOptionKind::RejectOnce,
            ),
        ]
    }

    #[test]
    fn normalizer_merges_placeholder_tool_call_with_refined_update() {
        let mut normalizer = ClientEventNormalizer::default();

        let placeholder = acp::SessionUpdate::ToolCall(
            acp::ToolCall::new(acp::ToolCallId::new("tool-1"), "Edit")
                .kind(acp::ToolKind::Edit)
                .status(acp::ToolCallStatus::Pending),
        );
        let refined = acp::SessionUpdate::ToolCallUpdate(acp::ToolCallUpdate::new(
            acp::ToolCallId::new("tool-1"),
            acp::ToolCallUpdateFields::new()
                .title("Edit /repo/README.md")
                .kind(acp::ToolKind::Edit)
                .status(acp::ToolCallStatus::InProgress)
                .locations(vec![acp::ToolCallLocation::new("/repo/README.md")])
                .raw_input(serde_json::json!({
                    "file_path": "/repo/README.md",
                    "old_string": "before\n",
                    "new_string": "after\n",
                }))
                .content(vec![
                    acp::Diff::new("/repo/README.md", "after\n")
                        .old_text("before\n")
                        .into(),
                ]),
        ));
        let completed = acp::SessionUpdate::ToolCallUpdate(acp::ToolCallUpdate::new(
            acp::ToolCallId::new("tool-1"),
            acp::ToolCallUpdateFields::new()
                .status(acp::ToolCallStatus::Completed)
                .raw_output(serde_json::json!({"success": true})),
        ));

        assert!(normalizer.push_session_update(&placeholder).is_empty());

        let refined_events = normalizer.push_session_update(&refined);
        assert_eq!(refined_events.len(), 1);

        let completed_events = normalizer.push_session_update(&completed);
        assert_eq!(completed_events.len(), 1);

        let ClientEvent::ToolSnapshot(snapshot) = &completed_events[0] else {
            panic!("expected tool snapshot");
        };

        assert_eq!(snapshot.call_id, "tool-1");
        assert_eq!(snapshot.title, "Edit /repo/README.md");
        assert_eq!(snapshot.kind, ToolKind::Edit);
        assert_eq!(snapshot.phase, ToolPhase::Completed);
        assert_eq!(
            snapshot.locations,
            vec![ToolLocation {
                path: "/repo/README.md".into(),
                line: None,
            }]
        );
        assert_eq!(
            snapshot.invocation,
            Some(Invocation::FileChanges {
                changes: vec![FileChange {
                    path: "/repo/README.md".into(),
                    old_text: Some("before\n".into()),
                    new_text: "after\n".into(),
                }],
            })
        );
        assert_eq!(
            snapshot.artifacts,
            vec![Artifact::Diff(FileChange {
                path: "/repo/README.md".into(),
                old_text: Some("before\n".into()),
                new_text: "after\n".into(),
            })]
        );
        assert_eq!(
            snapshot.raw_output,
            Some(serde_json::json!({"success": true}))
        );
    }

    #[test]
    fn normalizer_emits_approval_request_without_losing_diff_snapshot() {
        let mut normalizer = ClientEventNormalizer::default();
        let session_id = acp::SessionId::new("session-1");

        let request = acp::RequestPermissionRequest::new(
            session_id,
            acp::ToolCallUpdate::new(
                acp::ToolCallId::new("tool-approve"),
                acp::ToolCallUpdateFields::new()
                    .title("Write /repo/tmp.md")
                    .kind(acp::ToolKind::Edit)
                    .locations(vec![acp::ToolCallLocation::new("/repo/tmp.md")])
                    .raw_input(serde_json::json!({
                        "file_path": "/repo/tmp.md",
                        "content": "hello\n",
                    }))
                    .content(vec![acp::Diff::new("/repo/tmp.md", "hello\n").into()]),
            ),
            sample_permission_options(),
        );

        let events = normalizer.push_permission_request(&request);
        assert_eq!(events.len(), 1);

        let ClientEvent::ApprovalRequest(approval) = &events[0] else {
            panic!("expected approval request");
        };

        assert_eq!(approval.call_id, "tool-approve");
        assert_eq!(approval.title, "Write /repo/tmp.md");
        assert_eq!(approval.kind, ToolKind::Edit);
        assert_eq!(
            approval.subject,
            ApprovalSubject::ToolSnapshot(ToolSnapshot {
                call_id: "tool-approve".into(),
                title: "Write /repo/tmp.md".into(),
                kind: ToolKind::Edit,
                phase: ToolPhase::PendingApproval,
                locations: vec![ToolLocation {
                    path: "/repo/tmp.md".into(),
                    line: None,
                }],
                invocation: Some(Invocation::FileChanges {
                    changes: vec![FileChange {
                        path: "/repo/tmp.md".into(),
                        old_text: None,
                        new_text: "hello\n".into(),
                    }],
                }),
                artifacts: vec![Artifact::Diff(FileChange {
                    path: "/repo/tmp.md".into(),
                    old_text: None,
                    new_text: "hello\n".into(),
                })],
                raw_input: Some(serde_json::json!({
                    "file_path": "/repo/tmp.md",
                    "content": "hello\n",
                })),
                raw_output: None,
                owner_request_id: None,
            })
        );
        assert_eq!(approval.options.len(), 2);
    }

    #[test]
    fn normalizer_extracts_execute_invocation_and_output_text() {
        let mut normalizer = ClientEventNormalizer::default();

        let update = acp::SessionUpdate::ToolCallUpdate(acp::ToolCallUpdate::new(
            acp::ToolCallId::new("tool-exec"),
            acp::ToolCallUpdateFields::new()
                .title("Terminal")
                .kind(acp::ToolKind::Execute)
                .status(acp::ToolCallStatus::Completed)
                .raw_input(serde_json::json!({
                    "command": "cargo test -p nori-tui",
                }))
                .raw_output(serde_json::json!({
                    "stdout": "all green\n",
                })),
        ));

        let events = normalizer.push_session_update(&update);
        assert_eq!(events.len(), 1);

        let ClientEvent::ToolSnapshot(snapshot) = &events[0] else {
            panic!("expected tool snapshot");
        };

        assert_eq!(
            snapshot.invocation,
            Some(Invocation::Command {
                command: "cargo test -p nori-tui".into(),
            })
        );
        assert_eq!(
            snapshot.artifacts,
            vec![Artifact::Text {
                text: "all green\n".into(),
            }]
        );
    }

    #[test]
    fn normalizer_extracts_read_invocation_path() {
        let mut normalizer = ClientEventNormalizer::default();

        let update = acp::SessionUpdate::ToolCallUpdate(acp::ToolCallUpdate::new(
            acp::ToolCallId::new("tool-read"),
            acp::ToolCallUpdateFields::new()
                .title("Read File")
                .kind(acp::ToolKind::Read)
                .status(acp::ToolCallStatus::Completed)
                .raw_input(serde_json::json!({
                    "path": "Cargo.toml",
                })),
        ));

        let events = normalizer.push_session_update(&update);
        assert_eq!(events.len(), 1);

        let ClientEvent::ToolSnapshot(snapshot) = &events[0] else {
            panic!("expected tool snapshot");
        };

        assert_eq!(
            snapshot.invocation,
            Some(Invocation::Read {
                path: PathBuf::from("Cargo.toml"),
            })
        );
    }

    #[test]
    fn normalizer_extracts_codex_execute_command_from_command_array() {
        let mut normalizer = ClientEventNormalizer::default();

        let update = acp::SessionUpdate::ToolCallUpdate(acp::ToolCallUpdate::new(
            acp::ToolCallId::new("tool-exec-codex"),
            acp::ToolCallUpdateFields::new()
                .title("Run df -h .")
                .kind(acp::ToolKind::Execute)
                .status(acp::ToolCallStatus::Completed)
                .raw_input(serde_json::json!({
                    "command": ["/usr/bin/zsh", "-lc", "df -h ."],
                    "cwd": "/repo",
                    "parsed_cmd": [{
                        "cmd": "df -h .",
                        "type": "unknown",
                    }],
                })),
        ));

        let events = normalizer.push_session_update(&update);
        assert_eq!(events.len(), 1);

        let ClientEvent::ToolSnapshot(snapshot) = &events[0] else {
            panic!("expected tool snapshot");
        };

        assert_eq!(
            snapshot.invocation,
            Some(Invocation::Command {
                command: "df -h .".into(),
            })
        );
    }

    #[test]
    fn normalizer_preserves_non_shell_command_arrays() {
        let mut normalizer = ClientEventNormalizer::default();

        let update = acp::SessionUpdate::ToolCallUpdate(acp::ToolCallUpdate::new(
            acp::ToolCallId::new("tool-exec-array"),
            acp::ToolCallUpdateFields::new()
                .title("Run ls -l")
                .kind(acp::ToolKind::Execute)
                .status(acp::ToolCallStatus::Completed)
                .raw_input(serde_json::json!({
                    "command": ["ls", "-l"],
                })),
        ));

        let events = normalizer.push_session_update(&update);
        assert_eq!(events.len(), 1);

        let ClientEvent::ToolSnapshot(snapshot) = &events[0] else {
            panic!("expected tool snapshot");
        };

        assert_eq!(
            snapshot.invocation,
            Some(Invocation::Command {
                command: "ls -l".into(),
            })
        );
    }

    #[test]
    fn normalizer_extracts_codex_read_path_from_parsed_command() {
        let mut normalizer = ClientEventNormalizer::default();

        let update = acp::SessionUpdate::ToolCallUpdate(acp::ToolCallUpdate::new(
            acp::ToolCallId::new("tool-read-codex"),
            acp::ToolCallUpdateFields::new()
                .title("Read SKILL.md")
                .kind(acp::ToolKind::Read)
                .status(acp::ToolCallStatus::Completed)
                .raw_input(serde_json::json!({
                    "command": ["/usr/bin/zsh", "-lc", "sed -n '1,220p' /repo/SKILL.md"],
                    "cwd": "/repo",
                    "parsed_cmd": [{
                        "cmd": "sed -n '1,220p' /repo/SKILL.md",
                        "name": "SKILL.md",
                        "path": "/repo/SKILL.md",
                        "type": "read",
                    }],
                })),
        ));

        let events = normalizer.push_session_update(&update);
        assert_eq!(events.len(), 1);

        let ClientEvent::ToolSnapshot(snapshot) = &events[0] else {
            panic!("expected tool snapshot");
        };

        assert_eq!(
            snapshot.invocation,
            Some(Invocation::Read {
                path: PathBuf::from("/repo/SKILL.md"),
            })
        );
    }

    #[test]
    fn normalizer_extracts_codex_read_path_from_later_parsed_command_entry() {
        let mut normalizer = ClientEventNormalizer::default();

        let update = acp::SessionUpdate::ToolCallUpdate(acp::ToolCallUpdate::new(
            acp::ToolCallId::new("tool-read-codex-later"),
            acp::ToolCallUpdateFields::new()
                .title("Read file")
                .kind(acp::ToolKind::Read)
                .status(acp::ToolCallStatus::Completed)
                .raw_input(serde_json::json!({
                    "command": ["/usr/bin/zsh", "-lc", "test -f /repo/SKILL.md && sed -n '1,20p' /repo/SKILL.md"],
                    "parsed_cmd": [
                        {
                            "cmd": "test -f /repo/SKILL.md",
                            "type": "unknown",
                        },
                        {
                            "cmd": "sed -n '1,20p' /repo/SKILL.md",
                            "name": "SKILL.md",
                            "path": "/repo/SKILL.md",
                            "type": "read",
                        }
                    ],
                })),
        ));

        let events = normalizer.push_session_update(&update);
        assert_eq!(events.len(), 1);

        let ClientEvent::ToolSnapshot(snapshot) = &events[0] else {
            panic!("expected tool snapshot");
        };

        assert_eq!(
            snapshot.invocation,
            Some(Invocation::Read {
                path: PathBuf::from("/repo/SKILL.md"),
            })
        );
    }

    #[test]
    fn normalizer_extracts_codex_list_files_from_parsed_command() {
        let mut normalizer = ClientEventNormalizer::default();

        let update = acp::SessionUpdate::ToolCallUpdate(acp::ToolCallUpdate::new(
            acp::ToolCallId::new("tool-list-codex"),
            acp::ToolCallUpdateFields::new()
                .title("List files")
                .kind(acp::ToolKind::Search)
                .status(acp::ToolCallStatus::Completed)
                .raw_input(serde_json::json!({
                    "command": ["/usr/bin/zsh", "-lc", "find /repo/src -maxdepth 1"],
                    "cwd": "/repo",
                    "parsed_cmd": [{
                        "cmd": "find /repo/src -maxdepth 1",
                        "path": "/repo/src",
                        "type": "list_files",
                    }],
                })),
        ));

        let events = normalizer.push_session_update(&update);
        assert_eq!(events.len(), 1);

        let ClientEvent::ToolSnapshot(snapshot) = &events[0] else {
            panic!("expected tool snapshot");
        };

        assert_eq!(
            snapshot.invocation,
            Some(Invocation::ListFiles {
                path: Some(PathBuf::from("/repo/src")),
            })
        );
    }

    #[test]
    fn normalizer_extracts_codex_search_from_later_parsed_command_entry() {
        let mut normalizer = ClientEventNormalizer::default();

        let update = acp::SessionUpdate::ToolCallUpdate(acp::ToolCallUpdate::new(
            acp::ToolCallId::new("tool-search-codex-later"),
            acp::ToolCallUpdateFields::new()
                .title("Search files")
                .kind(acp::ToolKind::Search)
                .status(acp::ToolCallStatus::Completed)
                .raw_input(serde_json::json!({
                    "command": ["/usr/bin/zsh", "-lc", "cd /repo && rg needle src"],
                    "parsed_cmd": [
                        {
                            "cmd": "cd /repo",
                            "type": "unknown",
                        },
                        {
                            "cmd": "rg needle src",
                            "path": "src",
                            "query": "needle",
                            "type": "search",
                        }
                    ],
                })),
        ));

        let events = normalizer.push_session_update(&update);
        assert_eq!(events.len(), 1);

        let ClientEvent::ToolSnapshot(snapshot) = &events[0] else {
            panic!("expected tool snapshot");
        };

        assert_eq!(
            snapshot.invocation,
            Some(Invocation::Search {
                query: Some("needle".into()),
                path: Some(PathBuf::from("src")),
            })
        );
    }

    #[test]
    fn normalizer_extracts_codex_search_from_parsed_command() {
        let mut normalizer = ClientEventNormalizer::default();

        let update = acp::SessionUpdate::ToolCallUpdate(acp::ToolCallUpdate::new(
            acp::ToolCallId::new("tool-search-codex"),
            acp::ToolCallUpdateFields::new()
                .title("Search files")
                .kind(acp::ToolKind::Search)
                .status(acp::ToolCallStatus::Completed)
                .raw_input(serde_json::json!({
                    "command": ["/usr/bin/zsh", "-lc", "rg structured_invocation /repo/src"],
                    "cwd": "/repo",
                    "parsed_cmd": [{
                        "cmd": "rg structured_invocation /repo/src",
                        "path": "/repo/src",
                        "type": "search",
                    }],
                })),
        ));

        let events = normalizer.push_session_update(&update);
        assert_eq!(events.len(), 1);

        let ClientEvent::ToolSnapshot(snapshot) = &events[0] else {
            panic!("expected tool snapshot");
        };

        assert_eq!(
            snapshot.invocation,
            Some(Invocation::Search {
                query: Some("rg structured_invocation /repo/src".into()),
                path: Some(PathBuf::from("/repo/src")),
            })
        );
    }

    #[test]
    fn normalizer_emits_answer_message_delta_from_agent_message_chunk() {
        let mut normalizer = ClientEventNormalizer::default();
        let update = acp::SessionUpdate::AgentMessageChunk(acp::ContentChunk::new(
            acp::ContentBlock::Text(acp::TextContent::new("Hello from ACP")),
        ));

        let events = normalizer.push_session_update(&update);

        assert_eq!(
            events,
            vec![ClientEvent::MessageDelta(MessageDelta {
                stream: MessageStream::Answer,
                delta: "Hello from ACP".into(),
            })]
        );
    }

    #[test]
    fn normalizer_emits_reasoning_message_delta_from_agent_thought_chunk() {
        let mut normalizer = ClientEventNormalizer::default();
        let update = acp::SessionUpdate::AgentThoughtChunk(acp::ContentChunk::new(
            acp::ContentBlock::Text(acp::TextContent::new("**Analyzing** the repo")),
        ));

        let events = normalizer.push_session_update(&update);

        assert_eq!(
            events,
            vec![ClientEvent::MessageDelta(MessageDelta {
                stream: MessageStream::Reasoning,
                delta: "**Analyzing** the repo".into(),
            })]
        );
    }

    #[test]
    fn normalizer_emits_plan_snapshot_from_plan_update() {
        let mut normalizer = ClientEventNormalizer::default();
        let update = acp::SessionUpdate::Plan(acp::Plan::new(vec![
            acp::PlanEntry::new(
                "Research ACP",
                acp::PlanEntryPriority::High,
                acp::PlanEntryStatus::Completed,
            ),
            acp::PlanEntry::new(
                "Implement normalized flow",
                acp::PlanEntryPriority::Medium,
                acp::PlanEntryStatus::InProgress,
            ),
            acp::PlanEntry::new(
                "Delete legacy translation",
                acp::PlanEntryPriority::Low,
                acp::PlanEntryStatus::Pending,
            ),
        ]));

        let events = normalizer.push_session_update(&update);

        assert_eq!(
            events,
            vec![ClientEvent::PlanSnapshot(PlanSnapshot {
                entries: vec![
                    PlanEntry {
                        step: "Research ACP".into(),
                        status: PlanStatus::Completed,
                    },
                    PlanEntry {
                        step: "Implement normalized flow".into(),
                        status: PlanStatus::InProgress,
                    },
                    PlanEntry {
                        step: "Delete legacy translation".into(),
                        status: PlanStatus::Pending,
                    },
                ],
            })]
        );
    }

    #[test]
    fn client_event_round_trips_through_serde() {
        let event = ClientEvent::ToolSnapshot(ToolSnapshot {
            call_id: "tool-1".to_string(),
            title: "Edit /repo/README.md".to_string(),
            kind: ToolKind::Edit,
            phase: ToolPhase::Completed,
            locations: vec![ToolLocation {
                path: PathBuf::from("/repo/README.md"),
                line: Some(12),
            }],
            invocation: Some(Invocation::FileChanges {
                changes: vec![FileChange {
                    path: PathBuf::from("/repo/README.md"),
                    old_text: Some("before\n".to_string()),
                    new_text: "after\n".to_string(),
                }],
            }),
            artifacts: vec![Artifact::Diff(FileChange {
                path: PathBuf::from("/repo/README.md"),
                old_text: Some("before\n".to_string()),
                new_text: "after\n".to_string(),
            })],
            raw_input: Some(serde_json::json!({"path": "/repo/README.md"})),
            raw_output: None,
            owner_request_id: None,
        });

        let json = serde_json::to_string(&event).unwrap();
        let parsed: ClientEvent = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed, event);
    }

    #[test]
    fn turn_lifecycle_event_round_trips_through_serde() {
        let event = ClientEvent::TurnLifecycle(TurnLifecycle::ContextCompacted {
            summary: Some("Compact summary".into()),
        });

        let json = serde_json::to_string(&event).unwrap();
        let parsed: ClientEvent = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed, event);
    }

    #[test]
    fn replay_entry_event_round_trips_through_serde() {
        let event = ClientEvent::ReplayEntry(ReplayEntry::AssistantMessage {
            text: "Replayed answer".into(),
        });

        let json = serde_json::to_string(&event).unwrap();
        let parsed: ClientEvent = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed, event);
    }

    #[test]
    fn normalizer_extracts_delete_file_operation() {
        let mut normalizer = ClientEventNormalizer::default();

        let update = acp::SessionUpdate::ToolCallUpdate(acp::ToolCallUpdate::new(
            acp::ToolCallId::new("tool-delete"),
            acp::ToolCallUpdateFields::new()
                .title("Delete README.md")
                .kind(acp::ToolKind::Delete)
                .status(acp::ToolCallStatus::Completed)
                .raw_input(serde_json::json!({
                    "path": "README.md",
                    "content": "before\n",
                })),
        ));

        let events = normalizer.push_session_update(&update);
        assert_eq!(events.len(), 1);

        let ClientEvent::ToolSnapshot(snapshot) = &events[0] else {
            panic!("expected tool snapshot");
        };

        assert_eq!(snapshot.kind, ToolKind::Delete);
        assert_eq!(
            snapshot.invocation,
            Some(Invocation::FileOperations {
                operations: vec![FileOperation::Delete {
                    path: PathBuf::from("README.md"),
                    old_text: Some("before\n".into()),
                }],
            })
        );
    }

    #[test]
    fn normalizer_extracts_move_file_operation() {
        let mut normalizer = ClientEventNormalizer::default();

        let update = acp::SessionUpdate::ToolCallUpdate(acp::ToolCallUpdate::new(
            acp::ToolCallId::new("tool-move"),
            acp::ToolCallUpdateFields::new()
                .title("Move README.md")
                .kind(acp::ToolKind::Move)
                .status(acp::ToolCallStatus::Completed)
                .raw_input(serde_json::json!({
                    "from": "README.md",
                    "to": "docs/README.md",
                    "content": "before\n",
                })),
        ));

        let events = normalizer.push_session_update(&update);
        assert_eq!(events.len(), 1);

        let ClientEvent::ToolSnapshot(snapshot) = &events[0] else {
            panic!("expected tool snapshot");
        };

        assert_eq!(snapshot.kind, ToolKind::Move);
        assert_eq!(
            snapshot.invocation,
            Some(Invocation::FileOperations {
                operations: vec![FileOperation::Move {
                    from_path: PathBuf::from("README.md"),
                    to_path: PathBuf::from("docs/README.md"),
                    old_text: Some("before\n".into()),
                    new_text: Some("before\n".into()),
                }],
            })
        );
    }

    #[test]
    fn normalizer_extracts_generic_tool_invocation_for_fetch() {
        let mut normalizer = ClientEventNormalizer::default();

        let update = acp::SessionUpdate::ToolCallUpdate(acp::ToolCallUpdate::new(
            acp::ToolCallId::new("tool-fetch"),
            acp::ToolCallUpdateFields::new()
                .title("Fetch")
                .kind(acp::ToolKind::Fetch)
                .status(acp::ToolCallStatus::Completed)
                .raw_input(serde_json::json!({
                    "url": "https://example.com",
                })),
        ));

        let events = normalizer.push_session_update(&update);
        assert_eq!(events.len(), 1);

        let ClientEvent::ToolSnapshot(snapshot) = &events[0] else {
            panic!("expected tool snapshot");
        };

        assert_eq!(
            snapshot.invocation,
            Some(Invocation::Tool {
                tool_name: "Fetch".into(),
                input: Some(serde_json::json!({
                    "url": "https://example.com",
                })),
            })
        );
    }

    // --- Spec 08: Gemini Empty Content Fallback ---

    #[test]
    fn normalizer_synthesizes_read_invocation_from_location_when_no_raw_input() {
        let mut normalizer = ClientEventNormalizer::default();

        let tool_call = acp::SessionUpdate::ToolCall(
            acp::ToolCall::new(acp::ToolCallId::new("read-1"), "README.md")
                .kind(acp::ToolKind::Read)
                .status(acp::ToolCallStatus::Completed)
                .locations(vec![acp::ToolCallLocation::new("/repo/README.md")]),
        );

        let events = normalizer.push_session_update(&tool_call);
        assert_eq!(events.len(), 1);

        let ClientEvent::ToolSnapshot(snapshot) = &events[0] else {
            panic!("expected tool snapshot");
        };

        assert_eq!(
            snapshot.invocation,
            Some(Invocation::Read {
                path: "/repo/README.md".into(),
            })
        );
    }

    #[test]
    fn normalizer_synthesizes_search_invocation_from_location_when_no_raw_input() {
        let mut normalizer = ClientEventNormalizer::default();

        let tool_call = acp::SessionUpdate::ToolCall(
            acp::ToolCall::new(acp::ToolCallId::new("search-1"), "Search src")
                .kind(acp::ToolKind::Search)
                .status(acp::ToolCallStatus::Completed)
                .locations(vec![acp::ToolCallLocation::new("/repo/src")]),
        );

        let events = normalizer.push_session_update(&tool_call);
        assert_eq!(events.len(), 1);

        let ClientEvent::ToolSnapshot(snapshot) = &events[0] else {
            panic!("expected tool snapshot");
        };

        assert_eq!(
            snapshot.invocation,
            Some(Invocation::Search {
                query: None,
                path: Some("/repo/src".into()),
            })
        );
    }

    #[test]
    fn normalizer_does_not_synthesize_edit_invocation_from_location() {
        let mut normalizer = ClientEventNormalizer::default();

        let tool_call = acp::SessionUpdate::ToolCall(
            acp::ToolCall::new(acp::ToolCallId::new("edit-1"), "file.rs")
                .kind(acp::ToolKind::Edit)
                .status(acp::ToolCallStatus::InProgress)
                .locations(vec![acp::ToolCallLocation::new("/repo/file.rs")]),
        );

        let events = normalizer.push_session_update(&tool_call);
        assert_eq!(events.len(), 1);

        let ClientEvent::ToolSnapshot(snapshot) = &events[0] else {
            panic!("expected tool snapshot");
        };

        // Edit with only locations and no raw_input should NOT synthesize an
        // invocation — it falls through to the TUI's location-path fallback.
        assert_eq!(snapshot.invocation, None);
    }

    #[test]
    fn normalizer_sanitizes_title_stripping_cwd_and_description() {
        let mut normalizer = ClientEventNormalizer::default();

        let tool_call = acp::SessionUpdate::ToolCall(
            acp::ToolCall::new(
                acp::ToolCallId::new("exec-1"),
                "echo \"hello\" [current working directory /home/user/project] (Create test file)",
            )
            .kind(acp::ToolKind::Execute)
            .status(acp::ToolCallStatus::Completed)
            .raw_input(serde_json::json!({"command": "echo \"hello\""})),
        );

        let events = normalizer.push_session_update(&tool_call);
        assert_eq!(events.len(), 1);

        let ClientEvent::ToolSnapshot(snapshot) = &events[0] else {
            panic!("expected tool snapshot");
        };

        assert_eq!(snapshot.title, "echo \"hello\"");
    }

    #[test]
    fn normalizer_sanitizes_title_stripping_cwd_only() {
        let mut normalizer = ClientEventNormalizer::default();

        let tool_call = acp::SessionUpdate::ToolCall(
            acp::ToolCall::new(
                acp::ToolCallId::new("exec-2"),
                "ls -la [current working directory /home/user]",
            )
            .kind(acp::ToolKind::Execute)
            .status(acp::ToolCallStatus::Completed)
            .raw_input(serde_json::json!({"command": "ls -la"})),
        );

        let events = normalizer.push_session_update(&tool_call);
        assert_eq!(events.len(), 1);

        let ClientEvent::ToolSnapshot(snapshot) = &events[0] else {
            panic!("expected tool snapshot");
        };

        assert_eq!(snapshot.title, "ls -la");
    }

    #[test]
    fn normalizer_title_without_brackets_passes_through() {
        let mut normalizer = ClientEventNormalizer::default();

        let tool_call = acp::SessionUpdate::ToolCall(
            acp::ToolCall::new(acp::ToolCallId::new("exec-3"), "echo \"hello world\"")
                .kind(acp::ToolKind::Execute)
                .status(acp::ToolCallStatus::Completed)
                .raw_input(serde_json::json!({"command": "echo \"hello world\""})),
        );

        let events = normalizer.push_session_update(&tool_call);
        assert_eq!(events.len(), 1);

        let ClientEvent::ToolSnapshot(snapshot) = &events[0] else {
            panic!("expected tool snapshot");
        };

        assert_eq!(snapshot.title, "echo \"hello world\"");
    }

    #[test]
    fn normalizer_sanitizes_title_preserves_command_parens_without_description() {
        let mut normalizer = ClientEventNormalizer::default();

        let tool_call = acp::SessionUpdate::ToolCall(
            acp::ToolCall::new(
                acp::ToolCallId::new("exec-4"),
                "echo (hello) [current working directory /foo]",
            )
            .kind(acp::ToolKind::Execute)
            .status(acp::ToolCallStatus::Completed)
            .raw_input(serde_json::json!({"command": "echo (hello)"})),
        );

        let events = normalizer.push_session_update(&tool_call);
        assert_eq!(events.len(), 1);

        let ClientEvent::ToolSnapshot(snapshot) = &events[0] else {
            panic!("expected tool snapshot");
        };

        assert_eq!(snapshot.title, "echo (hello)");
    }

    #[test]
    fn normalizer_converts_available_commands_update() {
        let mut normalizer = ClientEventNormalizer::default();

        let update =
            acp::SessionUpdate::AvailableCommandsUpdate(acp::AvailableCommandsUpdate::new(vec![
                acp::AvailableCommand::new("loop", "Run a prompt on a recurring interval"),
                acp::AvailableCommand::new("schedule", "Create scheduled remote agents").input(
                    acp::AvailableCommandInput::Unstructured(acp::UnstructuredCommandInput::new(
                        "cron expression",
                    )),
                ),
            ]));

        let events = normalizer.push_session_update(&update);
        assert_eq!(events.len(), 1);

        let ClientEvent::AgentCommandsUpdate(commands_update) = &events[0] else {
            panic!("expected AgentCommandsUpdate");
        };

        assert_eq!(commands_update.commands.len(), 2);
        assert_eq!(commands_update.commands[0].name, "loop");
        assert_eq!(
            commands_update.commands[0].description,
            "Run a prompt on a recurring interval"
        );
        assert_eq!(commands_update.commands[0].input_hint, None);
        assert_eq!(commands_update.commands[1].name, "schedule");
        assert_eq!(
            commands_update.commands[1].input_hint,
            Some("cron expression".to_string())
        );
    }

    #[test]
    fn normalizer_converts_empty_available_commands_update() {
        let mut normalizer = ClientEventNormalizer::default();

        let update =
            acp::SessionUpdate::AvailableCommandsUpdate(acp::AvailableCommandsUpdate::new(vec![]));

        let events = normalizer.push_session_update(&update);
        assert_eq!(events.len(), 1);

        let ClientEvent::AgentCommandsUpdate(commands_update) = &events[0] else {
            panic!("expected AgentCommandsUpdate");
        };

        assert_eq!(commands_update.commands.len(), 0);
    }

    // ── refine_edit_kind: Codex ACP sends kind:"edit" for create/delete/move ──

    #[test]
    fn codex_edit_with_changes_type_add_becomes_create() {
        let mut normalizer = ClientEventNormalizer::default();

        let update = acp::SessionUpdate::ToolCallUpdate(acp::ToolCallUpdate::new(
            acp::ToolCallId::new("tool-create"),
            acp::ToolCallUpdateFields::new()
                .title("Edit /repo/new-file.txt")
                .kind(acp::ToolKind::Edit)
                .status(acp::ToolCallStatus::Completed)
                .raw_input(serde_json::json!({
                    "changes": {
                        "/repo/new-file.txt": {
                            "type": "add",
                            "content": "hello\n"
                        }
                    }
                }))
                .content(vec![acp::Diff::new("/repo/new-file.txt", "hello\n").into()]),
        ));

        let events = normalizer.push_session_update(&update);
        assert_eq!(events.len(), 1);

        let ClientEvent::ToolSnapshot(snapshot) = &events[0] else {
            panic!("expected tool snapshot");
        };

        assert_eq!(snapshot.kind, ToolKind::Create);
    }

    #[test]
    fn codex_edit_with_changes_type_delete_becomes_delete() {
        let mut normalizer = ClientEventNormalizer::default();

        let update = acp::SessionUpdate::ToolCallUpdate(acp::ToolCallUpdate::new(
            acp::ToolCallId::new("tool-delete"),
            acp::ToolCallUpdateFields::new()
                .title("Edit /repo/old-file.txt")
                .kind(acp::ToolKind::Edit)
                .status(acp::ToolCallStatus::Completed)
                .raw_input(serde_json::json!({
                    "changes": {
                        "/repo/old-file.txt": {
                            "type": "delete",
                            "content": "goodbye\n"
                        }
                    }
                }))
                .content(vec![
                    acp::Diff::new("/repo/old-file.txt", "")
                        .old_text("goodbye\n")
                        .into(),
                ]),
        ));

        let events = normalizer.push_session_update(&update);
        assert_eq!(events.len(), 1);

        let ClientEvent::ToolSnapshot(snapshot) = &events[0] else {
            panic!("expected tool snapshot");
        };

        assert_eq!(snapshot.kind, ToolKind::Delete);
    }

    #[test]
    fn codex_edit_with_move_path_becomes_move() {
        let mut normalizer = ClientEventNormalizer::default();

        let update = acp::SessionUpdate::ToolCallUpdate(acp::ToolCallUpdate::new(
            acp::ToolCallId::new("tool-move"),
            acp::ToolCallUpdateFields::new()
                .title("Edit /repo/old-name.txt")
                .kind(acp::ToolKind::Edit)
                .status(acp::ToolCallStatus::Completed)
                .raw_input(serde_json::json!({
                    "changes": {
                        "/repo/old-name.txt": {
                            "type": "update",
                            "unified_diff": "@@ -1 +1 @@\n old\n+new\n",
                            "move_path": "/repo/new-name.txt"
                        }
                    }
                }))
                .content(vec![
                    acp::Diff::new("/repo/old-name.txt", "new\n")
                        .old_text("old\n")
                        .into(),
                ]),
        ));

        let events = normalizer.push_session_update(&update);
        assert_eq!(events.len(), 1);

        let ClientEvent::ToolSnapshot(snapshot) = &events[0] else {
            panic!("expected tool snapshot");
        };

        assert_eq!(snapshot.kind, ToolKind::Move);
    }

    #[test]
    fn codex_edit_with_null_move_path_stays_edit() {
        let mut normalizer = ClientEventNormalizer::default();

        let update = acp::SessionUpdate::ToolCallUpdate(acp::ToolCallUpdate::new(
            acp::ToolCallId::new("tool-edit-normal"),
            acp::ToolCallUpdateFields::new()
                .title("Edit /repo/file.txt")
                .kind(acp::ToolKind::Edit)
                .status(acp::ToolCallStatus::Completed)
                .raw_input(serde_json::json!({
                    "changes": {
                        "/repo/file.txt": {
                            "type": "update",
                            "unified_diff": "@@ -1 +1 @@\n-old\n+new\n",
                            "move_path": null
                        }
                    }
                }))
                .content(vec![
                    acp::Diff::new("/repo/file.txt", "new\n")
                        .old_text("old\n")
                        .into(),
                ]),
        ));

        let events = normalizer.push_session_update(&update);
        assert_eq!(events.len(), 1);

        let ClientEvent::ToolSnapshot(snapshot) = &events[0] else {
            panic!("expected tool snapshot");
        };

        assert_eq!(snapshot.kind, ToolKind::Edit);
    }
}
