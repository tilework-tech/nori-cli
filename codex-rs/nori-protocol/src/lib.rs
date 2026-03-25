use std::collections::HashMap;
use std::path::PathBuf;

use sacp::schema as acp;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientEvent {
    ToolSnapshot(ToolSnapshot),
    ApprovalRequest(ApprovalRequest),
}

#[derive(Debug, Clone, PartialEq, Eq)]
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovalRequest {
    pub call_id: String,
    pub title: String,
    pub kind: ToolKind,
    pub options: Vec<ApprovalOption>,
    pub subject: ApprovalSubject,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApprovalSubject {
    ToolSnapshot(ToolSnapshot),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovalOption {
    pub option_id: String,
    pub name: String,
    pub kind: ApprovalOptionKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApprovalOptionKind {
    AllowAlways,
    AllowOnce,
    RejectOnce,
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolKind {
    Read,
    Search,
    Execute,
    Edit,
    Move,
    Fetch,
    Think,
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolPhase {
    Pending,
    PendingApproval,
    InProgress,
    Completed,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolLocation {
    pub path: PathBuf,
    pub line: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Invocation {
    FileChanges { changes: Vec<FileChange> },
    RawJson(serde_json::Value),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Artifact {
    Diff(FileChange),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileChange {
    pub path: PathBuf,
    pub old_text: Option<String>,
    pub new_text: String,
}

#[derive(Debug, Default)]
pub struct ClientEventNormalizer {
    tool_calls: HashMap<String, acp::ToolCall>,
}

impl ClientEventNormalizer {
    pub fn push_session_update(&mut self, update: &acp::SessionUpdate) -> Vec<ClientEvent> {
        match update {
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

fn is_generic_tool_call(tool_call: &acp::ToolCall) -> bool {
    tool_call.raw_input.is_none()
        && tool_call.locations.is_empty()
        && tool_call.content.is_empty()
        && !tool_call.title.contains('/')
}

fn tool_snapshot_from_tool_call(tool_call: &acp::ToolCall, phase: ToolPhase) -> ToolSnapshot {
    let artifacts = artifacts_from_tool_call(tool_call);
    let invocation = invocation_from_tool_call(tool_call, &artifacts);

    ToolSnapshot {
        call_id: tool_call.tool_call_id.to_string(),
        title: tool_call.title.clone(),
        kind: ToolKind::from_acp(tool_call.kind),
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
    }
}

fn invocation_from_tool_call(
    tool_call: &acp::ToolCall,
    artifacts: &[Artifact],
) -> Option<Invocation> {
    let diff_changes: Vec<FileChange> = artifacts
        .iter()
        .map(|artifact| match artifact {
            Artifact::Diff(change) => change.clone(),
        })
        .collect();

    if !diff_changes.is_empty() {
        return Some(Invocation::FileChanges {
            changes: diff_changes,
        });
    }

    tool_call.raw_input.clone().map(Invocation::RawJson)
}

fn artifacts_from_tool_call(tool_call: &acp::ToolCall) -> Vec<Artifact> {
    tool_call
        .content
        .iter()
        .filter_map(|content| match content {
            acp::ToolCallContent::Diff(diff) => Some(Artifact::Diff(FileChange {
                path: diff.path.clone(),
                old_text: diff.old_text.clone(),
                new_text: diff.new_text.clone(),
            })),
            _ => None,
        })
        .collect()
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
            })
        );
        assert_eq!(approval.options.len(), 2);
    }
}
