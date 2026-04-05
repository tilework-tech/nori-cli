use std::collections::HashMap;
use std::path::PathBuf;

use crate::app_event::AppEvent;
use crate::app_event_sender::AppEventSender;
use crate::bottom_pane::BottomPaneView;
use crate::bottom_pane::CancellationEvent;
use crate::bottom_pane::list_selection_view::ListSelectionView;
use crate::bottom_pane::list_selection_view::SelectionItem;
use crate::bottom_pane::list_selection_view::SelectionViewParams;
use crate::diff_render::DiffSummary;
use crate::exec_command::strip_bash_lc_and_escape;
use crate::history_cell;
use crate::key_hint;
use crate::key_hint::KeyBinding;
use crate::render::highlight::highlight_bash_to_lines;
use crate::render::renderable::ColumnRenderable;
use crate::render::renderable::Renderable;
use codex_core::protocol::ElicitationAction;
use codex_core::protocol::FileChange;
use codex_core::protocol::Op;
use codex_core::protocol::ReviewDecision;
use codex_core::protocol::SandboxCommandAssessment;
use codex_core::protocol::SandboxRiskLevel;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use crossterm::event::KeyModifiers;
use mcp_types::RequestId;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Wrap;

/// Request coming from the agent that needs user approval.
#[derive(Clone, Debug)]
pub(crate) enum ApprovalRequest {
    Exec {
        id: String,
        command: Vec<String>,
        reason: Option<String>,
        risk: Option<SandboxCommandAssessment>,
    },
    ApplyPatch {
        id: String,
        reason: Option<String>,
        cwd: PathBuf,
        changes: HashMap<PathBuf, FileChange>,
    },
    McpElicitation {
        server_name: String,
        request_id: RequestId,
        message: String,
    },
    /// ACP-native tool approval using protocol fields directly.
    AcpTool {
        call_id: String,
        title: String,
        kind: nori_protocol::ToolKind,
        cwd: PathBuf,
        snapshot: Box<nori_protocol::ToolSnapshot>,
    },
}

/// Modal overlay asking the user to approve or deny one or more requests.
pub(crate) struct ApprovalOverlay {
    current_request: Option<ApprovalRequest>,
    current_variant: Option<ApprovalVariant>,
    queue: Vec<ApprovalRequest>,
    app_event_tx: AppEventSender,
    list: ListSelectionView,
    options: Vec<ApprovalOption>,
    current_complete: bool,
    done: bool,
    agent_display_name: String,
}

impl ApprovalOverlay {
    pub fn new(
        request: ApprovalRequest,
        app_event_tx: AppEventSender,
        agent_display_name: String,
    ) -> Self {
        let mut view = Self {
            current_request: None,
            current_variant: None,
            queue: Vec::new(),
            app_event_tx: app_event_tx.clone(),
            list: ListSelectionView::new(Default::default(), app_event_tx),
            options: Vec::new(),
            current_complete: false,
            done: false,
            agent_display_name,
        };
        view.set_current(request);
        view
    }

    pub fn enqueue_request(&mut self, req: ApprovalRequest) {
        self.queue.push(req);
    }

    fn set_current(&mut self, request: ApprovalRequest) {
        self.current_request = Some(request.clone());
        let ApprovalRequestState { variant, header } = ApprovalRequestState::from(request);
        self.current_variant = Some(variant.clone());
        self.current_complete = false;
        let (options, params) = Self::build_options(variant, header, &self.agent_display_name);
        self.options = options;
        self.list = ListSelectionView::new(params, self.app_event_tx.clone());
    }

    fn build_options(
        variant: ApprovalVariant,
        header: Box<dyn Renderable>,
        agent_display_name: &str,
    ) -> (Vec<ApprovalOption>, SelectionViewParams) {
        let (options, title) = match &variant {
            ApprovalVariant::Exec { .. } => (
                exec_options(agent_display_name),
                "Would you like to run the following command?".to_string(),
            ),
            ApprovalVariant::ApplyPatch { .. } => (
                patch_options(agent_display_name),
                "Would you like to make the following edits?".to_string(),
            ),
            ApprovalVariant::McpElicitation { server_name, .. } => (
                elicitation_options(),
                format!("{server_name} needs your approval."),
            ),
            ApprovalVariant::AcpTool { title, kind, .. } => {
                let kind_str = crate::client_event_format::format_tool_kind(kind);
                (
                    acp_tool_options(agent_display_name),
                    format!("Would you like to allow {kind_str}: {title}?"),
                )
            }
        };

        let header = Box::new(ColumnRenderable::with([
            Line::from(title.bold()).into(),
            Line::from("").into(),
            header,
        ]));

        let items = options
            .iter()
            .map(|opt| SelectionItem {
                name: opt.label.clone(),
                display_shortcut: opt
                    .display_shortcut
                    .or_else(|| opt.additional_shortcuts.first().copied()),
                dismiss_on_select: false,
                ..Default::default()
            })
            .collect();

        let params = SelectionViewParams {
            footer_hint: Some(Line::from(vec![
                "Press ".into(),
                key_hint::plain(KeyCode::Enter).into(),
                " to confirm or ".into(),
                key_hint::plain(KeyCode::Esc).into(),
                " to cancel".into(),
            ])),
            items,
            header,
            ..Default::default()
        };

        (options, params)
    }

