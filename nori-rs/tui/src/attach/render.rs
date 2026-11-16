//! Terminal views shared by live attachments and snapshot tests.

use nori_tui_components::KeyHint;
use nori_tui_components::Picker;
use nori_tui_components::PickerDensity;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;

use super::state::AttachState;
use super::state::Part;
use crate::history_cell::HistoryCell;
use crate::presentation::MessageStream;

pub(super) fn composer_area(
    state: &AttachState,
    composer: &crate::ComposerInput,
    area: Rect,
) -> Rect {
    let reserved = u16::try_from(notice_lines(state, area.width).len())
        .unwrap_or(u16::MAX)
        .min(area.height.saturating_sub(6));
    let height = composer
        .desired_height(area.width)
        .max(3)
        .min(area.height.saturating_sub(3 + reserved));
    Rect::new(
        area.x,
        area.bottom().saturating_sub(height),
        area.width,
        height,
    )
}

pub(super) fn render(
    state: &AttachState,
    composer: &crate::ComposerInput,
    area: Rect,
    buf: &mut Buffer,
) {
    if area.height < 4 || area.width < 12 {
        return;
    }
    let mut header = vec![" Nori host  ".bold()];
    if state.stopped.is_some() {
        header.push("DISCONNECTED · ctrl+d to detach".red());
    } else if state.connected {
        header.push(if state.watching {
            state.role_label().cyan()
        } else {
            state.role_label().green()
        });
    } else {
        header.push("RECONNECTING · sessions continue on the host".cyan());
    }
    Line::from(header).render(Rect::new(area.x, area.y, area.width, 1), buf);
    identity_line(state, area.width).render(Rect::new(area.x, area.y + 1, area.width, 1), buf);
    if let Some(line) = waiting_line(state, area.width) {
        line.render(Rect::new(area.x, area.y + 2, area.width, 1), buf);
    }
    let input = composer_area(state, composer, area);
    let notice_lines = notice_lines(state, area.width);
    let notice_height = u16::try_from(notice_lines.len())
        .unwrap_or(u16::MAX)
        .min(input.y.saturating_sub(area.y + 3));
    let body = Rect::new(
        area.x,
        area.y + 3,
        area.width,
        input.y.saturating_sub(area.y + 3 + notice_height),
    );
    Paragraph::new(notice_lines).render(
        Rect::new(
            area.x,
            input.y.saturating_sub(notice_height),
            area.width,
            notice_height,
        ),
        buf,
    );
    if state.list_open {
        if state.sessions.is_empty() {
            Paragraph::new(vec![
                Line::from(" Sessions 0".bold()),
                Line::default(),
                Line::from(" No sessions yet. Type /new and press Enter."),
                Line::from(" The host keeps running when you detach.".dim()),
            ])
            .render(body, buf);
        } else {
            Picker::new(&state.picker)
                .theme(crate::style::component_theme())
                .density(PickerDensity::Compact)
                .show_details(false)
                .horizontal_margin(0)
                .selection_keeps_tones(true)
                .footer_hints([
                    KeyHint::new("↑↓/j/k", "move"),
                    KeyHint::new("enter", "open"),
                    KeyHint::new("ctrl+r", "resume"),
                    KeyHint::new("/", "search"),
                    KeyHint::new("esc", "back"),
                ])
                .render(body, buf);
        }
    } else if let Some(id) = &state.selected {
        render_conversation(state, id, body, buf);
    } else {
        Paragraph::new(" No session selected. Use /sessions to browse or /new to start.")
            .render(body, buf);
    }
    // A request on screen has the keyboard; the draft waits hidden.
    if !state.request_on_screen() || state.list_open {
        composer.render_ref(input, buf);
    }
}

/// Where the terminal cursor goes: in the composer, unless a request has the
/// keyboard.
pub(super) fn cursor_position(
    state: &AttachState,
    composer: &crate::ComposerInput,
    area: Rect,
) -> Option<(u16, u16)> {
    if state.request_on_screen() && !state.list_open {
        return None;
    }
    composer.cursor_pos(composer_area(state, composer, area))
}

/// A listed session's name, whole: the lines that show it fit it to the
/// terminal width.
fn row_name(row: &super::state::SessionRow) -> String {
    row.title
        .as_deref()
        .and_then(nori_protocol::session_full_name)
        .unwrap_or_else(|| nori_protocol::UNTITLED_SESSION_NAME.to_string())
}

