//! Worktree ask popup for the "ask" auto-worktree mode.
//!
//! Shows a simple two-option popup at TUI startup asking the user whether
//! to create a git worktree for this session.

use crate::tui::FrameRequester;
use crate::tui::Tui;
use crate::tui::TuiEvent;
use color_eyre::Result;
use crossterm::event::KeyEvent;
use nori_tui_components::MenuDensity;
use nori_tui_components::MenuItem;
use nori_tui_components::MenuOutcome;
use nori_tui_components::MenuRowPattern;
use nori_tui_components::MenuState;
use nori_tui_components::OverlayMenu;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::StatefulWidget;
use ratatui::widgets::WidgetRef;
use tokio_stream::StreamExt;

/// Run the worktree blocked popup. Shows why worktree creation was skipped and
/// waits for the user to acknowledge.
pub(crate) async fn run_worktree_blocked_popup(tui: &mut Tui, reason: &str) -> Result<()> {
    let mut screen = WorktreeBlockedScreen::new(tui.frame_requester(), reason.to_string());
    tui.draw(u16::MAX, |frame| {
        frame.render_widget_ref(&screen, frame.area());
    })?;

    let events = tui.event_stream();
    tokio::pin!(events);

    while !screen.is_done() {
        if let Some(event) = events.next().await {
            match event {
                TuiEvent::Key(key_event) => screen.handle_key(key_event),
                TuiEvent::Paste(_) => {}
                TuiEvent::Draw => {
                    tui.draw(u16::MAX, |frame| {
                        frame.render_widget_ref(&screen, frame.area());
                    })?;
                }
            }
        } else {
            break;
        }
    }

    Ok(())
}

/// Run the worktree ask popup. Returns `true` if the user chose to create a worktree.
pub(crate) async fn run_worktree_ask_popup(tui: &mut Tui) -> Result<bool> {
    let mut screen = WorktreeAskScreen::new(tui.frame_requester());
    tui.draw(u16::MAX, |frame| {
        frame.render_widget_ref(&screen, frame.area());
    })?;

    let events = tui.event_stream();
    tokio::pin!(events);

    while !screen.is_done() {
        if let Some(event) = events.next().await {
            match event {
                TuiEvent::Key(key_event) => screen.handle_key(key_event),
                TuiEvent::Paste(_) => {}
                TuiEvent::Draw => {
                    tui.draw(u16::MAX, |frame| {
                        frame.render_widget_ref(&screen, frame.area());
                    })?;
                }
            }
        } else {
            break;
        }
    }

    Ok(screen.selection == Some(WorktreeSelection::Yes))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WorktreeSelection {
    Yes,
    No,
}

struct WorktreeAskScreen {
    request_frame: FrameRequester,
    menu: MenuState<WorktreeSelection>,
    selection: Option<WorktreeSelection>,
}

impl WorktreeAskScreen {
    fn new(request_frame: FrameRequester) -> Self {
        Self {
            request_frame,
            menu: crate::overlay_menu::state_from_items(
                [
                    MenuItem::new(WorktreeSelection::Yes, "Yes, create a worktree")
                        .description(
                            "Start this session on an isolated branch and working directory",
                        )
                        .mnemonic('y')
                        .number_shortcut(1),
                    MenuItem::new(WorktreeSelection::No, "No, continue without a worktree")
                        .description("Use the current working directory")
                        .mnemonic('n')
                        .number_shortcut(2),
                ],
                "worktree choice",
            ),
            selection: None,
        }
    }

    fn handle_key(&mut self, key_event: KeyEvent) {
        let Some(action) = crate::overlay_menu::action_from_key_event(key_event) else {
            return;
        };
        match self.menu.handle(action) {
            MenuOutcome::Activated(selection) => self.select(selection),
            MenuOutcome::Cancelled => self.select(WorktreeSelection::No),
            MenuOutcome::SelectionChanged(_) => self.request_frame.schedule_frame(),
            MenuOutcome::Unchanged => {}
        }
    }

    fn select(&mut self, selection: WorktreeSelection) {
        self.selection = Some(selection);
        self.request_frame.schedule_frame();
    }

    fn is_done(&self) -> bool {
        self.selection.is_some()
    }
}

struct WorktreeBlockedScreen {
    request_frame: FrameRequester,
    reason: String,
    menu: MenuState<()>,
    done: bool,
}

impl WorktreeBlockedScreen {
    fn new(request_frame: FrameRequester, reason: String) -> Self {
        Self {
            request_frame,
            reason,
            menu: crate::overlay_menu::state_from_items(
                [MenuItem::new((), "Continue without a worktree")
                    .description("Start the session in the current working directory")
                    .number_shortcut(1)],
                "worktree blocked acknowledgement",
            ),
            done: false,
        }
    }

    fn handle_key(&mut self, key_event: KeyEvent) {
        if let Some(action) = crate::overlay_menu::action_from_key_event(key_event) {
            match self.menu.handle(action) {
                MenuOutcome::Activated(()) | MenuOutcome::Cancelled => {
                    self.done = true;
                    self.request_frame.schedule_frame();
                }
                MenuOutcome::Unchanged | MenuOutcome::SelectionChanged(_) => {}
            }
        }
    }

    fn is_done(&self) -> bool {
        self.done
    }
}

impl WidgetRef for &WorktreeBlockedScreen {
    fn render_ref(&self, area: Rect, buf: &mut Buffer) {
        let mut state = self.menu.clone();
        render_menu(
            "Worktree cannot be created",
            Some(format!("Reason: {}", self.reason)),
            area,
            buf,
            &mut state,
        );
    }
}

impl WidgetRef for &WorktreeAskScreen {
    fn render_ref(&self, area: Rect, buf: &mut Buffer) {
        let mut state = self.menu.clone();
        render_menu(
            "Create a git worktree for this session?",
            Some("Each session gets an isolated branch and working directory.".to_string()),
            area,
            buf,
            &mut state,
        );
    }
}

fn render_menu<K: Clone + Eq>(
    title: &str,
    subtitle: Option<String>,
    area: Rect,
    buf: &mut Buffer,
    state: &mut MenuState<K>,
) {
    let mut menu = OverlayMenu::new(title.to_string())
        .theme(crate::style::component_theme())
        .max_width(76)
        .density(MenuDensity::Dense)
        .row_pattern(MenuRowPattern::Zebra)
        .fullscreen_selection_rails(true)
        .key_hints(crate::overlay_menu::default_hints());
    if let Some(subtitle) = subtitle {
        menu = menu.subtitle(subtitle);
    }
    menu.render(area, buf, state);
}