    fn apply_selection(&mut self, actual_idx: usize) {
        if self.current_complete {
            return;
        }
        let Some(option) = self.options.get(actual_idx) else {
            return;
        };
        if let Some(variant) = self.current_variant.as_ref() {
            match (&variant, &option.decision) {
                (ApprovalVariant::Exec { id, command }, ApprovalDecision::Review(decision)) => {
                    self.handle_exec_decision(id, command, *decision);
                }
                (ApprovalVariant::ApplyPatch { id, .. }, ApprovalDecision::Review(decision)) => {
                    self.handle_patch_decision(id, *decision);
                }
                (
                    ApprovalVariant::McpElicitation {
                        server_name,
                        request_id,
                    },
                    ApprovalDecision::McpElicitation(decision),
                ) => {
                    self.handle_elicitation_decision(server_name, request_id, *decision);
                }
                (
                    ApprovalVariant::AcpTool {
                        call_id,
                        title,
                        kind,
                    },
                    ApprovalDecision::Review(decision),
                ) => {
                    self.handle_acp_tool_decision(call_id, title, kind, *decision);
                }
                _ => {}
            }
        }

        self.current_complete = true;
        self.advance_queue();
    }

    fn handle_exec_decision(&self, id: &str, command: &[String], decision: ReviewDecision) {
        let cell = history_cell::new_approval_decision_cell(command.to_vec(), decision);
        self.app_event_tx.send(AppEvent::InsertHistoryCell(cell));
        self.app_event_tx.send(AppEvent::CodexOp(Op::ExecApproval {
            id: id.to_string(),
            decision,
        }));
    }

    fn handle_acp_tool_decision(
        &self,
        call_id: &str,
        title: &str,
        kind: &nori_protocol::ToolKind,
        decision: ReviewDecision,
    ) {
        let cell = history_cell::new_acp_approval_decision_cell(title, kind, decision);
        self.app_event_tx.send(AppEvent::InsertHistoryCell(cell));
        self.app_event_tx.send(AppEvent::CodexOp(Op::ExecApproval {
            id: call_id.to_string(),
            decision,
        }));
    }

    fn handle_patch_decision(&self, id: &str, decision: ReviewDecision) {
        self.app_event_tx.send(AppEvent::CodexOp(Op::PatchApproval {
            id: id.to_string(),
            decision,
        }));
    }

    fn handle_elicitation_decision(
        &self,
        server_name: &str,
        request_id: &RequestId,
        decision: ElicitationAction,
    ) {
        self.app_event_tx
            .send(AppEvent::CodexOp(Op::ResolveElicitation {
                server_name: server_name.to_string(),
                request_id: request_id.clone(),
                decision,
            }));
    }

    fn advance_queue(&mut self) {
        if let Some(next) = self.queue.pop() {
            self.set_current(next);
        } else {
            self.done = true;
        }
    }

    fn try_handle_shortcut(&mut self, key_event: &KeyEvent) -> bool {
        match key_event {
            KeyEvent {
                kind: KeyEventKind::Press,
                code: KeyCode::Char('a'),
                modifiers,
                ..
            } if modifiers.contains(KeyModifiers::CONTROL) => {
                if let Some(request) = self.current_request.as_ref() {
                    self.app_event_tx
                        .send(AppEvent::FullScreenApprovalRequest(request.clone()));
                    true
                } else {
                    false
                }
            }
            e => {
                if let Some(idx) = self
                    .options
                    .iter()
                    .position(|opt| opt.shortcuts().any(|s| s.is_press(*e)))
                {
                    self.apply_selection(idx);
                    true
                } else {
                    false
                }
            }
        }
    }
}

impl BottomPaneView for ApprovalOverlay {
    fn handle_key_event(&mut self, key_event: KeyEvent) {
        if self.try_handle_shortcut(&key_event) {
            return;
        }
        self.list.handle_key_event(key_event);
        if let Some(idx) = self.list.take_last_selected_index() {
            self.apply_selection(idx);
        }
    }

    fn on_ctrl_c(&mut self) -> CancellationEvent {
        if self.done {
            return CancellationEvent::Handled;
        }
        if !self.current_complete
            && let Some(variant) = self.current_variant.as_ref()
        {
            match &variant {
                ApprovalVariant::Exec { id, command } => {
                    self.handle_exec_decision(id, command, ReviewDecision::Abort);
                }
                ApprovalVariant::ApplyPatch { id, .. } => {
                    self.handle_patch_decision(id, ReviewDecision::Abort);
                }
                ApprovalVariant::McpElicitation {
                    server_name,
                    request_id,
                } => {
                    self.handle_elicitation_decision(
                        server_name,
                        request_id,
                        ElicitationAction::Cancel,
                    );
                }
                ApprovalVariant::AcpTool {
                    call_id,
                    title,
                    kind,
                } => {
                    self.handle_acp_tool_decision(call_id, title, kind, ReviewDecision::Abort);
                }
            }
        }
        self.queue.clear();
        self.done = true;
        CancellationEvent::Handled
    }

