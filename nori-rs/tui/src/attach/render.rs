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
    let identity = state
        .selected
        .as_ref()
        .map(|id| {
            let row = state.sessions.iter().find(|row| &row.session_id == id);
            format!(
                " {} · {} · {}",
                row.map_or("session", |row| row.agent.as_str()),
                id,
                row.map_or("", |row| crate::session_status::status_label(row.status))
            )
        })
        .unwrap_or_else(|| " Sessions · /new starts a session · ctrl+d detaches".into());
    Line::from(identity.dim()).render(Rect::new(area.x, area.y + 1, area.width, 1), buf);
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
        area.height.saturating_sub(permission_height),
    );
    let mut lines = Vec::new();
    for part in &conversation.parts {
        match part {
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
