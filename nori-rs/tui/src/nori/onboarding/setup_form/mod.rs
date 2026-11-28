//! The first-launch setup form: three questions, answerable with one Enter.
//!
//! 1. Default agent (required). Prefilled with the configured agent, else the
//!    first agent Nori can see a sign-in for; with neither, the user picks.
//! 2. Permissions, prefilled with Agent.
//! 3. Usage stats, prefilled with Share.
//!
//! Folder questions (trust, tool output) are not asked here; they belong to
//! the per-folder prompt.

mod detect;
mod save;
#[cfg(test)]
mod tests;

use std::cell::Cell;

use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use nori_auth_providers::AuthStatus;
use nori_harness::AgentKind;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;
use ratatui::widgets::WidgetRef;

use super::KeyboardHandler;
use super::StepState;
use super::StepStateProvider;
pub(crate) use detect::detect_agents;
pub(crate) use save::save;

/// One agent offered as the default, and whether Nori sees a sign-in for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AgentChoice {
    pub kind: AgentKind,
    pub status: AuthStatus,
}

/// The permission mode new sessions start in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Permission {
    Ask,
    Agent,
    ApproveAll,
}

impl Permission {
    const ALL: [Permission; 3] = [Permission::Ask, Permission::Agent, Permission::ApproveAll];

    fn label(self) -> &'static str {
        match self {
            Permission::Ask => "Ask",
            Permission::Agent => "Agent",
            Permission::ApproveAll => "Approve all",
        }
    }

    fn note(self) -> &'static str {
        match self {
            Permission::Ask => "asks before every edit and command",
            Permission::Agent => "the agent decides what to ask you about",
            Permission::ApproveAll => "approves every request without asking",
        }
    }
}

/// What the user chose.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SetupChoices {
    pub agent: AgentKind,
    pub permission: Permission,
    pub share_usage: bool,
}

/// How much room the form takes. Shorter terminals get denser forms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Density {
    /// Every agent listed, with blank rows between sections.
    Full,
    /// [`VISIBLE_AGENTS`] agents at a time, with blank rows between sections.
    Compact,
    /// [`VISIBLE_AGENTS`] agents at a time, no blank rows.
    Tight,
}

/// Agents listed at once by the compact and tight forms.
const VISIBLE_AGENTS: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Row {
    Agent(usize),
    Permission,
    Usage,
}

pub(crate) struct SetupForm {
    agents: Vec<AgentChoice>,
    selected: Option<usize>,
    cursor: Row,
    permission: Permission,
    share_usage: bool,
    missing_agent: bool,
    done: Option<SetupChoices>,
    /// First agent listed when only [`VISIBLE_AGENTS`] fit.
    agent_top: Cell<usize>,
}

impl SetupForm {
    pub(crate) fn new(agents: Vec<AgentChoice>, configured: Option<AgentKind>) -> Self {
        let selected = configured
            .and_then(|kind| agents.iter().position(|a| a.kind == kind))
            .or_else(|| agents.iter().position(|a| a.status == AuthStatus::SignedIn));
        Self {
            cursor: Row::Agent(selected.unwrap_or(0)),
            agents,
            selected,
            permission: Permission::Agent,
            share_usage: true,
            missing_agent: false,
            done: None,
            agent_top: Cell::new(0),
        }
    }

    /// The answers, once the form is submitted.
    pub(crate) fn choices(&self) -> Option<SetupChoices> {
        self.done.clone()
    }

    fn rows(&self) -> Vec<Row> {
        (0..self.agents.len())
            .map(Row::Agent)
            .chain([Row::Permission, Row::Usage])
            .collect()
    }

    fn step(&mut self, by: i32) {
        let rows = self.rows();
        let at = rows.iter().position(|r| *r == self.cursor).unwrap_or(0) as i32;
        let next = (at + by).clamp(0, rows.len() as i32 - 1) as usize;
        self.cursor = rows[next];
    }

    fn change(&mut self, by: i32) {
        match self.cursor {
            Row::Agent(i) => self.selected = Some(i),
            Row::Permission => {
                let at = Permission::ALL
                    .iter()
                    .position(|p| *p == self.permission)
                    .unwrap_or(1) as i32;
                let next = (at + by).rem_euclid(Permission::ALL.len() as i32) as usize;
                self.permission = Permission::ALL[next];
            }
            Row::Usage => self.share_usage = !self.share_usage,
        }
    }

    fn submit(&mut self) {
        if let Row::Agent(i) = self.cursor {
            self.selected = Some(i);
        }
        match self.selected.and_then(|i| self.agents.get(i)) {
            Some(agent) => {
                self.done = Some(SetupChoices {
                    agent: agent.kind,
                    permission: self.permission,
                    share_usage: self.share_usage,
                });
            }
            None => {
                self.missing_agent = true;
                self.cursor = Row::Agent(0);
            }
        }
    }
}