/// The selected session's agent, name, and status. Inside a delegated child
/// it is a breadcrumb from the top-level session, with the key that returns
/// to the parent.
fn identity_line(state: &AttachState, width: u16) -> Line<'static> {
    let Some(id) = state.selected.as_ref() else {
        return Line::from(" Sessions · /new starts a session · ctrl+d detaches".dim());
    };
    let row = |id: &str| state.sessions.iter().find(|row| row.session_id == id);
    let Some(current) = row(id) else {
        return Line::from(format!(" session · {id}").dim());
    };
    let top_level = current.delegating_parent().and_then(row).is_none();
    // A top-level session speaks for its family, as in the session lists.
    let shown = if top_level {
        current.family_status.unwrap_or(current.status)
    } else {
        current.status
    };
    let status = format!(" · {}", crate::session_status::status_label(shown));
    let mut path = vec![current];
    while let Some(parent) = path
        .last()
        .and_then(|row| row.delegating_parent())
        .and_then(row)
        .filter(|parent| !path.iter().any(|seen| seen.session_id == parent.session_id))
    {
        path.push(parent);
    }
    let path: Vec<crate::session_path::PathStep> = path
        .into_iter()
        .rev()
        .map(|row| crate::session_path::PathStep {
            agent: row.agent.clone(),
            name: row_name(row),
        })
        .collect();
    let key = if top_level || state.parent_binding.is_none() {
        String::new()
    } else {
        format!(" · {} parent", state.parent_binding.as_str())
    };
    let mut line =
        crate::session_path::breadcrumb_line(&path, &status, &key, width.saturating_sub(1));
    line.spans.insert(0, " ".into());
    line
}

/// The next session, other than the one on screen, that waits on the user,
/// named under its parent, with the key that jumps to it.
fn waiting_line(state: &AttachState, width: u16) -> Option<Line<'static>> {
    let row = |id: &str| state.sessions.iter().find(|row| row.session_id == id);
    let waiting: Vec<&super::state::SessionRow> = state
        .sessions
        .iter()
        .filter(|row| row.status == nori_protocol::SessionStatus::WaitingOnYou)
        .filter(|row| state.selected.as_deref() != Some(row.session_id.as_str()))
        .collect();
    let first = waiting.first()?;
    let mut path = Vec::new();
    if let Some(parent) = first.parent.as_deref().and_then(row) {
        path.push(row_name(parent));
    }
    path.push(row_name(first));
    let key = (!state.jump_binding.is_none()).then(|| state.jump_binding.as_str().to_string());
    let mut line = crate::session_path::waiting_line(
        &path,
        None,
        waiting.len() - 1,
        key.as_deref(),
        width.saturating_sub(1),
    );
    line.spans.insert(0, " ".into());
    Some(line)
}

fn notice_lines(state: &AttachState, width: u16) -> Vec<Line<'static>> {
    state
        .stopped
        .as_ref()
        .or(state.replay_error.as_ref())
        .or(state.notice.as_ref())
        .map(|notice| {
            crate::wrapping::word_wrap_lines(
                [Line::from(format!(" {notice}").red())],
                usize::from(width),
            )
        })
        .unwrap_or_default()
}

