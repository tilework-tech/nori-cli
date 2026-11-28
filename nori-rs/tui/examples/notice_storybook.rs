//! Notice cell specimen.
//!
//! Two scenes render the production notice cell over synthetic fixtures:
//!
//! - `gallery` lists every specimen at one width, in one state.
//! - `startup` mocks the first seconds of a session: the real composer has
//!   focus from the first frame, a notice arrives a moment later, and Enter
//!   sends the draft and settles the notice into history. Tab and Esc act on
//!   the notice only while the prompt is empty.
//!
//! Everything around the cell is simulated: the welcome lines, the agent
//! reply, the notice timing, and every action. Actions only print what they
//! would do.
//!
//! Flags set the starting state so captures are repeatable:
//! `--scene gallery|startup`, `--width N`, `--state active|after-prompt|after-tab`,
//! `--notice cli|adapter|sign-in|info|invitation|tip`, `--also <notice>`,
//! `--arrive-ms N`.

#[path = "../../tui-components/examples/support/mod.rs"]
mod support;

use std::time::Duration;
use std::time::Instant;

use anyhow::Result;
use anyhow::bail;
use crossterm::event::Event;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use crossterm::event::KeyModifiers;
use nori_tui::ComposerAction;
use nori_tui::ComposerInput;
use nori_tui::storybook::Notice;
use nori_tui::storybook::NoticeKey;
use nori_tui::storybook::NoticeSlot;
use nori_tui::storybook::NoticeSpecimen;
use nori_tui::storybook::PromptState;
use nori_tui::storybook::user_prompt_lines;
use nori_tui_components::KeyHint;
use nori_tui_components::KeyHints;
use nori_tui_components::Theme;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;

use support::StorybookTerminal;

