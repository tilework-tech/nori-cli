//! The folder trust prompt, drawn under the onigiri welcome: whether Nori
//! may work in this folder without asking, and (when Nori would otherwise
//! show full tool output) whether to use compact tool output here. One Enter
//! accepts the prefilled answers and saves them for the project. Tool
//! output shows Full until changed and is saved only once the user picks a
//! value, so an untouched answer keeps following the global setting.
//!
//! Trust is prefilled from version control: "Work without asking" in a git
//! repository, "Ask first" elsewhere.

#[cfg(test)]
mod tests;

use std::path::PathBuf;

use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use nori_config::NoriConfigEdits;
use nori_config::ToolOutput;
use nori_config::TrustLevel;
use nori_config::resolve_root_git_project_for_trust;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;

use super::KeyboardHandler;
use super::StepState;
use super::StepStateProvider;
use super::TrustDirectorySelection;
use super::welcome_step::Density;
use super::welcome_step::Welcome;
use super::welcome_step::WelcomeForm;
use super::welcome_step::WelcomeStep;

/// The trust prompt under the welcome.
pub(crate) type NoriTrustStep = WelcomeStep<TrustForm>;

impl NoriTrustStep {
    /// `follows` is set when the trust prompt comes after the setup form,
    /// so it unrolls as soon as it takes over.
    pub(crate) fn trust(
        nori_home: PathBuf,
        cwd: PathBuf,
        is_git_repo: bool,
        ask_tool_output: bool,
        welcome: Welcome,
        follows: bool,
    ) -> Self {
        WelcomeStep::new(
            TrustForm::new(nori_home, cwd, is_git_repo, ask_tool_output),
            welcome,
            follows,
        )
    }