fn render_conversation(state: &AttachState, id: &str, area: Rect, buf: &mut Buffer) {
    let Some(conversation) = state.conversations.get(id) else {
        Paragraph::new(" Waiting for session history…").render(area, buf);
        return;
    };
    let delegation = conversation
        .delegations
        .iter()
        .find(|delegation| delegation.status == nori_protocol::DelegationStatus::AwaitingApproval);
    let delegation_lines = delegation
        .map(|delegation| {
            let (title, rows) = crate::delegation::approval_lines(delegation);
            let mut lines = vec![Line::from(title.bold())];
            lines.extend(rows);
            lines.push(Line::from(vec![
                "1 Yes, start it".green(),
                " · ".dim(),
                "2 No, decline".into(),
                " · esc or ctrl+c declines".dim(),
            ]));
            crate::wrapping::word_wrap_lines(
                lines.iter(),
                usize::from(area.width.saturating_sub(2)),
            )
        })
        .unwrap_or_default();
    let delegation_height = u16::try_from(delegation_lines.len())
        .unwrap_or(u16::MAX)
        .min(area.height);
    let permission = conversation.permissions.first();
    let permission_height = permission.map_or(0, |request| {
        u16::try_from(request.options.len() + 4)
            .unwrap_or(u16::MAX)
            .min(area.height)
    });
    let history_area = Rect::new(
        area.x + 1,
        area.y,
        area.width.saturating_sub(2),
        area.height
            .saturating_sub(permission_height + delegation_height),
    );
    let mut lines = Vec::new();
    for part in &conversation.parts {
        match part {
            Part::Message {
                stream: MessageStream::User,
                text,
                ..
            } if nori_protocol::delegation_result_parts(text).is_some() => {
                let mut line = Line::from(vec!["› ".bold()]);
                line.spans.extend(
                    crate::delegation::delivered_result_spans(
                        text,
                        usize::from(history_area.width).saturating_sub(2),
                    )
                    .unwrap_or_default(),
                );
                lines.push(line);
            }
            Part::Message { stream, text, .. } => {
                match stream {
                    MessageStream::User => lines.push(Line::from("› You".bold())),
                    MessageStream::Reasoning => lines.push(Line::from("Thinking".dim())),
                    MessageStream::Answer => {}
                }
                crate::markdown::append_markdown_with_cwd(
                    text,
                    Some(usize::from(history_area.width)),
                    Some(&conversation.cwd),
                    &mut lines,
                );
            }
            Part::Tool(snapshot) => {
                let cell = crate::client_tool_cell::ClientToolCell::new(
                    (**snapshot).clone(),
                    conversation.cwd.clone(),
                    false,
                );
                lines.extend(cell.display_lines(history_area.width));
            }
            Part::Plan(plan) => {
                lines.push(Line::from("Plan".bold()));
                for entry in &plan.entries {
                    let marker = match entry.status {
                        crate::presentation::PlanStatus::Completed => "✓",
                        crate::presentation::PlanStatus::InProgress => "›",
                        crate::presentation::PlanStatus::Pending => "·",
                    };
                    lines.push(Line::from(format!("{marker} {}", entry.step)));
                }
            }
            Part::Delegation(delegation) => {
                lines.extend(
                    super::state::delegation_cell(delegation).display_lines(history_area.width),
                );
            }
            Part::Notice(notice) => {
                lines.extend(crate::wrapping::word_wrap_lines(
                    [Line::from(notice.clone().cyan())],
                    usize::from(history_area.width),
                ));
            }
            Part::Ended(message) => {
                lines.extend(crate::wrapping::word_wrap_lines(
                    [Line::from(state.end_notice(id, message.as_deref()).cyan())],
                    usize::from(history_area.width),
                ));
            }
        }
        lines.push(Line::default());
    }
    if state.transcript_text(id).is_empty() {
        lines.push(Line::from("Ready. Send a prompt to begin.".dim()));
    }
    let start = lines
        .len()
        .saturating_sub(usize::from(history_area.height))
        .saturating_sub(state.scroll);
    Paragraph::new(
        lines
            .into_iter()
            .skip(start)
            .take(usize::from(history_area.height))
            .collect::<Vec<_>>(),
    )
    .render(history_area, buf);
    if delegation_height > 0 {
        Paragraph::new(delegation_lines).render(
            Rect::new(
                area.x + 1,
                area.bottom()
                    .saturating_sub(permission_height + delegation_height),
                area.width.saturating_sub(2),
                delegation_height,
            ),
            buf,
        );
    }
    if let Some(permission) = permission {
        let mut rows = vec![Line::from(
            format!("Permission · {}", permission.title).bold(),
        )];
        for (index, option) in permission.options.iter().enumerate() {
            let prefix = if permission.selected == index {
                "›"
            } else {
                " "
            };
            rows.push(Line::from(vec![
                format!("{prefix} {}  ", index + 1).green(),
                option.name.clone().into(),
            ]));
        }
        rows.push(Line::from(if state.watching {
            "Watching · read-only · alt+t to take control".cyan()
        } else {
            "↑↓ choose · enter answer · number selects · esc reject · ctrl+c cancel".dim()
        }));
        if state.permission_count(id) > 1 {
            rows.push(Line::from(
                format!("{} requests waiting", state.permission_count(id)).dim(),
            ));
        }
        Paragraph::new(rows).render(
            Rect::new(
                area.x + 1,
                area.bottom().saturating_sub(permission_height),
                area.width.saturating_sub(2),
                permission_height,
            ),
            buf,
        );
    }
}
