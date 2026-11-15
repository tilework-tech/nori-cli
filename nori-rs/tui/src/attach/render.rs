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
use unicode_width::UnicodeWidthStr;

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
                .fullscreen_selection_rails(true)
                .footer_hints([
                    KeyHint::new("↑↓", "move"),
                    KeyHint::new("enter", "open"),
                    KeyHint::new("ctrl+r", "resume"),
                    KeyHint::new("ctrl+f", "search"),
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
    composer.render_ref(input, buf);
}

/// The selected session's agent, name, and status. Inside a delegated child
/// it is a breadcrumb from the parent with the key that returns there.
fn identity_line(state: &AttachState, width: u16) -> Line<'static> {
    let Some(id) = state.selected.as_ref() else {
        return Line::from(" Sessions · /new starts a session · ctrl+d detaches".dim());
    };
    let row = |id: &str| state.sessions.iter().find(|row| row.session_id == id);
    let Some(current) = row(id) else {
        return Line::from(format!(" session · {id}").dim());
    };
    let name = |row: &super::state::SessionRow| {
        row.title
            .as_deref()
            .and_then(nori_protocol::session_display_name)
            .unwrap_or_else(|| row.session_id.chars().take(8).collect())
    };
    let status = format!(" · {}", crate::session_status::status_label(current.status));
    let Some(parent) = current.delegating_parent().and_then(row) else {
        return Line::from(vec![
            " ".into(),
            format!("{} · ", current.agent).dim(),
            name(current).bold(),
            status.dim(),
        ]);
    };
    let key = if state.parent_binding.is_none() {
        String::new()
    } else {
        format!(" · {} parent", state.parent_binding.as_str())
    };
    let parent_agent = format!("{} · ", parent.agent);
    let child_agent = format!("{} · ", current.agent);
    // The two names share whatever the agents, status, and parent key leave.
    let fixed = [" ", &parent_agent, " › ", &child_agent, &status, &key]
        .iter()
        .map(|part| part.width())
        .sum::<usize>();
    let room = usize::from(width).saturating_sub(fixed) / 2;
    let fit = |name: String| {
        if name.width() <= room {
            return name;
        }
        let mut kept = String::new();
        for ch in name.chars() {
            if kept.width() + ch.to_string().width() + 1 > room {
                break;
            }
            kept.push(ch);
        }
        format!("{}…", kept.trim_end())
    };
    Line::from(vec![
        " ".into(),
        parent_agent.dim(),
        fit(name(parent)).into(),
        " › ".dim(),
        child_agent.dim(),
        fit(name(current)).bold(),
        status.dim(),
        key.dim(),
    ])
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
            } if nori_protocol::delegation_result_summary(text).is_some() => {
                let summary = nori_protocol::delegation_result_summary(text).unwrap_or_default();
                lines.push(Line::from(vec![
                    "› ".bold(),
                    "delegation result ".dim(),
                    summary.to_owned().into(),
                    " · delivered to this session".dim(),
                ]));
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
            "↑↓ choose · enter answer · number selects an option".dim()
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