const WIDTHS: [u16; 9] = [120, 100, 80, 64, 56, 48, 40, 32, 24];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Scene {
    Gallery,
    Startup,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum GalleryState {
    Active,
    AfterPrompt,
    AfterTab,
}

impl GalleryState {
    fn next(self) -> Self {
        match self {
            Self::Active => Self::AfterPrompt,
            Self::AfterPrompt => Self::AfterTab,
            Self::AfterTab => Self::Active,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::AfterPrompt => "in history after a prompt",
            Self::AfterTab => "in history after Tab (Tab passes through without an action)",
        }
    }
}

struct Args {
    scene: Scene,
    width: usize,
    state: GalleryState,
    notices: Vec<NoticeSpecimen>,
    arrive: Duration,
}

fn specimen(name: &str) -> Result<NoticeSpecimen> {
    Ok(match name {
        "cli" => NoticeSpecimen::CliUpdate,
        "adapter" => NoticeSpecimen::AdapterUpdate,
        "sign-in" => NoticeSpecimen::AdapterSignIn,
        "info" => NoticeSpecimen::AdapterInfo,
        "invitation" => NoticeSpecimen::Invitation,
        "tip" => NoticeSpecimen::Tip,
        other => bail!("unknown notice {other}"),
    })
}

fn parse_args() -> Result<Args> {
    let mut args = Args {
        scene: Scene::Gallery,
        width: 2,
        state: GalleryState::Active,
        notices: vec![NoticeSpecimen::CliUpdate],
        arrive: Duration::from_millis(1200),
    };
    let mut argv = std::env::args().skip(1);
    while let Some(arg) = argv.next() {
        let mut value = || argv.next().unwrap_or_default();
        match arg.as_str() {
            "--scene" => {
                args.scene = match value().as_str() {
                    "gallery" => Scene::Gallery,
                    "startup" => Scene::Startup,
                    other => bail!("unknown --scene {other}"),
                }
            }
            "--width" => {
                let width: u16 = value().parse()?;
                let Some(index) = WIDTHS.iter().position(|w| *w == width) else {
                    bail!("--width must be one of {WIDTHS:?}");
                };
                args.width = index;
            }
            "--state" => {
                args.state = match value().as_str() {
                    "active" => GalleryState::Active,
                    "after-prompt" => GalleryState::AfterPrompt,
                    "after-tab" => GalleryState::AfterTab,
                    other => bail!("unknown --state {other}"),
                }
            }
            "--notice" => args.notices = vec![specimen(&value())?],
            "--also" => args.notices.push(specimen(&value())?),
            "--arrive-ms" => args.arrive = Duration::from_millis(value().parse()?),
            other => bail!("unknown argument {other}"),
        }
    }
    Ok(args)
}

fn main() -> Result<()> {
    let args = parse_args()?;
    let mut terminal = StorybookTerminal::enter()?;
    match args.scene {
        Scene::Gallery => gallery(&mut terminal, args),
        Scene::Startup => startup(&mut terminal, &args),
    }
}

fn gallery(terminal: &mut StorybookTerminal, mut args: Args) -> Result<()> {
    let theme = terminal.theme;
    let mut scroll: u16 = 0;
    loop {
        terminal.terminal.draw(|frame| {
            let area = frame.area();
            let width = WIDTHS[args.width].min(area.width.saturating_sub(2));
            let mut lines = vec![
                Line::styled("Notice cell specimens", theme.title),
                Line::styled(
                    format!("Width {width} · State: {}", args.state.label()),
                    theme.muted,
                ),
                ruler(width, theme.muted),
            ];
            for specimen in NoticeSpecimen::ALL {
                lines.push(Line::default());
                lines.push(Line::styled(specimen.label(), theme.muted));
                lines.extend(gallery_notice(specimen, args.state).lines(width));
            }
            let body = Rect::new(1, 1, width, area.height.saturating_sub(3));
            Paragraph::new(lines)
                .scroll((scroll, 0))
                .render(body, frame.buffer_mut());
            KeyHints::new([
                KeyHint::new("← →", "width"),
                KeyHint::new("s", "state"),
                KeyHint::new("↑ ↓", "scroll"),
                KeyHint::new("q", "close"),
            ])
            .theme(theme)
            .render(
                Rect::new(
                    1,
                    area.height.saturating_sub(1),
                    area.width.saturating_sub(2),
                    1,
                ),
                frame.buffer_mut(),
            );
        })?;

        let Some(Event::Key(key)) = terminal.next_event()? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        match key.code {
            KeyCode::Left => args.width = (args.width + 1).min(WIDTHS.len() - 1),
            KeyCode::Right => args.width = args.width.saturating_sub(1),
            KeyCode::Char('s') => args.state = args.state.next(),
            KeyCode::Up => scroll = scroll.saturating_sub(1),
            KeyCode::Down => scroll = scroll.saturating_add(1),
            KeyCode::Esc | KeyCode::Char('q') => return Ok(()),
            _ => {}
        }
    }
}

/// Put a specimen into the requested state through the same slot the
/// session uses, so the gallery shows only states the slot produces. A
/// notice without an action ignores Tab and stays active.
fn gallery_notice(specimen: NoticeSpecimen, state: GalleryState) -> Notice {
    let mut slot = NoticeSlot::default();
    slot.offer(specimen.notice());
    match state {
        GalleryState::Active => specimen.notice(),
        GalleryState::AfterPrompt => slot.on_submit().remove(0),
        GalleryState::AfterTab => match slot.handle_key(press(KeyCode::Tab), IDLE_EMPTY_PROMPT) {
            NoticeKey::Settled { notice, .. } => notice,
            NoticeKey::PassThrough => specimen.notice(),
        },
    }
}

const IDLE_EMPTY_PROMPT: PromptState = PromptState {
    is_empty: true,
    is_busy: false,
};

/// A transcript entry in the mocked session.
enum Entry {
    Notice(Notice),
    Prompt(String),
    Mock(String),
}

struct Session {
    started: Instant,
    pending: Vec<NoticeSpecimen>,
    slot: NoticeSlot,
    composer: ComposerInput,
    history: Vec<Entry>,
    sends: i64,
}

impl Session {
    fn new(notices: &[NoticeSpecimen]) -> Self {
        let mut composer = ComposerInput::for_attachment();
        composer.set_hint_items(vec![
            ("Enter", "send"),
            ("Ctrl+R", "replay"),
            ("Ctrl+C", "quit"),
        ]);
        Self {
            started: Instant::now(),
            pending: notices.to_vec(),
            slot: NoticeSlot::default(),
            composer,
            history: Vec::new(),
            sends: 0,
        }
    }

    fn deliver_due_notices(&mut self, arrive: Duration) {
        if self.started.elapsed() >= arrive {
            for specimen in self.pending.drain(..) {
                self.slot.offer(specimen.notice());
            }
        }
    }

    fn key(&mut self, key: KeyEvent) {
        let prompt = PromptState {
            is_empty: self.composer.is_empty(),
            is_busy: self.composer.is_in_paste_burst(),
        };
        match self.slot.handle_key(key, prompt) {
            NoticeKey::Settled { invoked, notice } => {
                self.history.push(Entry::Notice(notice));
                if let Some(action) = invoked {
                    self.history.push(Entry::Mock(format!(
                        "mock action {action} invoked; nothing ran"
                    )));
                }
            }
            NoticeKey::PassThrough => {
                if let ComposerAction::Submitted(text) = self.composer.input(key) {
                    self.sends += 1;
                    self.history
                        .extend(self.slot.on_submit().into_iter().map(Entry::Notice));
                    self.history.push(Entry::Prompt(text));
                    self.history.push(Entry::Mock(format!(
                        "mock agent received send #{}",
                        self.sends
                    )));
                }
            }
        }
    }
}

fn startup(terminal: &mut StorybookTerminal, args: &Args) -> Result<()> {
    let theme = terminal.theme;
    let mut session = Session::new(&args.notices);
    loop {
        session.deliver_due_notices(args.arrive);
        session.composer.flush_paste_burst_if_due();
        terminal
            .terminal
            .draw(|frame| draw_startup(frame, &session, theme))?;

        let Some(event) = terminal.next_event()? else {
            continue;
        };
        let Event::Key(key) = event else {
            if let Event::Paste(text) = event {
                session.composer.handle_paste(text);
            }
            continue;
        };
        if key.modifiers == KeyModifiers::CONTROL && key.kind == KeyEventKind::Press {
            match key.code {
                KeyCode::Char('c') => return Ok(()),
                KeyCode::Char('r') => {
                    session = Session::new(&args.notices);
                    continue;
                }
                _ => {}
            }
        }
        session.key(key);
    }
}

fn draw_startup(frame: &mut Frame, session: &Session, theme: Theme) {
    let area = frame.area();
    let width = area.width;
    let mut lines = vec![
        Line::styled(
            "Notice storybook · simulated: welcome lines, notice timing, agent, actions",
            theme.muted,
        ),
        Line::default(),
        Line::from(vec!["〉nori CLI".green().bold(), " 0.30.2".dim()]),
        Line::from("  Claude Code (mock agent) · sign-in not checked".dim()),
        Line::default(),
    ];
    for entry in &session.history {
        match entry {
            Entry::Notice(notice) => {
                lines.extend(notice.lines(width));
                lines.push(Line::default());
            }
            Entry::Prompt(text) => lines.extend(user_prompt_lines(text, width)),
            Entry::Mock(text) => {
                lines.push(Line::from(Span::styled(format!("  · {text}"), theme.muted)));
                lines.push(Line::default());
            }
        }
    }
    if let Some(notice) = session.slot.active() {
        lines.extend(notice.lines(width));
        lines.push(Line::default());
    }

    let composer_height = session.composer.desired_height(width);
    let room = area.height.saturating_sub(composer_height + 1);
    let skip = lines.len().saturating_sub(usize::from(room));
    let shown = u16::try_from(lines.len() - skip).unwrap_or(room);
    Paragraph::new(lines.split_off(skip)).render(Rect::new(0, 0, width, shown), frame.buffer_mut());

    let composer = Rect::new(0, shown, width, composer_height);
    session.composer.render_ref(composer, frame.buffer_mut());
    if let Some((x, y)) = session.composer.cursor_pos(composer) {
        frame.set_cursor_position((x, y));
    }

    let status = format!(
        "sends {} · active notice: {} · elapsed {:.1}s",
        session.sends,
        session.slot.active().map_or("none", Notice::source),
        session.started.elapsed().as_secs_f32()
    );
    Paragraph::new(Line::styled(status, theme.muted)).render(
        Rect::new(0, area.height.saturating_sub(1), width, 1),
        frame.buffer_mut(),
    );
}

fn press(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

/// A dotted row exactly `width` cells wide, so the edge is visible.
fn ruler(width: u16, style: Style) -> Line<'static> {
    let width = usize::from(width);
    let label = format!(" {width}");
    let dots = width.saturating_sub(label.len());
    Line::from(Span::styled(format!("{}{label}", "·".repeat(dots)), style))
}