    fn is_complete(&self) -> bool {
        self.done
    }

    fn try_consume_approval_request(
        &mut self,
        request: ApprovalRequest,
    ) -> Option<ApprovalRequest> {
        self.enqueue_request(request);
        None
    }
}

impl Renderable for ApprovalOverlay {
    fn desired_height(&self, width: u16) -> u16 {
        self.list.desired_height(width)
    }

    fn render(&self, area: Rect, buf: &mut Buffer) {
        self.list.render(area, buf);
    }

    fn cursor_pos(&self, area: Rect) -> Option<(u16, u16)> {
        self.list.cursor_pos(area)
    }
}

struct ApprovalRequestState {
    variant: ApprovalVariant,
    header: Box<dyn Renderable>,
}

impl From<ApprovalRequest> for ApprovalRequestState {
    fn from(value: ApprovalRequest) -> Self {
        match value {
            ApprovalRequest::Exec {
                id,
                command,
                reason,
                risk,
            } => {
                let reason = reason.filter(|item| !item.is_empty());
                let has_reason = reason.is_some();
                let mut header: Vec<Line<'static>> = Vec::new();
                if let Some(reason) = reason {
                    header.push(Line::from(vec!["Reason: ".into(), reason.italic()]));
                }
                if let Some(risk) = risk.as_ref() {
                    header.extend(render_risk_lines(risk));
                } else if has_reason {
                    header.push(Line::from(""));
                }
                let full_cmd = strip_bash_lc_and_escape(&command);
                let mut full_cmd_lines = highlight_bash_to_lines(&full_cmd);
                if let Some(first) = full_cmd_lines.first_mut() {
                    first.spans.insert(0, Span::from("$ "));
                }
                header.extend(full_cmd_lines);
                Self {
                    variant: ApprovalVariant::Exec { id, command },
                    header: Box::new(Paragraph::new(header).wrap(Wrap { trim: false })),
                }
            }
            ApprovalRequest::ApplyPatch {
                id,
                reason,
                cwd,
                changes,
            } => {
                let mut header: Vec<Box<dyn Renderable>> = Vec::new();
                if let Some(reason) = reason
                    && !reason.is_empty()
                {
                    header.push(Box::new(
                        Paragraph::new(Line::from_iter(["Reason: ".into(), reason.italic()]))
                            .wrap(Wrap { trim: false }),
                    ));
                    header.push(Box::new(Line::from("")));
                }
                header.push(DiffSummary::new(changes, cwd).into());
                Self {
                    variant: ApprovalVariant::ApplyPatch { id },
                    header: Box::new(ColumnRenderable::with(header)),
                }
            }
            ApprovalRequest::McpElicitation {
                server_name,
                request_id,
                message,
            } => {
                let header = Paragraph::new(vec![
                    Line::from(vec!["Server: ".into(), server_name.clone().bold()]),
                    Line::from(""),
                    Line::from(message),
                ])
                .wrap(Wrap { trim: false });
                Self {
                    variant: ApprovalVariant::McpElicitation {
                        server_name,
                        request_id,
                    },
                    header: Box::new(header),
                }
            }
            ApprovalRequest::AcpTool {
                call_id,
                title,
                kind,
                cwd,
                snapshot,
            } => {
                let rel_title = crate::client_event_format::relativize_paths_in_text(&title, &cwd);

                let is_edit_like = matches!(
                    kind,
                    nori_protocol::ToolKind::Create
                        | nori_protocol::ToolKind::Edit
                        | nori_protocol::ToolKind::Delete
                        | nori_protocol::ToolKind::Move
                );

                // For edit-like tools, try to render a DiffSummary from the snapshot
                if is_edit_like {
                    let mut changes =
                        crate::client_tool_cell::diff_changes_from_artifacts(&snapshot.artifacts);
                    if changes.is_empty() {
                        changes =
                            crate::client_tool_cell::changes_from_invocation(&snapshot.invocation);
                    }
                    if !changes.is_empty() {
                        let header: Vec<Box<dyn Renderable>> =
                            vec![DiffSummary::new(changes, cwd).into()];
                        return Self {
                            variant: ApprovalVariant::AcpTool {
                                call_id,
                                title: rel_title,
                                kind,
                            },
                            header: Box::new(ColumnRenderable::with(header)),
                        };
                    }
                }

                // Non-edit tools or edit tools without diff data: text-only rendering
                let mut lines: Vec<Line<'static>> = Vec::new();
                lines.push(Line::from(rel_title.clone()));
                if let Some(inv_text) =
                    crate::client_event_format::format_invocation(&snapshot.invocation)
                {
                    let rel_inv =
                        crate::client_event_format::relativize_paths_in_text(&inv_text, &cwd);
                    if !crate::client_event_format::is_invocation_redundant(&rel_inv, &rel_title) {
                        lines.push(Line::from(rel_inv));
                    }
                }
                for text in crate::client_event_format::format_artifacts(&snapshot.artifacts) {
                    lines.push(Line::from(text));
                }
                Self {
                    variant: ApprovalVariant::AcpTool {
                        call_id,
                        title: rel_title,
                        kind,
                    },
                    header: Box::new(Paragraph::new(lines).wrap(Wrap { trim: false })),
                }
            }
        }
    }
}

