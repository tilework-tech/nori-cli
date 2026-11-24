//! `/approvals` in an attached terminal: the host's modes for the session on
//! screen. The host owns the mode; this terminal shows what the host
//! announces and asks it for changes.

use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use nori_harness::permission_mode::PermissionMode;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::widgets::Clear;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;

use super::runtime::Pending;
use super::runtime::Runtime;
use super::state::AttachState;
use crate::host_client::HostCall;

pub(super) const TITLE: &str = "Approvals for this session";

/// The open mode menu, for the session it was opened on.
pub(super) struct ModeMenu {
    pub session_id: String,
    pub selected: usize,
    /// Approve all waits for a second Enter.
    pub confirming: bool,
}

impl AttachState {
    /// The mode the host last announced for `session_id`.
    /// A live announcement wins; the list row covers a session whose
    /// announcement left the host's replay window.
    pub fn approval_mode(&self, session_id: &str) -> Option<PermissionMode> {
        self.conversations
            .get(session_id)
            .and_then(|conversation| conversation.approval_mode)
            .or_else(|| {
                self.sessions
                    .iter()
                    .find(|row| row.session_id == session_id)
                    .and_then(|row| row.approval_mode)
            })
            .map(PermissionMode::from)
    }

    /// The menu is drawn and takes keys only while no request needs an answer.
    pub fn mode_menu_on_screen(&self) -> bool {
        self.mode_menu.is_some() && !self.request_on_screen()
    }
}

impl Runtime {
    /// `/approvals` with no argument opens the menu; a mode id sets it.
    pub(super) fn approvals_command(&mut self, session_id: String, args: &str, draft: String) {
        let wanted = args.trim();
        let position = |mode: PermissionMode| {
            PermissionMode::ALL
                .iter()
                .position(|candidate| *candidate == mode)
                .unwrap_or_default()
        };
        if wanted.is_empty() {
            let current = self
                .state
                .approval_mode(&session_id)
                .unwrap_or(PermissionMode::Agent);
            self.state.mode_menu = Some(ModeMenu {
                session_id,
                selected: position(current),
                confirming: false,
            });
            return;
        }
        match PermissionMode::from_id(wanted) {
            Some(PermissionMode::ApproveAll) => {
                self.state.mode_menu = Some(ModeMenu {
                    session_id,
                    selected: position(PermissionMode::ApproveAll),
                    confirming: true,
                });
            }
            Some(mode) => self.set_mode(session_id, mode),
            None => {
                self.composer.set_text(draft);
                self.state.show_notice(format!(
                    "/approvals: unknown mode {wanted}; use ask, agent or approve-all"
                ));
            }
        }
    }

    fn set_mode(&mut self, session_id: String, mode: PermissionMode) {
        self.send(
            HostCall::SetMode {
                session_id,
                mode: mode.into(),
            },
            Pending::Mutation { draft: None },
        );
    }

    /// Keys while the menu is open. True when the menu took the key.
    /// Other keys, such as Alt+T and session moves, keep their meaning.
    pub(super) fn mode_menu_key(&mut self, key: KeyEvent) -> bool {
        if !self.state.mode_menu_on_screen() || !key.modifiers.is_empty() {
            return false;
        }
        let Some(menu) = self.state.mode_menu.as_mut() else {
            return false;
        };
        let last = PermissionMode::ALL.len() - 1;
        match key.code {
            KeyCode::Up | KeyCode::Char('k') if !menu.confirming => {
                menu.selected = menu.selected.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') if !menu.confirming => {
                menu.selected = (menu.selected + 1).min(last);
            }
            KeyCode::Esc if menu.confirming => menu.confirming = false,
            KeyCode::Esc => self.state.mode_menu = None,
            KeyCode::Enter
                if PermissionMode::ALL[menu.selected] == PermissionMode::ApproveAll
                    && !menu.confirming =>
            {
                menu.confirming = true;
            }
            KeyCode::Enter => {
                if let Some(menu) = self.state.mode_menu.take() {
                    self.set_mode(menu.session_id, PermissionMode::ALL[menu.selected]);
                }
            }
            _ => return false,
        }
        true
    }
}

/// Draw the open menu over the bottom of `area`.
pub(super) fn render_mode_menu(state: &AttachState, area: Rect, buf: &mut Buffer) {
    let Some(menu) = state
        .mode_menu
        .as_ref()
        .filter(|_| state.mode_menu_on_screen())
    else {
        return;
    };
    let current = state.approval_mode(&menu.session_id);
    if menu.confirming {
        let lines = vec![
            Line::from(" Approve every request in this session?".bold()),
            Line::from(
                " Nori will say yes to every edit, command and tool call the agent asks about.",
            ),
            Line::from(
                " The sandbox setting still applies; nothing else stops a harmful request.".red(),
            ),
            Line::default(),
            Line::from(" enter approve all · esc go back".dim()),
        ];
        draw_at_bottom(lines, area, buf);
        return;
    }
    let mut lines = vec![
        Line::from(format!(" {TITLE}").bold()),
        Line::from(" New sessions start in the host default. The sandbox is set separately.".dim()),
        Line::default(),
    ];
    for (index, mode) in PermissionMode::ALL.into_iter().enumerate() {
        let marker = if index == menu.selected {
            " › "
        } else {
            "   "
        };
        let mut name = format!("{marker}{}", mode.name());
        if current == Some(mode) {
            name.push_str(" (current)");
        }
        let name = format!("{name:<26}");
        let name = if index == menu.selected {
            name.cyan().bold()
        } else {
            name.into()
        };
        lines.push(Line::from(vec![name, mode.description().dim()]));
    }
    lines.push(Line::default());
    lines.push(Line::from(" ↑↓ move · enter choose · esc close".dim()));
    draw_at_bottom(lines, area, buf);
}

fn draw_at_bottom(lines: Vec<Line<'static>>, area: Rect, buf: &mut Buffer) {
    let height = u16::try_from(lines.len())
        .unwrap_or(u16::MAX)
        .min(area.height);
    let menu_area = Rect::new(
        area.x,
        area.bottom().saturating_sub(height),
        area.width,
        height,
    );
    Clear.render(menu_area, buf);
    Paragraph::new(lines).render(menu_area, buf);
}
