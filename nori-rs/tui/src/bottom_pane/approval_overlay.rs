use std::path::PathBuf;

use crate::app_event::AppEvent;
use crate::app_event::HarnessAction;
use crate::app_event_sender::AppEventSender;
use crate::bottom_pane::BottomPaneView;
use crate::bottom_pane::CancellationEvent;
use crate::bottom_pane::list_selection_view::ListSelectionView;
use crate::bottom_pane::list_selection_view::SelectionItem;
use crate::bottom_pane::list_selection_view::SelectionViewParams;
use crate::diff_render::DiffSummary;
use crate::history_cell;
use crate::key_hint;
use crate::key_hint::KeyBinding;
use crate::render::renderable::ColumnRenderable;
use crate::render::renderable::Renderable;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use crossterm::event::KeyModifiers;
use nori_protocol::acp::v1 as acp;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Wrap;

/// An ACP permission request awaiting a schema-native client response.
#[derive(Clone, Debug)]
pub(crate) struct ApprovalRequest {
    pub(crate) request_id: acp::RequestId,
    pub(crate) title: String,
    pub(crate) kind: crate::presentation::ToolKind,
    pub(crate) cwd: PathBuf,
    pub(crate) snapshot: Box<crate::presentation::ToolSnapshot>,
    pub(crate) options: Vec<acp::PermissionOption>,
}

pub(crate) struct ApprovalOverlay {
    current_request: Option<ApprovalRequest>,
    queue: Vec<ApprovalRequest>,
    app_event_tx: AppEventSender,
    list: ListSelectionView,
    options: Vec<ApprovalOption>,
    current_complete: bool,
    done: bool,
}

impl ApprovalOverlay {
    pub fn new(
        request: ApprovalRequest,
        app_event_tx: AppEventSender,
        _agent_display_name: String,
    ) -> Self {
        let mut view = Self {
            current_request: None,
            queue: Vec::new(),
            app_event_tx: app_event_tx.clone(),
            list: ListSelectionView::new(Default::default(), app_event_tx),
            options: Vec::new(),
            current_complete: false,
            done: false,
        };
        view.set_current(request);
        view
    }

    pub fn enqueue_request(&mut self, request: ApprovalRequest) {
        self.queue.push(request);
    }

    fn set_current(&mut self, request: ApprovalRequest) {
        let header = approval_header(&request);
        self.options = request
            .options
            .iter()
            .cloned()
            .map(ApprovalOption::from)
            .collect();
        let items = self
            .options
            .iter()
            .map(|option| SelectionItem {
                name: option.label.clone(),
                display_shortcut: option
                    .display_shortcut
                    .or_else(|| option.additional_shortcuts.first().copied()),
                dismiss_on_select: false,
                ..Default::default()
            })
            .collect();
        let kind = crate::client_event_format::format_tool_kind(&request.kind);
        let title = format!("Would you like to allow {kind}: {}?", request.title);
        self.list = ListSelectionView::new(
            SelectionViewParams {
                footer_hint: Some(Line::from(vec![
                    "Press ".into(),
                    key_hint::plain(KeyCode::Enter).into(),
                    " to confirm or ".into(),
                    key_hint::plain(KeyCode::Esc).into(),
                    " to cancel".into(),
                ])),
                items,
                header: Box::new(ColumnRenderable::with([
                    Line::from(title.bold()).into(),
                    Line::from("").into(),
                    header,
                ])),
                ..Default::default()
            },
            self.app_event_tx.clone(),
        );
        self.current_request = Some(request);
        self.current_complete = false;
    }

    fn apply_selection(&mut self, index: usize) {
        if self.current_complete {
            return;
        }
        let (Some(request), Some(option)) =
            (self.current_request.as_ref(), self.options.get(index))
        else {
            return;
        };
        self.app_event_tx.send(AppEvent::InsertHistoryCell(Box::new(
            history_cell::new_info_event(
                format!("Permission: {}", option.label),
                Some(request.title.clone()),
            ),
        )));
        let outcome = acp::RequestPermissionOutcome::Selected(acp::SelectedPermissionOutcome::new(
            option.permission.option_id.clone(),
        ));
        self.app_event_tx
            .send(AppEvent::HarnessAction(HarnessAction::RespondToAgent {
                request_id: request.request_id.clone(),
                response: Ok(acp::ClientResponse::RequestPermissionResponse(
                    acp::RequestPermissionResponse::new(outcome),
                )),
            }));
        self.current_complete = true;
        self.advance_queue();
    }

