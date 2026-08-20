use super::MenuStory;
use nori_tui_components::KeyHint;
use nori_tui_components::MenuItem;
use nori_tui_components::MenuItemTone;
use nori_tui_components::MenuModelError;
use nori_tui_components::MenuState;

pub(super) struct MenuPresentation {
    pub(super) title: &'static str,
    pub(super) subtitle: Option<&'static str>,
    pub(super) max_width: u16,
}

pub(super) fn presentation(story: MenuStory) -> MenuPresentation {
    match story {
        MenuStory::Action => MenuPresentation {
            title: "Choose how to continue",
            subtitle: Some("Select one action"),
            max_width: 58,
        },
        MenuStory::Shortcuts => MenuPresentation {
            title: "Choose a session action",
            subtitle: Some("Shortcuts activate immediately"),
            max_width: 58,
        },
        MenuStory::Narrow => MenuPresentation {
            title: "Continue",
            subtitle: Some("Supporting copy disappears at this width"),
            max_width: 58,
        },
        MenuStory::Destructive => MenuPresentation {
            title: "Remove local session?",
            subtitle: Some("Remote history will remain available"),
            max_width: 58,
        },
    }
}

pub(super) fn footer_hints(story: MenuStory) -> Vec<KeyHint<'static>> {
    match story {
        MenuStory::Shortcuts => vec![
            KeyHint::new("1-5/r,s,i,a", "activate"),
            KeyHint::new("↑↓/jk", "move"),
            KeyHint::new("tab", "example"),
            KeyHint::new("q", "close"),
        ],
        MenuStory::Action | MenuStory::Narrow | MenuStory::Destructive => vec![
            KeyHint::new("↑↓/jk", "move"),
            KeyHint::new("enter", "select"),
            KeyHint::new("tab", "example"),
            KeyHint::new("q", "close"),
        ],
    }
}

pub(super) fn state(story: MenuStory) -> Result<MenuState<&'static str>, MenuModelError> {
    let items = match story {
        MenuStory::Action => vec![
            MenuItem::new("resume", "Resume session")
                .description("Continue the selected transcript"),
            MenuItem::new("new", "Start a new session")
                .description("Create an empty local session"),
            MenuItem::new("inspect", "Open read-only")
                .description("Inspect without changing state"),
            MenuItem::new("share", "Share session")
                .description("Unavailable for this session")
                .disabled(true),
        ],
        MenuStory::Shortcuts => vec![
            MenuItem::new("resume", "Resume session")
                .description("Continue the selected transcript")
                .mnemonic('r')
                .number_shortcut(1),
            MenuItem::new("new", "Start a new session")
                .description("Create an empty local session")
                .mnemonic('s')
                .number_shortcut(2),
            MenuItem::new("inspect", "Inspect transcript")
                .description("Open without changing state")
                .mnemonic('i')
                .number_shortcut(3),
            MenuItem::new("archive", "Archive session")
                .description("Move the transcript to local history")
                .mnemonic('a')
                .number_shortcut(4),
            MenuItem::new("share", "Share session")
                .description("Unavailable for this session")
                .number_shortcut(5)
                .disabled(true),
        ],
        MenuStory::Narrow => vec![
            MenuItem::new(
                "resume",
                "Resume the selected transcript without changing its history",
            )
            .description("Continue from the most recent complete assistant response"),
            MenuItem::new("new", "Start a new session")
                .description("Create an empty local session in this directory"),
            MenuItem::new("inspect", "Open read-only")
                .description("Inspect the transcript without changing it"),
            MenuItem::new("share", "Share session")
                .description("Unavailable for this session")
                .disabled(true),
        ],
        MenuStory::Destructive => vec![
            MenuItem::new("keep", "Keep session")
                .description("Return without changing local history"),
            MenuItem::new("delete", "Delete local transcript")
                .description("This cannot be restored from this machine")
                .tone(MenuItemTone::Destructive),
            MenuItem::new("archive", "Archive before deleting")
                .description("Review the transcript before removing it")
                .tone(MenuItemTone::Warning),
        ],
    };
    let mut state = MenuState::try_new(items)?;
    if matches!(story, MenuStory::Destructive) {
        let _ = state.select_key(&"delete");
    }
    Ok(state)
}
