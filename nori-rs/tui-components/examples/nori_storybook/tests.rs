use super::MenuAction;
use super::MenuStory;
use super::MenuStoryState;
use super::render;
use super::state::MenuOutcome;
use codex_tui_components::Theme;
use insta::assert_snapshot;
use pretty_assertions::assert_eq;
use ratatui::Terminal;
use ratatui::backend::TestBackend;

#[test]
fn movement_wraps_and_skips_disabled_items() {
    let mut state = MenuStoryState::new(MenuStory::Shortcuts);

    assert_eq!(
        state.handle(MenuAction::MoveUp),
        MenuOutcome::SelectionChanged("Archive session")
    );
    assert_eq!(
        state.handle(MenuAction::MoveDown),
        MenuOutcome::SelectionChanged("Resume session")
    );
    assert_eq!(
        state.handle(MenuAction::MoveDown),
        MenuOutcome::SelectionChanged("Start a new session")
    );
}

#[test]
fn number_and_character_shortcuts_activate_immediately() {
    let mut state = MenuStoryState::new(MenuStory::Shortcuts);

    assert_eq!(
        state.handle(MenuAction::InvokeNumber(2)),
        MenuOutcome::Activated("Start a new session")
    );
    assert_eq!(
        state.handle(MenuAction::InvokeCharacter('I')),
        MenuOutcome::Activated("Inspect transcript")
    );
    assert_eq!(
        state.handle(MenuAction::InvokeCharacter('a')),
        MenuOutcome::Activated("Archive session")
    );
    assert_eq!(
        state.handle(MenuAction::InvokeNumber(5)),
        MenuOutcome::Unchanged
    );
    assert_eq!(
        state.handle(MenuAction::ActivateSelected),
        MenuOutcome::Activated("Archive session")
    );
}

#[test]
fn selected_rows_use_symmetric_rails_on_both_lines() {
    let rows = rendered_rows(&MenuStoryState::new(MenuStory::Action));
    let label_row = rows
        .iter()
        .position(|row| row.contains("Resume session"))
        .expect("resume item should render");
    let description_row = label_row + 1;
    let label_left = rows[label_row]
        .find('▏')
        .expect("selected label should have a left rail");
    let label_right = rows[label_row]
        .find('▕')
        .expect("selected label should have a right rail");

    assert_eq!(rows[description_row].find('▏'), Some(label_left));
    assert_eq!(rows[description_row].find('▕'), Some(label_right));
    assert!(!rows[label_row].contains('▎'));
    assert!(!rows[description_row].contains('▎'));
}

#[test]
fn double_height_items_have_a_blank_row_between_them() {
    let rows = rendered_rows(&MenuStoryState::new(MenuStory::Action));
    let resume_row = rows
        .iter()
        .position(|row| row.contains("Resume session"))
        .expect("resume item should render");
    let start_row = rows
        .iter()
        .position(|row| row.contains("Start a new session"))
        .expect("start item should render");
    let menu_left = rows[resume_row]
        .chars()
        .position(|character| character == '▏')
        .expect("selected item should show its left rail");
    let menu_right = rows[resume_row]
        .chars()
        .position(|character| character == '▕')
        .expect("selected item should show its right rail");

    assert_eq!(start_row, resume_row + 3);
    assert!(
        rows[resume_row + 2]
            .chars()
            .skip(menu_left)
            .take(menu_right - menu_left + 1)
            .all(|character| character == ' ')
    );
}

#[test]
fn wide_action_menu_snapshot() {
    assert_snapshot!(
        "overlay_menu_wide_action",
        rendered(&MenuStoryState::new(MenuStory::Action), 80, 24)
    );
}

#[test]
fn narrow_scrolled_menu_snapshot() {
    let mut state = MenuStoryState::new(MenuStory::Narrow);
    state.handle(MenuAction::MoveDown);
    state.handle(MenuAction::MoveDown);

    assert_snapshot!("overlay_menu_narrow_scrolled", rendered(&state, 30, 12));
}

fn rendered_rows(state: &MenuStoryState) -> Vec<String> {
    rendered(state, 80, 24)
        .lines()
        .map(str::to_string)
        .collect()
}

fn rendered(state: &MenuStoryState, width: u16, height: u16) -> String {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("test terminal should be created");

    terminal
        .draw(|frame| render(frame.area(), frame.buffer_mut(), Theme::default(), state))
        .expect("storybook should render");
    terminal.backend().to_string()
}