    pub(crate) fn selection(&self) -> Option<TrustDirectorySelection> {
        self.form.selection()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Row {
    Trust,
    DontTrust,
    ToolOutput,
}

pub(crate) struct TrustForm {
    nori_home: PathBuf,
    /// The project the answers are saved for: the git root, else the folder.
    target: PathBuf,
    /// `target` as shown, relative to the home directory when inside it.
    label: String,
    is_git_repo: bool,
    trust: TrustDirectorySelection,
    ask_tool_output: bool,
    /// The tool output the user picked; `None` until they pick one.
    tool_output: Option<ToolOutput>,
    cursor: Row,
    done: Option<TrustDirectorySelection>,
}

impl TrustForm {
    pub(crate) fn new(
        nori_home: PathBuf,
        cwd: PathBuf,
        is_git_repo: bool,
        ask_tool_output: bool,
    ) -> Self {
        let target = resolve_root_git_project_for_trust(&cwd).unwrap_or(cwd);
        let label = crate::exec_command::relativize_to_home(&target)
            .map(|p| format!("~/{}", p.display()))
            .unwrap_or_else(|| target.display().to_string());
        let trust = if is_git_repo {
            TrustDirectorySelection::Trust
        } else {
            TrustDirectorySelection::DontTrust
        };
        Self {
            nori_home,
            target,
            label,
            is_git_repo,
            trust,
            ask_tool_output,
            tool_output: None,
            cursor: row_for(trust),
            done: None,
        }
    }

    /// The trust answer, once the form is submitted.
    pub(crate) fn selection(&self) -> Option<TrustDirectorySelection> {
        self.done
    }

    fn rows(&self) -> Vec<Row> {
        let mut rows = vec![Row::Trust, Row::DontTrust];
        if self.ask_tool_output {
            rows.push(Row::ToolOutput);
        }
        rows
    }

    fn step(&mut self, by: i32) {
        let rows = self.rows();
        let at = rows.iter().position(|r| *r == self.cursor).unwrap_or(0) as i32;
        self.cursor = rows[(at + by).clamp(0, rows.len() as i32 - 1) as usize];
    }

    fn change(&mut self) {
        match self.cursor {
            Row::Trust => self.trust = TrustDirectorySelection::Trust,
            Row::DontTrust => self.trust = TrustDirectorySelection::DontTrust,
            Row::ToolOutput => {
                self.tool_output = Some(match self.shown_tool_output() {
                    ToolOutput::Full => ToolOutput::Compact,
                    ToolOutput::Compact => ToolOutput::Full,
                });
            }
        }
    }

    fn shown_tool_output(&self) -> ToolOutput {
        self.tool_output.unwrap_or(ToolOutput::Full)
    }

    fn pick_trust(&mut self, trust: TrustDirectorySelection) {
        self.trust = trust;
        self.cursor = row_for(trust);
    }

    fn submit(&mut self) {
        if matches!(self.cursor, Row::Trust | Row::DontTrust) {
            self.change();
        }
        let level = match self.trust {
            TrustDirectorySelection::Trust => TrustLevel::Trusted,
            TrustDirectorySelection::DontTrust => TrustLevel::Untrusted,
        };
        let mut edits =
            NoriConfigEdits::new(&self.nori_home).set_project_trust_level(&self.target, level);
        if let Some(tool_output) = self.tool_output {
            edits = edits.set_project_tool_output(&self.target, tool_output);
        }
        if let Err(e) = edits.apply_blocking() {
            tracing::error!("Failed to save folder trust: {e:?}");
        }
        self.done = Some(self.trust);
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
        let note = if self.is_git_repo {
            "version controlled, so changes can be undone"
        } else {
            "not version controlled, so asking first is safer"
        };
        let mut lines = vec![Line::from(vec![
            "  ".into(),
            format!("Trust {}?", self.label).bold(),
            format!("   {note}").dim(),
        ])];
        let options = [
            (
                Row::Trust,
                TrustDirectorySelection::Trust,
                "Work without asking",
                "edits and commands run without approval",
            ),
            (
                Row::DontTrust,
                TrustDirectorySelection::DontTrust,
                "Ask first",
                "approve each edit and command",
            ),
        ];
        for (row, trust, label, note) in options {
            let radio = if self.trust == trust {
                "●".green()
            } else {
                "○".dim()
            };
            lines.push(Line::from(vec![
                "  ".into(),
                self.pointer(row),
                radio,
                format!(" {label:19}   ").into(),
                note.dim(),
            ]));
        }
        if self.ask_tool_output {
            if gap {
                lines.push(Line::from(""));
            }
            let (label, note) = match self.shown_tool_output() {
                ToolOutput::Full => ("Full", "every command with all of its output"),
                ToolOutput::Compact => ("Compact", "one line per tool call; Ctrl+O shows all"),
            };
            lines.push(Line::from(vec![
                self.pointer(Row::ToolOutput),
                format!("  {:15}", "Tool output").bold(),
                format!("‹ {label} ›").green(),
                format!("   {note}").dim(),
            ]));
        }
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

fn row_for(trust: TrustDirectorySelection) -> Row {
    match trust {
        TrustDirectorySelection::Trust => Row::Trust,
        TrustDirectorySelection::DontTrust => Row::DontTrust,
    }
}

impl KeyboardHandler for TrustForm {
    fn handle_key_event(&mut self, key: KeyEvent) {
        if key.kind == KeyEventKind::Release || self.done.is_some() {
            return;
        }
        match key.code {
            KeyCode::Up | KeyCode::BackTab => self.step(-1),
            KeyCode::Down | KeyCode::Tab => self.step(1),
            KeyCode::Left | KeyCode::Right | KeyCode::Char(' ') => self.change(),
            KeyCode::Char('y') | KeyCode::Char('1') => {
                self.pick_trust(TrustDirectorySelection::Trust);
            }
            KeyCode::Char('n') | KeyCode::Char('2') => {
                self.pick_trust(TrustDirectorySelection::DontTrust);
            }
            KeyCode::Char('c') if self.ask_tool_output => {
                self.tool_output = Some(ToolOutput::Compact);
            }
            KeyCode::Char('f') if self.ask_tool_output => {
                self.tool_output = Some(ToolOutput::Full);
            }
            KeyCode::Enter => self.submit(),
            _ => {}
        }
    }
}

impl StepStateProvider for TrustForm {
    fn get_step_state(&self) -> StepState {
        if self.done.is_some() {
            StepState::Complete
        } else {
            StepState::InProgress
        }
    }
}

impl WelcomeForm for TrustForm {
    fn height(&self, density: Density) -> u16 {
        let tool = u16::from(self.ask_tool_output);
        let gaps = match density {
            Density::Full | Density::Compact => 1 + tool,
            Density::Tight => 0,
        };
        4 + tool + gaps
    }

    fn render_with(&self, density: Density, area: Rect, buf: &mut Buffer) {
        Paragraph::new(self.lines(density)).render(area, buf);
    }
}
