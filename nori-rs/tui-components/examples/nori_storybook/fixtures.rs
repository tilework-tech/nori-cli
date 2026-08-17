use super::MenuStory;
use super::MenuTone;
use super::PrototypeItem;
use super::PrototypeMenu;
use codex_tui_components::KeyHint;

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

pub(super) fn menu(story: MenuStory) -> PrototypeMenu {
    match story {
        MenuStory::Action => PrototypeMenu {
            title: "Choose how to continue",
            subtitle: Some("Select one action"),
            items: vec![
                PrototypeItem::new("Resume session", "Continue the selected transcript"),
                PrototypeItem::new("Start a new session", "Create an empty local session"),
                PrototypeItem::new("Open read-only", "Inspect without changing state"),
                PrototypeItem::new("Share session", "Unavailable for this session").disabled(),
            ],
            selected: 0,
        },
        MenuStory::Shortcuts => PrototypeMenu {
            title: "Choose a session action",
            subtitle: Some("Shortcuts activate immediately"),
            items: vec![
                PrototypeItem::new("Resume session", "Continue the selected transcript")
                    .mnemonic('r')
                    .number(1),
                PrototypeItem::new("Start a new session", "Create an empty local session")
                    .mnemonic('s')
                    .number(2),
                PrototypeItem::new("Inspect transcript", "Open without changing state")
                    .mnemonic('i')
                    .number(3),
                PrototypeItem::new("Archive session", "Move the transcript to local history")
                    .mnemonic('a')
                    .number(4),
                PrototypeItem::new("Share session", "Unavailable for this session")
                    .number(5)
                    .disabled(),
            ],
            selected: 0,
        },
        MenuStory::Narrow => PrototypeMenu {
            title: "Continue",
            subtitle: Some("Supporting copy disappears at this width"),
            items: vec![
                PrototypeItem::new(
                    "Resume the selected transcript without changing its history",
                    "Continue from the most recent complete assistant response",
                ),
                PrototypeItem::new(
                    "Start a new session",
                    "Create an empty local session in this directory",
                ),
                PrototypeItem::new(
                    "Open read-only",
                    "Inspect the transcript without changing it",
                ),
                PrototypeItem::new("Share session", "Unavailable for this session").disabled(),
            ],
            selected: 0,
        },
        MenuStory::Destructive => PrototypeMenu {
            title: "Remove local session?",
            subtitle: Some("Remote history will remain available"),
            items: vec![
                PrototypeItem::new("Keep session", "Return without changing local history"),
                PrototypeItem::new(
                    "Delete local transcript",
                    "This cannot be restored from this machine",
                )
                .tone(MenuTone::Destructive),
                PrototypeItem::new(
                    "Archive before deleting",
                    "Review the transcript before removing it",
                )
                .tone(MenuTone::Warning),
            ],
            selected: 1,
        },
    }
}