    fn cancel_current(&self) {
        let Some(request) = self.current_request.as_ref() else {
            return;
        };
        self.app_event_tx
            .send(AppEvent::HarnessAction(HarnessAction::RespondToAgent {
                request_id: request.request_id.clone(),
                response: Ok(acp::ClientResponse::RequestPermissionResponse(
                    acp::RequestPermissionResponse::new(acp::RequestPermissionOutcome::Cancelled),
                )),
            }));
    }

    fn advance_queue(&mut self) {
        if let Some(next) = self.queue.pop() {
            self.set_current(next);
        } else {
            self.done = true;
        }
    }

    fn try_handle_shortcut(&mut self, event: &KeyEvent) -> bool {
        match event {
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
            event => {
                let selected = self.options.iter().position(|option| {
                    option.shortcuts().any(|shortcut| shortcut.is_press(*event))
                });
                if let Some(index) = selected {
                    self.apply_selection(index);
                    true
                } else {
                    false
                }
            }
        }
    }
}

impl BottomPaneView for ApprovalOverlay {
    fn handle_key_event(&mut self, event: KeyEvent) {
        if self.try_handle_shortcut(&event) {
            return;
        }
        self.list.handle_key_event(event);
        if let Some(index) = self.list.take_last_selected_index() {
            self.apply_selection(index);
        }
    }

    fn on_ctrl_c(&mut self) -> CancellationEvent {
        if !self.done && !self.current_complete {
            self.cancel_current();
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

    fn render(&self, area: Rect, buffer: &mut Buffer) {
        self.list.render(area, buffer);
    }

    fn cursor_pos(&self, area: Rect) -> Option<(u16, u16)> {
        self.list.cursor_pos(area)
    }
}

fn approval_header(request: &ApprovalRequest) -> Box<dyn Renderable> {
    let title = crate::client_event_format::relativize_paths_in_text(&request.title, &request.cwd);
    let edit_like = matches!(
        request.kind,
        crate::presentation::ToolKind::Create
            | crate::presentation::ToolKind::Edit
            | crate::presentation::ToolKind::Delete
            | crate::presentation::ToolKind::Move
    );
    if edit_like {
        let mut changes = crate::client_tool_cell::diff_changes_from_artifacts(
            &request.snapshot.artifacts,
            &request.cwd,
        );
        if changes.is_empty() {
            changes = crate::client_tool_cell::changes_from_invocation(
                &request.snapshot.invocation,
                &request.cwd,
            );
        }
        if !changes.is_empty() {
            return DiffSummary::new(changes, request.cwd.clone()).into();
        }
    }

    let mut lines = vec![Line::from(title.clone())];
    if let Some(invocation) =
        crate::client_event_format::format_invocation(&request.snapshot.invocation)
    {
        let invocation =
            crate::client_event_format::relativize_paths_in_text(&invocation, &request.cwd);
        if !crate::client_event_format::is_invocation_redundant(&invocation, &title) {
            lines.push(Line::from(invocation));
        }
    }
    lines.extend(
        crate::client_event_format::format_artifacts(&request.snapshot.artifacts)
            .into_iter()
            .map(Line::from),
    );
    Box::new(Paragraph::new(lines).wrap(Wrap { trim: false }))
}

#[derive(Clone)]
struct ApprovalOption {
    label: String,
    permission: acp::PermissionOption,
    display_shortcut: Option<KeyBinding>,
    additional_shortcuts: Vec<KeyBinding>,
}

impl From<acp::PermissionOption> for ApprovalOption {
    fn from(permission: acp::PermissionOption) -> Self {
        let (display_shortcut, additional_shortcuts) = match permission.kind {
            acp::PermissionOptionKind::AllowOnce => {
                (None, vec![key_hint::plain(KeyCode::Char('y'))])
            }
            acp::PermissionOptionKind::AllowAlways => {
                (None, vec![key_hint::plain(KeyCode::Char('a'))])
            }
            acp::PermissionOptionKind::RejectOnce => (
                Some(key_hint::plain(KeyCode::Esc)),
                vec![key_hint::plain(KeyCode::Char('n'))],
            ),
            acp::PermissionOptionKind::RejectAlways => {
                (None, vec![key_hint::plain(KeyCode::Char('d'))])
            }
            _ => (None, Vec::new()),
        };
        Self {
            label: permission.name.clone(),
            permission,
            display_shortcut,
            additional_shortcuts,
        }
    }
}

impl ApprovalOption {
    fn shortcuts(&self) -> impl Iterator<Item = KeyBinding> + '_ {
        self.display_shortcut
            .into_iter()
            .chain(self.additional_shortcuts.iter().copied())
    }
}