fn render_risk_lines(risk: &SandboxCommandAssessment) -> Vec<Line<'static>> {
    let level_span = match risk.risk_level {
        SandboxRiskLevel::Low => "LOW".green().bold(),
        SandboxRiskLevel::Medium => "MEDIUM".cyan().bold(),
        SandboxRiskLevel::High => "HIGH".red().bold(),
    };

    let mut lines = Vec::new();

    let description = risk.description.trim();
    if !description.is_empty() {
        lines.push(Line::from(vec![
            "Summary: ".into(),
            description.to_string().into(),
        ]));
    }

    lines.push(vec!["Risk: ".into(), level_span].into());
    lines.push(Line::from(""));
    lines
}

#[derive(Clone)]
enum ApprovalVariant {
    Exec {
        id: String,
        command: Vec<String>,
    },
    ApplyPatch {
        id: String,
    },
    McpElicitation {
        server_name: String,
        request_id: RequestId,
    },
    AcpTool {
        call_id: String,
        title: String,
        kind: nori_protocol::ToolKind,
    },
}

#[derive(Clone)]
enum ApprovalDecision {
    Review(ReviewDecision),
    McpElicitation(ElicitationAction),
}

#[derive(Clone)]
struct ApprovalOption {
    label: String,
    decision: ApprovalDecision,
    display_shortcut: Option<KeyBinding>,
    additional_shortcuts: Vec<KeyBinding>,
}

impl ApprovalOption {
    fn shortcuts(&self) -> impl Iterator<Item = KeyBinding> + '_ {
        self.display_shortcut
            .into_iter()
            .chain(self.additional_shortcuts.iter().copied())
    }
}

fn exec_options(agent_display_name: &str) -> Vec<ApprovalOption> {
    let display_name = if agent_display_name.is_empty() {
        "the agent"
    } else {
        agent_display_name
    };
    vec![
        ApprovalOption {
            label: "Yes, proceed".to_string(),
            decision: ApprovalDecision::Review(ReviewDecision::Approved),
            display_shortcut: None,
            additional_shortcuts: vec![key_hint::plain(KeyCode::Char('y'))],
        },
        ApprovalOption {
            label: "Yes, and don't ask again for this command".to_string(),
            decision: ApprovalDecision::Review(ReviewDecision::ApprovedForSession),
            display_shortcut: None,
            additional_shortcuts: vec![key_hint::plain(KeyCode::Char('a'))],
        },
        ApprovalOption {
            label: format!("No, and tell {display_name} what to do differently"),
            decision: ApprovalDecision::Review(ReviewDecision::Abort),
            display_shortcut: Some(key_hint::plain(KeyCode::Esc)),
            additional_shortcuts: vec![key_hint::plain(KeyCode::Char('n'))],
        },
    ]
}

fn patch_options(agent_display_name: &str) -> Vec<ApprovalOption> {
    let display_name = if agent_display_name.is_empty() {
        "the agent"
    } else {
        agent_display_name
    };
    vec![
        ApprovalOption {
            label: "Yes, proceed".to_string(),
            decision: ApprovalDecision::Review(ReviewDecision::Approved),
            display_shortcut: None,
            additional_shortcuts: vec![key_hint::plain(KeyCode::Char('y'))],
        },
        ApprovalOption {
            label: format!("No, and tell {display_name} what to do differently"),
            decision: ApprovalDecision::Review(ReviewDecision::Abort),
            display_shortcut: Some(key_hint::plain(KeyCode::Esc)),
            additional_shortcuts: vec![key_hint::plain(KeyCode::Char('n'))],
        },
    ]
}

fn elicitation_options() -> Vec<ApprovalOption> {
    vec![
        ApprovalOption {
            label: "Yes, provide the requested info".to_string(),
            decision: ApprovalDecision::McpElicitation(ElicitationAction::Accept),
            display_shortcut: None,
            additional_shortcuts: vec![key_hint::plain(KeyCode::Char('y'))],
        },
        ApprovalOption {
            label: "No, but continue without it".to_string(),
            decision: ApprovalDecision::McpElicitation(ElicitationAction::Decline),
            display_shortcut: None,
            additional_shortcuts: vec![key_hint::plain(KeyCode::Char('n'))],
        },
        ApprovalOption {
            label: "Cancel this request".to_string(),
            decision: ApprovalDecision::McpElicitation(ElicitationAction::Cancel),
            display_shortcut: Some(key_hint::plain(KeyCode::Esc)),
            additional_shortcuts: vec![key_hint::plain(KeyCode::Char('c'))],
        },
    ]
}

