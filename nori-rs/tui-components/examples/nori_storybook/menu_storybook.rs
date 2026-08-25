//! Storybook adapter for the production overlay-menu component.

mod fixtures;

use nori_tui_components::MenuModelError;
use nori_tui_components::MenuOutcome;
use nori_tui_components::MenuShortcut;
use nori_tui_components::MenuState;
use nori_tui_components::OverlayMenu;
use nori_tui_components::Theme;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Block;
use ratatui::widgets::Paragraph;
use ratatui::widgets::StatefulWidget;
use ratatui::widgets::Widget;

pub(super) use nori_tui_components::MenuAction;

#[derive(Clone, Copy)]
pub(super) enum MenuStory {
    Action,
    Shortcuts,
    Narrow,
    Destructive,
}

impl MenuStory {
    fn next(self) -> Self {
        match self {
            Self::Action => Self::Shortcuts,
            Self::Shortcuts => Self::Narrow,
            Self::Narrow => Self::Destructive,
            Self::Destructive => Self::Action,
        }
    }

    fn previous(self) -> Self {
        match self {
            Self::Action => Self::Destructive,
            Self::Shortcuts => Self::Action,
            Self::Narrow => Self::Shortcuts,
            Self::Destructive => Self::Narrow,
        }
    }
}

pub(super) struct MenuStoryState {
    story: MenuStory,
    menu: MenuState<&'static str>,
    notice: Option<String>,
}

impl MenuStoryState {
    pub(super) fn new(story: MenuStory) -> Result<Self, MenuModelError> {
        Ok(Self {
            story,
            menu: fixtures::state(story)?,
            notice: None,
        })
    }

    pub(super) fn next_story(&mut self) -> Result<(), MenuModelError> {
        self.set_story(self.story.next())
    }

    pub(super) fn previous_story(&mut self) -> Result<(), MenuModelError> {
        self.set_story(self.story.previous())
    }

    pub(super) fn handle(&mut self, action: MenuAction) -> MenuOutcome<&'static str> {
        let unavailable = self.unavailable_shortcut(action);
        let outcome = self.menu.handle(action);
        match &outcome {
            MenuOutcome::Activated(key) => {
                let label = self
                    .menu
                    .items()
                    .iter()
                    .find(|item| item.key() == key)
                    .map_or(*key, |item| item.label());
                self.notice = Some(format!("Activated {label}"));
            }
            MenuOutcome::SelectionChanged(_) => self.notice = None,
            MenuOutcome::Unchanged => {
                if let Some(label) = unavailable {
                    self.notice = Some(format!("{label} is unavailable"));
                }
            }
            MenuOutcome::Cancelled => {}
        }
        outcome
    }

    fn set_story(&mut self, story: MenuStory) -> Result<(), MenuModelError> {
        let menu = fixtures::state(story)?;
        self.story = story;
        self.menu = menu;
        self.notice = None;
        Ok(())
    }

    fn unavailable_shortcut(&self, action: MenuAction) -> Option<String> {
        let MenuAction::InvokeShortcut(shortcut) = action else {
            return None;
        };
        self.menu
            .items()
            .iter()
            .find(|item| {
                item.is_disabled()
                    && match shortcut {
                        MenuShortcut::Character(character) => item
                            .mnemonic_shortcut()
                            .is_some_and(|mnemonic| mnemonic.eq_ignore_ascii_case(&character)),
                        MenuShortcut::Number(number) => item.number() == Some(number),
                    }
            })
            .map(|item| item.label().to_string())
    }
}

pub(super) fn render(area: Rect, buf: &mut Buffer, theme: Theme, state: &mut MenuStoryState) {
    render_host(area, buf, theme, state);
    let caller_area = match state.story {
        MenuStory::Narrow => {
            let width = area.width.min(30);
            let height = area.height.min(12);
            Rect::new(
                area.x.saturating_add(area.width.saturating_sub(width) / 2),
                area.y
                    .saturating_add(area.height.saturating_sub(height) / 2),
                width,
                height,
            )
        }
        MenuStory::Action | MenuStory::Shortcuts | MenuStory::Destructive => area,
    };
    let presentation = fixtures::presentation(state.story);
    let mut menu = OverlayMenu::new(presentation.title)
        .theme(theme)
        .max_width(presentation.max_width)
        .fullscreen_selection_rails(!matches!(state.story, MenuStory::Narrow))
        .key_hints(fixtures::footer_hints(state.story));
    if let Some(subtitle) = state.notice.as_deref().or(presentation.subtitle) {
        menu = menu.subtitle(subtitle);
    }
    StatefulWidget::render(menu, caller_area, buf, &mut state.menu);
}

fn render_host(area: Rect, buf: &mut Buffer, theme: Theme, state: &MenuStoryState) {
    Block::default().style(theme.surface).render(area, buf);
    if area.width < 60 {
        return;
    }
    let story_name = match state.story {
        MenuStory::Action => "centered action",
        MenuStory::Shortcuts => "shortcut-heavy",
        MenuStory::Narrow => "30x12 caller area",
        MenuStory::Destructive => "semantic consequence",
    };
    let lines = vec![
        Line::from(vec![
            Span::styled("Overlay menu", theme.text.add_modifier(Modifier::BOLD)),
            Span::styled(format!("  {story_name}"), theme.muted),
        ]),
        Line::styled("tab next example   shift-tab previous example", theme.muted),
        Line::styled(state.notice.as_deref().unwrap_or_default(), theme.info),
        Line::styled("Session transcript", theme.text),
        Line::styled("[host] transcript remains beneath the overlay", theme.muted),
        Line::styled("[host] status stays visible outside the menu", theme.muted),
    ];
    Paragraph::new(lines).render(area, buf);
}
