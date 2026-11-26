//! Nori-specific update prompt UI
//!
//! This module provides the update prompt screen for Nori CLI updates.

#![cfg(not(debug_assertions))]

use crate::nori::update_action::UpdateAction;
use crate::nori::updates;
use crate::tui::FrameRequester;
use crate::tui::Tui;
use crate::tui::TuiEvent;
use color_eyre::Result;
use crossterm::event::KeyEvent;
use nori_config::NoriConfig;
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

pub(crate) enum UpdatePromptOutcome {
    Continue,
    RunUpdate(UpdateAction),
}

pub(crate) async fn run_update_prompt_if_needed(
    tui: &mut Tui,
    config: &NoriConfig,
) -> Result<UpdatePromptOutcome> {
    let Some(latest_version) = updates::get_upgrade_version_for_popup(config) else {
        return Ok(UpdatePromptOutcome::Continue);
    };
    let Some(update_action) = crate::nori::update_action::get_update_action() else {
        return Ok(UpdatePromptOutcome::Continue);
    };

    let mut screen =
        UpdatePromptScreen::new(tui.frame_requester(), latest_version.clone(), update_action);
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

    match screen.selection() {
        Some(UpdateSelection::UpdateNow) => {
            tui.terminal.clear()?;
            Ok(UpdatePromptOutcome::RunUpdate(update_action))
        }
        Some(UpdateSelection::NotNow) | None => Ok(UpdatePromptOutcome::Continue),
        Some(UpdateSelection::DontRemind) => {
            if let Err(err) = updates::dismiss_version(config, screen.latest_version()).await {
                tracing::error!("Failed to persist update dismissal: {err}");
            }
            Ok(UpdatePromptOutcome::Continue)
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum UpdateSelection {
    UpdateNow,
    NotNow,
    DontRemind,
}

struct UpdatePromptScreen {
    request_frame: FrameRequester,
    latest_version: String,
    current_version: String,
    menu: MenuState<UpdateSelection>,
    selection: Option<UpdateSelection>,
}

impl UpdatePromptScreen {
    fn new(
        request_frame: FrameRequester,
        latest_version: String,
        update_action: UpdateAction,
    ) -> Self {
        let update_command = update_action.command_str();
        Self {
            request_frame,
            latest_version,
            current_version: env!("CARGO_PKG_VERSION").to_string(),
            menu: crate::overlay_menu::state_from_items(
                [
                    MenuItem::new(UpdateSelection::UpdateNow, "Update now")
                        .description(format!("Run `{update_command}`"))
                        .mnemonic('u')
                        .number_shortcut(1),
                    MenuItem::new(UpdateSelection::NotNow, "Not now")
                        .description("Continue with the installed version")
                        .mnemonic('n')
                        .number_shortcut(2),
                    MenuItem::new(
                        UpdateSelection::DontRemind,
                        "Don't remind me for this version",
                    )
                    .description("Hide this release until a newer version is available")
                    .mnemonic('d')
                    .number_shortcut(3),
                ],
                "update prompt",
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
            MenuOutcome::Cancelled => self.select(UpdateSelection::NotNow),
            MenuOutcome::SelectionChanged(_) => self.request_frame.schedule_frame(),
            MenuOutcome::Unchanged => {}
        }
    }

    fn select(&mut self, selection: UpdateSelection) {
        self.selection = Some(selection);
        self.request_frame.schedule_frame();
    }

    fn is_done(&self) -> bool {
        self.selection.is_some()
    }

    fn selection(&self) -> Option<UpdateSelection> {
        self.selection
    }

    fn latest_version(&self) -> &str {
        self.latest_version.as_str()
    }
}

impl WidgetRef for &UpdatePromptScreen {
    fn render_ref(&self, area: Rect, buf: &mut Buffer) {
        let mut state = self.menu.clone();
        OverlayMenu::new(format!(
            "Update available: {} → {}",
            self.current_version, self.latest_version
        ))
        .subtitle("Release notes: https://github.com/tilework-tech/nori-cli/releases/latest")
        .theme(crate::style::component_theme())
        .max_width(76)
        .density(MenuDensity::Dense)
        .row_pattern(MenuRowPattern::Zebra)
        .fullscreen_selection_rails(true)
        .key_hints(crate::overlay_menu::default_hints())
        .render(area, buf, &mut state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_backend::VT100Backend;
    use crate::tui::FrameRequester;
    use crossterm::event::KeyCode;
    use crossterm::event::KeyEvent;
    use crossterm::event::KeyModifiers;
    use ratatui::Terminal;
    use ratatui::widgets::FrameExt;

    fn new_prompt() -> UpdatePromptScreen {
        UpdatePromptScreen::new(
            FrameRequester::test_dummy(),
            "9.9.9".into(),
            UpdateAction::NpmGlobalLatest,
        )
    }

    #[test]
    fn nori_update_prompt_snapshot() {
        let screen = new_prompt();
        let mut terminal = Terminal::new(VT100Backend::new(80, 12)).expect("terminal");
        terminal
            .draw(|frame| frame.render_widget_ref(&screen, frame.area()))
            .expect("render update prompt");
        insta::assert_snapshot!("nori_update_prompt_modal", terminal.backend());
    }

    #[test]
    fn nori_update_prompt_confirm_selects_update() {
        let mut screen = new_prompt();
        screen.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(screen.is_done());
        assert_eq!(screen.selection(), Some(UpdateSelection::UpdateNow));
    }

    #[test]
    fn nori_update_prompt_dismiss_option_leaves_prompt_in_normal_state() {
        let mut screen = new_prompt();
        screen.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        screen.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(screen.is_done());
        assert_eq!(screen.selection(), Some(UpdateSelection::NotNow));
    }

    #[test]
    fn nori_update_prompt_dont_remind_selects_dismissal() {
        let mut screen = new_prompt();
        screen.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        screen.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        screen.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(screen.is_done());
        assert_eq!(screen.selection(), Some(UpdateSelection::DontRemind));
    }

    #[test]
    fn nori_update_prompt_ctrl_c_skips_update() {
        let mut screen = new_prompt();
        screen.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
        assert!(screen.is_done());
        assert_eq!(screen.selection(), Some(UpdateSelection::NotNow));
    }

    #[test]
    fn nori_update_prompt_navigation_wraps_between_entries() {
        let mut screen = new_prompt();
        screen.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(
            screen.menu.selected_item().map(|item| *item.key()),
            Some(UpdateSelection::DontRemind)
        );
        screen.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(
            screen.menu.selected_item().map(|item| *item.key()),
            Some(UpdateSelection::UpdateNow)
        );
    }
}