impl KeyboardHandler for SetupForm {
    fn handle_key_event(&mut self, key: KeyEvent) {
        if key.kind == KeyEventKind::Release || self.done.is_some() {
            return;
        }
        match key.code {
            KeyCode::Up | KeyCode::BackTab => self.step(-1),
            KeyCode::Down | KeyCode::Tab => self.step(1),
            KeyCode::Left => self.change(-1),
            KeyCode::Right | KeyCode::Char(' ') => self.change(1),
            KeyCode::Enter => self.submit(),
            _ => {}
        }
    }
}

impl StepStateProvider for SetupForm {
    fn get_step_state(&self) -> StepState {
        if self.done.is_some() {
            StepState::Complete
        } else {
            StepState::InProgress
        }
    }
}

/// Width of the question labels column.
const LABEL_W: usize = 15;

impl SetupForm {
    /// Rows the form needs at `density`.
    pub(crate) fn height(&self, density: Density) -> u16 {
        let (agents, gaps) = match density {
            Density::Full => (self.agents.len(), 2),
            Density::Compact => (self.agents.len().min(VISIBLE_AGENTS), 2),
            Density::Tight => (self.agents.len().min(VISIBLE_AGENTS), 0),
        };
        (agents + gaps + 5) as u16
    }

    /// The agents listed at `density`, scrolled so the cursor, or else the
    /// selected agent, is in view.
    fn agent_window(&self, density: Density) -> std::ops::Range<usize> {
        let n = self.agents.len();
        if density == Density::Full || n <= VISIBLE_AGENTS {
            return 0..n;
        }
        let focus = match self.cursor {
            Row::Agent(i) => i,
            Row::Permission | Row::Usage => self.selected.unwrap_or(0),
        };
        let top = self
            .agent_top
            .get()
            .min(focus)
            .max((focus + 1).saturating_sub(VISIBLE_AGENTS))
            .min(n - VISIBLE_AGENTS);
        self.agent_top.set(top);
        top..top + VISIBLE_AGENTS
    }

    fn pointer(&self, row: Row) -> Span<'static> {
        if self.cursor == row {
            "› ".green().bold()
        } else {
            "  ".into()
        }
    }

    fn lines(&self, density: Density) -> Vec<Line<'static>> {
        let gap = density != Density::Tight;
        let window = self.agent_window(density);
        let mut lines = vec![Line::from(vec![
            "  ".into(),
            "Default agent".bold(),
            if self.missing_agent {
                "   Pick an agent to start with".green()
            } else {
                "   you can switch any time with /agent".dim()
            },
        ])];
        let name_w = self
            .agents
            .iter()
            .map(|a| a.kind.display_name().len())
            .max()
            .unwrap_or(0);
        let (hidden_above, hidden_below) = (window.start, self.agents.len() - window.end);
        for (i, agent) in self.agents.iter().enumerate() {
            if !window.contains(&i) {
                continue;
            }
            let radio = if self.selected == Some(i) {
                "●".green()
            } else {
                "○".dim()
            };
            let name = format!(" {:name_w$}   ", agent.kind.display_name());
            let status = match agent.status {
                AuthStatus::SignedIn => "signed in".green(),
                AuthStatus::SignedOut => "not signed in".dim(),
                AuthStatus::Unknown => "sign-in not checked".dim(),
            };
            let mut line = Line::from(vec![
                "  ".into(),
                self.pointer(Row::Agent(i)),
                radio,
                name.into(),
                status,
            ]);
            if i == window.start && hidden_above > 0 {
                line.push_span(format!("   ↑ {hidden_above} more").dim());
            }
            if i + 1 == window.end && hidden_below > 0 {
                line.push_span(format!("   ↓ {hidden_below} more").dim());
            }
            lines.push(line);
        }
        if gap {
            lines.push(Line::from(""));
        }
        let choice = |text: String| Span::from(format!("‹ {text} ›"));
        lines.push(Line::from(vec![
            self.pointer(Row::Permission),
            format!("  {:LABEL_W$}", "Permissions").bold(),
            choice(self.permission.label().to_string()).green(),
            format!("   {}", self.permission.note()).dim(),
        ]));
        lines.push(Line::from(vec![
            self.pointer(Row::Usage),
            format!("  {:LABEL_W$}", "Usage stats").bold(),
            choice(
                if self.share_usage {
                    "Share"
                } else {
                    "Don't share"
                }
                .to_string(),
            )
            .green(),
            "   help the Nori team improve the CLI".dim(),
        ]));
        lines.push(Line::from(
            format!(
                "    {:LABEL_W$}Counts only. Never your prompts, code, file paths or sessions.",
                ""
            )
            .dim(),
        ));
        if gap {
            lines.push(Line::from(""));
        }
        lines.push(Line::from(vec![
            "  ".into(),
            "Enter".bold(),
            " start   ".dim(),
            "↑↓".bold(),
            " move   ".dim(),
            "←→".bold(),
            " change   ".dim(),
            "q".bold(),
            " quit".dim(),
        ]));
        lines
    }
}

impl SetupForm {
    pub(crate) fn render_with(&self, density: Density, area: Rect, buf: &mut Buffer) {
        Paragraph::new(self.lines(density)).render(area, buf);
    }
}

impl WidgetRef for SetupForm {
    fn render_ref(&self, area: Rect, buf: &mut Buffer) {
        self.render_with(Density::Full, area, buf);
    }
}