fn acp_tool_options(agent_display_name: &str) -> Vec<ApprovalOption> {
    let display_name = if agent_display_name.is_empty() {
        "the agent"
    } else {
        agent_display_name
    };
    vec![
        ApprovalOption {
            label: "Yes, proceed".to_string(),
            decision: ApprovalDecision::Review(ReviewDecision::Approved),
            display_shortcut: None,
            additional_shortcuts: vec![key_hint::plain(KeyCode::Char('y'))],
        },
        ApprovalOption {
            label: "Yes, and don't ask again for this tool".to_string(),
            decision: ApprovalDecision::Review(ReviewDecision::ApprovedForSession),
            display_shortcut: None,
            additional_shortcuts: vec![key_hint::plain(KeyCode::Char('a'))],
        },
        ApprovalOption {
            label: format!("No, and tell {display_name} what to do differently"),
            decision: ApprovalDecision::Review(ReviewDecision::Abort),
            display_shortcut: Some(key_hint::plain(KeyCode::Esc)),
            additional_shortcuts: vec![key_hint::plain(KeyCode::Char('n'))],
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_event::AppEvent;
    use pretty_assertions::assert_eq;
    use tokio::sync::mpsc::unbounded_channel;

    fn make_exec_request() -> ApprovalRequest {
        ApprovalRequest::Exec {
            id: "test".to_string(),
            command: vec!["echo".to_string(), "hi".to_string()],
            reason: Some("reason".to_string()),
            risk: None,
        }
    }

    #[test]
    fn ctrl_c_aborts_and_clears_queue() {
        let (tx, _rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx);
        let mut view = ApprovalOverlay::new(make_exec_request(), tx, String::new());
        view.enqueue_request(make_exec_request());
        assert_eq!(CancellationEvent::Handled, view.on_ctrl_c());
        assert!(view.queue.is_empty());
        assert!(view.is_complete());
    }

    #[test]
    fn shortcut_triggers_selection() {
        let (tx, mut rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx);
        let mut view = ApprovalOverlay::new(make_exec_request(), tx, String::new());
        assert!(!view.is_complete());
        view.handle_key_event(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE));
        // We expect at least one CodexOp message in the queue.
        let mut saw_op = false;
        while let Ok(ev) = rx.try_recv() {
            if matches!(ev, AppEvent::CodexOp(_)) {
                saw_op = true;
                break;
            }
        }
        assert!(saw_op, "expected approval decision to emit an op");
    }

    #[test]
    fn header_includes_command_snippet() {
        let (tx, _rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx);
        let command = vec!["echo".into(), "hello".into(), "world".into()];
        let exec_request = ApprovalRequest::Exec {
            id: "test".into(),
            command,
            reason: None,
            risk: None,
        };

        let view = ApprovalOverlay::new(exec_request, tx, String::new());
        let mut buf = Buffer::empty(Rect::new(0, 0, 80, view.desired_height(80)));
        view.render(Rect::new(0, 0, 80, view.desired_height(80)), &mut buf);

        let rendered: Vec<String> = (0..buf.area.height)
            .map(|row| {
                (0..buf.area.width)
                    .map(|col| buf[(col, row)].symbol().to_string())
                    .collect()
            })
            .collect();
        assert!(
            rendered
                .iter()
                .any(|line| line.contains("echo hello world")),
            "expected header to include command snippet, got {rendered:?}"
        );
    }

    #[test]
    fn exec_history_cell_wraps_with_two_space_indent() {
        let command = vec![
            "/bin/zsh".into(),
            "-lc".into(),
            "git add tui/src/render/mod.rs tui/src/render/renderable.rs".into(),
        ];
        let cell = history_cell::new_approval_decision_cell(command, ReviewDecision::Approved);
        let lines = cell.display_lines(28);
        let rendered: Vec<String> = lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect();
        let expected = vec![
            "✔ You approved Nori to".to_string(),
            "  rungit add tui/src/render/".to_string(),
            "  mod.rs tui/src/render/".to_string(),
            "  renderable.rs this time".to_string(),
        ];
        assert_eq!(rendered, expected);
    }

    #[test]
    fn enter_sets_last_selected_index_without_dismissing() {
        let (tx_raw, mut rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx_raw);
        let mut view = ApprovalOverlay::new(make_exec_request(), tx, String::new());
        view.handle_key_event(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        view.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        assert!(
            view.is_complete(),
            "exec approval should complete without queued requests"
        );

        let mut decision = None;
        while let Ok(ev) = rx.try_recv() {
            if let AppEvent::CodexOp(Op::ExecApproval { decision: d, .. }) = ev {
                decision = Some(d);
                break;
            }
        }
        assert_eq!(decision, Some(ReviewDecision::ApprovedForSession));
    }

    #[test]
    fn exec_approval_shows_model_name_in_deny_option() {
        let (tx, _rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx);
        let exec_request = ApprovalRequest::Exec {
            id: "test".into(),
            command: vec!["echo".into(), "test".into()],
            reason: None,
            risk: None,
        };

        let view = ApprovalOverlay::new(exec_request, tx, "Claude".to_string());
        let mut buf = Buffer::empty(Rect::new(0, 0, 80, view.desired_height(80)));
        view.render(Rect::new(0, 0, 80, view.desired_height(80)), &mut buf);

        let rendered: Vec<String> = (0..buf.area.height)
            .map(|row| {
                (0..buf.area.width)
                    .map(|col| buf[(col, row)].symbol().to_string())
                    .collect()
            })
            .collect();

        assert!(
            rendered.iter().any(|line| line.contains("tell Claude")),
            "expected deny option to include model name 'Claude', got {rendered:?}"
        );
        assert!(
            !rendered.iter().any(|line| line.contains("tell Codex")),
            "should not contain hardcoded 'Codex', got {rendered:?}"
        );
    }

    #[test]
    fn patch_approval_shows_model_name_in_deny_option() {
        let (tx, _rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx);
        let patch_request = ApprovalRequest::ApplyPatch {
            id: "test".into(),
            reason: None,
            cwd: PathBuf::from("/tmp"),
            changes: HashMap::new(),
        };

        let view = ApprovalOverlay::new(patch_request, tx, "Gemini".to_string());
        let mut buf = Buffer::empty(Rect::new(0, 0, 80, view.desired_height(80)));
        view.render(Rect::new(0, 0, 80, view.desired_height(80)), &mut buf);

        let rendered: Vec<String> = (0..buf.area.height)
            .map(|row| {
                (0..buf.area.width)
                    .map(|col| buf[(col, row)].symbol().to_string())
                    .collect()
            })
            .collect();

        assert!(
            rendered.iter().any(|line| line.contains("tell Gemini")),
            "expected deny option to include model name 'Gemini', got {rendered:?}"
        );
        assert!(
            !rendered.iter().any(|line| line.contains("tell Codex")),
            "should not contain hardcoded 'Codex', got {rendered:?}"
        );
    }

    #[test]
    fn approval_overlay_uses_fallback_for_empty_model_name() {
        let (tx, _rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx);
        let exec_request = ApprovalRequest::Exec {
            id: "test".into(),
            command: vec!["echo".into(), "test".into()],
            reason: None,
            risk: None,
        };

        let view = ApprovalOverlay::new(exec_request, tx, "".to_string());
        let mut buf = Buffer::empty(Rect::new(0, 0, 80, view.desired_height(80)));
        view.render(Rect::new(0, 0, 80, view.desired_height(80)), &mut buf);

        let rendered: Vec<String> = (0..buf.area.height)
            .map(|row| {
                (0..buf.area.width)
                    .map(|col| buf[(col, row)].symbol().to_string())
                    .collect()
            })
            .collect();

        assert!(
            rendered.iter().any(|line| line.contains("tell the agent")),
            "expected deny option to use fallback 'the agent' for empty model name, got {rendered:?}"
        );
    }

    fn make_acp_tool_request() -> ApprovalRequest {
        ApprovalRequest::AcpTool {
            call_id: "call-1".to_string(),
            title: "Read /src/main.rs".to_string(),
            kind: nori_protocol::ToolKind::Read,
            cwd: std::path::PathBuf::from("."),
            snapshot: Box::new(nori_protocol::ToolSnapshot {
                call_id: "call-1".to_string(),
                title: "Read /src/main.rs".to_string(),
                kind: nori_protocol::ToolKind::Read,
                phase: nori_protocol::ToolPhase::PendingApproval,
                locations: vec![],
                invocation: Some(nori_protocol::Invocation::Read {
                    path: std::path::PathBuf::from("/src/main.rs"),
                }),
                artifacts: vec![],
                raw_input: None,
                raw_output: None,
            }),
        }
    }

    #[test]
    fn acp_tool_shortcut_triggers_selection() {
        let (tx, mut rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx);
        let mut view = ApprovalOverlay::new(make_acp_tool_request(), tx, String::new());
        assert!(!view.is_complete());

        // Press 'y' to approve
        view.handle_key_event(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE));

        // Should emit an ExecApproval Op with Approved decision
        let mut decision = None;
        while let Ok(ev) = rx.try_recv() {
            if let AppEvent::CodexOp(Op::ExecApproval { decision: d, .. }) = ev {
                decision = Some(d);
                break;
            }
        }
        assert_eq!(
            decision,
            Some(ReviewDecision::Approved),
            "expected AcpTool 'y' shortcut to emit Approved decision"
        );
    }

    #[test]
    fn acp_tool_ctrl_c_aborts() {
        let (tx, mut rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx);
        let mut view = ApprovalOverlay::new(make_acp_tool_request(), tx, String::new());
        view.enqueue_request(make_acp_tool_request());

        assert_eq!(CancellationEvent::Handled, view.on_ctrl_c());
        assert!(view.is_complete());

        // Should emit an ExecApproval Op with Abort decision
        let mut decision = None;
        while let Ok(ev) = rx.try_recv() {
            if let AppEvent::CodexOp(Op::ExecApproval { decision: d, .. }) = ev {
                decision = Some(d);
                break;
            }
        }
        assert_eq!(
            decision,
            Some(ReviewDecision::Abort),
            "expected Ctrl+C on AcpTool to emit Abort decision"
        );
    }

    #[test]
    fn acp_tool_header_includes_tool_title() {
        let (tx, _rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx);

        let view = ApprovalOverlay::new(make_acp_tool_request(), tx, String::new());
        let mut buf = Buffer::empty(Rect::new(0, 0, 80, view.desired_height(80)));
        view.render(Rect::new(0, 0, 80, view.desired_height(80)), &mut buf);

        let rendered: Vec<String> = (0..buf.area.height)
            .map(|row| {
                (0..buf.area.width)
                    .map(|col| buf[(col, row)].symbol().to_string())
                    .collect()
            })
            .collect();
        assert!(
            rendered
                .iter()
                .any(|line| line.contains("Would you like to allow")),
            "expected AcpTool header to contain 'Would you like to allow', got {rendered:?}"
        );
        assert!(
            rendered
                .iter()
                .any(|line| line.contains("Read /src/main.rs")),
            "expected AcpTool header to contain tool title, got {rendered:?}"
        );
    }

    #[test]
    fn acp_tool_approved_history_cell_text() {
        let cell = history_cell::new_acp_approval_decision_cell(
            "Read /src/main.rs",
            &nori_protocol::ToolKind::Read,
            ReviewDecision::Approved,
        );
        let lines = cell.display_lines(80);
        let rendered: String = lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            rendered.contains("approved"),
            "expected approved text, got {rendered}"
        );
        assert!(
            rendered.contains("read"),
            "expected tool kind in text, got {rendered}"
        );
        assert!(
            rendered.contains("Read /src/main.rs"),
            "expected tool title in text, got {rendered}"
        );
        assert!(
            rendered.contains("this time"),
            "expected 'this time' for single approval, got {rendered}"
        );
    }

    #[test]
    fn acp_tool_denied_history_cell_text() {
        let cell = history_cell::new_acp_approval_decision_cell(
            "Read /src/main.rs",
            &nori_protocol::ToolKind::Read,
            ReviewDecision::Denied,
        );
        let lines = cell.display_lines(80);
        let rendered: String = lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            rendered.contains("denied"),
            "expected denied text, got {rendered}"
        );
        assert!(
            rendered.contains("read"),
            "expected tool kind, got {rendered}"
        );
    }

    fn make_acp_edit_tool_request() -> ApprovalRequest {
        ApprovalRequest::AcpTool {
            call_id: "call-edit-1".to_string(),
            title: "Edit src/main.rs".to_string(),
            kind: nori_protocol::ToolKind::Edit,
            cwd: std::path::PathBuf::from("."),
            snapshot: Box::new(nori_protocol::ToolSnapshot {
                call_id: "call-edit-1".to_string(),
                title: "Edit src/main.rs".to_string(),
                kind: nori_protocol::ToolKind::Edit,
                phase: nori_protocol::ToolPhase::PendingApproval,
                locations: vec![nori_protocol::ToolLocation {
                    path: std::path::PathBuf::from("src/main.rs"),
                    line: None,
                }],
                invocation: Some(nori_protocol::Invocation::FileChanges {
                    changes: vec![nori_protocol::FileChange {
                        path: std::path::PathBuf::from("src/main.rs"),
                        old_text: Some("fn main() {}\n".to_string()),
                        new_text: "fn main() {\n    println!(\"hello\");\n}\n".to_string(),
                    }],
                }),
                artifacts: vec![nori_protocol::Artifact::Diff(nori_protocol::FileChange {
                    path: std::path::PathBuf::from("src/main.rs"),
                    old_text: Some("fn main() {}\n".to_string()),
                    new_text: "fn main() {\n    println!(\"hello\");\n}\n".to_string(),
                })],
                raw_input: None,
                raw_output: None,
            }),
        }
    }

    #[test]
    fn acp_edit_tool_overlay_shows_diff_content() {
        let (tx, _rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx);

        let view = ApprovalOverlay::new(make_acp_edit_tool_request(), tx, String::new());
        let mut buf = Buffer::empty(Rect::new(0, 0, 80, view.desired_height(80)));
        view.render(Rect::new(0, 0, 80, view.desired_height(80)), &mut buf);

        let rendered: Vec<String> = (0..buf.area.height)
            .map(|row| {
                (0..buf.area.width)
                    .map(|col| buf[(col, row)].symbol().to_string())
                    .collect()
            })
            .collect();

        // The overlay should show "Would you like to allow edit: Edit src/main.rs?"
        assert!(
            rendered
                .iter()
                .any(|line| line.contains("Would you like to allow")),
            "expected AcpTool edit header to contain approval question, got {rendered:?}"
        );
        // The overlay should contain diff change counts (e.g., "+3 -1") from the DiffSummary.
        // This proves the diff artifacts were rendered, not just the title.
        assert!(
            rendered
                .iter()
                .any(|line| line.contains("+3") || line.contains("-1")),
            "expected AcpTool edit overlay to show diff line counts, got {rendered:?}"
        );
    }

    #[test]
    fn acp_edit_tool_has_always_approve_option() {
        let (tx, _rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx);

        let view = ApprovalOverlay::new(make_acp_edit_tool_request(), tx, String::new());
        let mut buf = Buffer::empty(Rect::new(0, 0, 80, view.desired_height(80)));
        view.render(Rect::new(0, 0, 80, view.desired_height(80)), &mut buf);

        let rendered: Vec<String> = (0..buf.area.height)
            .map(|row| {
                (0..buf.area.width)
                    .map(|col| buf[(col, row)].symbol().to_string())
                    .collect()
            })
            .collect();

        // AcpTool gets "don't ask again" option (unlike ApplyPatch which only has Yes/No)
        assert!(
            rendered.iter().any(|line| line.contains("don't ask again")),
            "expected AcpTool edit to have 'don't ask again' option, got {rendered:?}"
        );
    }

    #[test]
    fn acp_edit_tool_approved_history_cell() {
        let cell = history_cell::new_acp_approval_decision_cell(
            "Edit src/main.rs",
            &nori_protocol::ToolKind::Edit,
            ReviewDecision::Approved,
        );
        let lines = cell.display_lines(80);
        let rendered: String = lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            rendered.contains("approved"),
            "expected approved text, got {rendered}"
        );
        assert!(
            rendered.contains("edit"),
            "expected 'edit' tool kind in text, got {rendered}"
        );
        assert!(
            rendered.contains("Edit src/main.rs"),
            "expected tool title in text, got {rendered}"
        );
    }

    // --- Spec 13: Approval title path relativization ---

    #[test]
    fn acp_tool_overlay_relativizes_absolute_path_in_title() {
        let (tx, _rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx);

        let request = ApprovalRequest::AcpTool {
            call_id: "call-abs".to_string(),
            title: "Edit /home/user/project/README.md".to_string(),
            kind: nori_protocol::ToolKind::Edit,
            cwd: std::path::PathBuf::from("/home/user/project"),
            snapshot: Box::new(nori_protocol::ToolSnapshot {
                call_id: "call-abs".to_string(),
                title: "Edit /home/user/project/README.md".to_string(),
                kind: nori_protocol::ToolKind::Edit,
                phase: nori_protocol::ToolPhase::PendingApproval,
                locations: vec![nori_protocol::ToolLocation {
                    path: std::path::PathBuf::from("/home/user/project/README.md"),
                    line: None,
                }],
                invocation: Some(nori_protocol::Invocation::FileChanges {
                    changes: vec![nori_protocol::FileChange {
                        path: std::path::PathBuf::from("/home/user/project/README.md"),
                        old_text: Some("old\n".to_string()),
                        new_text: "new\n".to_string(),
                    }],
                }),
                artifacts: vec![nori_protocol::Artifact::Diff(nori_protocol::FileChange {
                    path: std::path::PathBuf::from("/home/user/project/README.md"),
                    old_text: Some("old\n".to_string()),
                    new_text: "new\n".to_string(),
                })],
                raw_input: None,
                raw_output: None,
            }),
        };

        let view = ApprovalOverlay::new(request, tx, String::new());
        let mut buf = Buffer::empty(Rect::new(0, 0, 80, view.desired_height(80)));
        view.render(Rect::new(0, 0, 80, view.desired_height(80)), &mut buf);

        let rendered: Vec<String> = (0..buf.area.height)
            .map(|row| {
                (0..buf.area.width)
                    .map(|col| buf[(col, row)].symbol().to_string())
                    .collect()
            })
            .collect();

        // The prompt title line should show relative path, not absolute.
        // Find the "Would you like to allow" line and verify it uses the relative path.
        let prompt_line = rendered
            .iter()
            .find(|line| line.contains("Would you like to allow"))
            .expect("expected approval prompt line");
        assert!(
            !prompt_line.contains("/home/user/project/README.md"),
            "Approval prompt should not show absolute path, got: {prompt_line}"
        );
        assert!(
            prompt_line.contains("README.md"),
            "Approval prompt should show relative path, got: {prompt_line}"
        );
    }

    #[test]
    fn acp_decision_cell_uses_relativized_title_from_overlay() {
        // The overlay relativizes the title before storing it in ApprovalVariant.
        // When the decision cell is created, it receives the already-relativized title.
        // Verify this by constructing a cell with a pre-relativized title.
        let cell = history_cell::new_acp_approval_decision_cell(
            "Edit README.md",
            &nori_protocol::ToolKind::Edit,
            ReviewDecision::Approved,
        );
        let lines = cell.display_lines(80);
        let rendered: String = lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");

        assert!(
            rendered.contains("approved"),
            "expected approved text, got {rendered}"
        );
        assert!(
            rendered.contains("Edit README.md"),
            "expected relativized title in decision cell, got {rendered}"
        );
    }
}
