use super::MenuStory;
use super::PrototypeMenu;
use super::fixtures;

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

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum MenuAction {
    MoveUp,
    MoveDown,
    ActivateSelected,
    InvokeNumber(u8),
    InvokeCharacter(char),
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum MenuOutcome {
    Unchanged,
    SelectionChanged(&'static str),
    Activated(&'static str),
}

pub(crate) struct MenuStoryState {
    story: MenuStory,
    selected_index: usize,
    notice: Option<String>,
}

impl MenuStoryState {
    pub(crate) fn new(story: MenuStory) -> Self {
        let selected_index = fixtures::menu(story).selected;
        Self {
            story,
            selected_index,
            notice: None,
        }
    }

    pub(crate) fn story(&self) -> MenuStory {
        self.story
    }

    pub(crate) fn selected_index(&self) -> usize {
        self.selected_index
    }

    pub(crate) fn notice(&self) -> Option<&str> {
        self.notice.as_deref()
    }

    pub(crate) fn next_story(&mut self) {
        self.set_story(self.story.next());
    }

    pub(crate) fn previous_story(&mut self) {
        self.set_story(self.story.previous());
    }

    pub(crate) fn handle(&mut self, action: MenuAction) -> MenuOutcome {
        let menu = fixtures::menu(self.story);
        match action {
            MenuAction::MoveUp => self.move_selection(&menu, MenuDirection::Up),
            MenuAction::MoveDown => self.move_selection(&menu, MenuDirection::Down),
            MenuAction::ActivateSelected => self.activate(&menu, self.selected_index),
            MenuAction::InvokeNumber(number) => menu
                .items
                .iter()
                .position(|item| item.number == Some(number))
                .map_or(MenuOutcome::Unchanged, |index| self.activate(&menu, index)),
            MenuAction::InvokeCharacter(character) => menu
                .items
                .iter()
                .position(|item| {
                    item.mnemonic
                        .is_some_and(|mnemonic| mnemonic.eq_ignore_ascii_case(&character))
                })
                .map_or(MenuOutcome::Unchanged, |index| self.activate(&menu, index)),
        }
    }

    fn set_story(&mut self, story: MenuStory) {
        self.story = story;
        self.selected_index = fixtures::menu(story).selected;
        self.notice = None;
    }

    fn move_selection(&mut self, menu: &PrototypeMenu, direction: MenuDirection) -> MenuOutcome {
        let mut index = self.selected_index;
        for _ in 0..menu.items.len() {
            index = match direction {
                MenuDirection::Up => index.checked_sub(1).unwrap_or(menu.items.len() - 1),
                MenuDirection::Down => (index + 1) % menu.items.len(),
            };
            if !menu.items[index].disabled {
                self.selected_index = index;
                self.notice = None;
                return MenuOutcome::SelectionChanged(menu.items[index].label);
            }
        }
        MenuOutcome::Unchanged
    }

    fn activate(&mut self, menu: &PrototypeMenu, index: usize) -> MenuOutcome {
        let item = &menu.items[index];
        if item.disabled {
            self.notice = Some(format!("{} is unavailable", item.label));
            return MenuOutcome::Unchanged;
        }
        self.selected_index = index;
        self.notice = Some(format!("Activated {}", item.label));
        MenuOutcome::Activated(item.label)
    }
}

enum MenuDirection {
    Up,
    Down,
}
