#[path = "nori_storybook/menu_storybook.rs"]
mod menu_storybook;
mod support;

use anyhow::Result;
use codex_tui_components::EmptyState;
use codex_tui_components::KeyHint;
use codex_tui_components::KeyHints;
use codex_tui_components::Markdown;
use codex_tui_components::MessageLevel;
use codex_tui_components::Picker;
use codex_tui_components::PickerAction;
use codex_tui_components::PickerColumn;
use codex_tui_components::PickerColumnWidth;
use codex_tui_components::PickerDensity;
use codex_tui_components::PickerDetail;
use codex_tui_components::PickerItem;
use codex_tui_components::PickerLoadState;
use codex_tui_components::PickerMode;
use codex_tui_components::PickerOutcome;
use codex_tui_components::PickerState;
use codex_tui_components::SearchMode;
use codex_tui_components::SemanticMessage;
use codex_tui_components::Theme;
use crossterm::event::Event;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use ratatui::layout::Alignment;
use ratatui::layout::Constraint;
use ratatui::layout::Layout;
use ratatui::layout::Margin;
use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Block;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;

use support::StorybookTerminal;

const MARKDOWN_SAMPLE: &str = r#"# Shared Markdown

Nori renders **strong text**, *emphasis*, `inline code`, links, and adaptive
tables from caller-owned source.

| Session | Project | Updated | Turn status |
| :-- | :-- | --: | :-- |
| Fix parser recovery | nori-cli | 2m | Working |
| Improve Markdown tables | external-codex | 18m | Waiting |
| Rework Handroll UI | sessions | 1h | Ready |

Narrow the terminal until the grid becomes stacked records.
"#;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Page {
    #[default]
    Picker,
    Markdown,
    Primitives,
    States,
    OverlayMenu,
}

fn main() -> Result<()> {
    let mut terminal = StorybookTerminal::enter()?;
    let theme = terminal.theme;
    let mut page = Page::default();
    let mut density = PickerDensity::Normal;
    let mut state = picker_state();
    let mut menu_story = menu_storybook::MenuStory::Action;
    let mut notice = "Resize the terminal to exercise responsive layout".to_string();

    loop {
        terminal.terminal.draw(|frame| {
            render_navigation(frame.area(), frame.buffer_mut(), page, theme);
            let content = Rect::new(
                frame.area().x,
                frame.area().y.saturating_add(1),
                frame.area().width,
                frame.area().height.saturating_sub(1),
            );
            match page {
                Page::Picker => {
                    frame.render_widget(Picker::new(&state).theme(theme).density(density), content);
                    render_storybook_footer(content, frame.buffer_mut(), density, &notice, theme);
                }
                Page::Markdown => render_markdown(content, frame.buffer_mut(), theme),
                Page::Primitives => render_primitives(content, frame.buffer_mut(), theme),
                Page::States => render_states(content, frame.buffer_mut(), theme),
                Page::OverlayMenu => {
                    menu_storybook::render(content, frame.buffer_mut(), theme, menu_story)
                }
            }
        })?;

        let Some(Event::Key(key)) = terminal.next_event()? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => break,
            KeyCode::Char('1') => page = Page::Picker,
            KeyCode::Char('2') => page = Page::Markdown,
            KeyCode::Char('3') => page = Page::Primitives,
            KeyCode::Char('4') => page = Page::States,
            KeyCode::Char('5') => page = Page::OverlayMenu,
            KeyCode::Char('a') if page == Page::OverlayMenu => {
                menu_story = menu_storybook::MenuStory::Action;
            }
            KeyCode::Char('s') if page == Page::OverlayMenu => {
                menu_story = menu_storybook::MenuStory::Shortcuts;
            }
            KeyCode::Char('n') if page == Page::OverlayMenu => {
                menu_story = menu_storybook::MenuStory::Narrow;
            }
            KeyCode::Char('d') if page == Page::OverlayMenu => {
                menu_story = menu_storybook::MenuStory::Destructive;
            }
            KeyCode::Char('d') if page == Page::Picker => {
                density = match density {
                    PickerDensity::Compact => PickerDensity::Normal,
                    PickerDensity::Normal => PickerDensity::Compact,
                };
                notice = format!("Density changed to {density:?}").to_lowercase();
            }
            KeyCode::Char('m') if page == Page::Picker => {
                state.mode = match state.mode {
                    PickerMode::Single => PickerMode::Multi,
                    PickerMode::Toggle | PickerMode::Multi => PickerMode::Single,
                };
                notice = format!("Selection mode changed to {:?}", state.mode).to_lowercase();
            }
            KeyCode::Char('s') if page == Page::Picker => {
                state.load_state = match &state.load_state {
                    PickerLoadState::Ready => {
                        PickerLoadState::Loading("Refreshing ACP sessions...".to_string())
                    }
                    PickerLoadState::Loading(_) => {
                        PickerLoadState::Failed("The session list request timed out".to_string())
                    }
                    PickerLoadState::Failed(_) => PickerLoadState::Ready,
                };
            }
            _ if page == Page::Picker => {
                if let Some(action) = picker_action(key) {
                    match state.handle(action) {
                        PickerOutcome::Selected(key) => notice = format!("Selected {key}"),
                        PickerOutcome::Submitted(keys) => {
                            notice = format!("Submitted {} sessions", keys.len())
                        }
                        PickerOutcome::Cancelled => break,
                        PickerOutcome::Unchanged
                        | PickerOutcome::SelectionChanged(_)
                        | PickerOutcome::Toggled { .. }
                        | PickerOutcome::QueryChanged(_)
                        | PickerOutcome::CategoryChanged(_) => {}
                    }
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn render_navigation(area: Rect, buf: &mut ratatui::buffer::Buffer, page: Page, theme: Theme) {
    Block::default().style(theme.surface).render(area, buf);
    let labels = [
        (Page::Picker, "1 Picker"),
        (Page::Markdown, "2 Markdown"),
        (Page::Primitives, "3 Primitives"),
        (Page::States, "4 States"),
        (Page::OverlayMenu, "5 Overlay menu"),
    ];
    let spans = labels
        .into_iter()
        .enumerate()
        .flat_map(|(index, (candidate, label))| {
            let gap = (index > 0).then(|| Span::raw("   "));
            let style = if candidate == page {
                theme.accent
            } else {
                theme.muted
            };
            gap.into_iter().chain([Span::styled(label, style)])
        })
        .collect::<Vec<_>>();
    Paragraph::new(Line::from(spans))
        .alignment(Alignment::Center)
        .render(Rect::new(area.x, area.y, area.width, 1), buf);
}

fn render_storybook_footer(
    area: Rect,
    buf: &mut ratatui::buffer::Buffer,
    density: PickerDensity,
    notice: &str,
    theme: Theme,
) {
    let footer = Rect::new(
        area.x.saturating_add(2),
        area.bottom().saturating_sub(2),
        area.width.saturating_sub(4),
        1,
    );
    buf.set_style(footer, theme.surface);
    KeyHints::new([
        KeyHint::new("d", format!("{density:?} density").to_lowercase()),
        KeyHint::new("m", "selection mode"),
        KeyHint::new("s", "content state"),
        KeyHint::new("q", "close"),
    ])
    .theme(theme)
    .render(footer, buf);
    if area.height > 4 {
        Paragraph::new(notice.to_string())
            .style(theme.muted)
            .alignment(Alignment::Center)
            .render(
                Rect::new(
                    area.x.saturating_add(2),
                    area.bottom().saturating_sub(3),
                    area.width.saturating_sub(4),
                    1,
                ),
                buf,
            );
    }
}

fn render_markdown(area: Rect, buf: &mut ratatui::buffer::Buffer, theme: Theme) {
    let inner = page_frame(area, buf, "Markdown", theme);
    let text = Markdown::new(MARKDOWN_SAMPLE)
        .theme(theme)
        .width(inner.width)
        .render_text();
    Paragraph::new(text).render(inner, buf);
    render_page_footer(area, buf, theme);
}

fn render_primitives(area: Rect, buf: &mut ratatui::buffer::Buffer, theme: Theme) {
    let inner = page_frame(area, buf, "Primitives", theme);
    let sections = Layout::vertical([
        Constraint::Length(3),
        Constraint::Length(9),
        Constraint::Min(3),
    ])
    .split(inner);
    Paragraph::new(vec![
        Line::styled("Semantic tokens", theme.title),
        Line::from(vec![
            Span::styled("accent   ", theme.accent),
            Span::styled("success   ", theme.success),
            Span::styled("warning   ", theme.warning),
            Span::styled("error   ", theme.error),
            Span::styled("supporting", theme.muted),
        ]),
    ])
    .render(sections[0], buf);
    let messages = [
        SemanticMessage::new(MessageLevel::Info, "Connected to the agent"),
        SemanticMessage::new(MessageLevel::Success, "Snapshot suite passed"),
        SemanticMessage::new(MessageLevel::Warning, "Two sessions are still running")
            .detail("Open the picker to inspect them"),
        SemanticMessage::new(MessageLevel::Error, "Could not resume the session")
            .detail("The agent no longer reports that session id"),
    ];
    for (index, message) in messages.into_iter().enumerate() {
        message.theme(theme).render(
            Rect::new(
                sections[1].x,
                sections[1].y.saturating_add(index as u16 * 2),
                sections[1].width,
                2,
            ),
            buf,
        );
    }
    EmptyState::new("No matching sessions")
        .detail("Try a title, project path, or session id")
        .theme(theme)
        .render(sections[2], buf);
    render_page_footer(area, buf, theme);
}

fn render_states(area: Rect, buf: &mut ratatui::buffer::Buffer, theme: Theme) {
    let inner = page_frame(area, buf, "States", theme);
    let rows = Layout::vertical([
        Constraint::Percentage(34),
        Constraint::Percentage(33),
        Constraint::Percentage(33),
    ])
    .split(inner);
    let examples = [
        EmptyState::new("Loading sessions")
            .marker("◌")
            .detail("Results appear without rebuilding the picker"),
        EmptyState::new("No sessions available").detail("The caller has not supplied any rows"),
        EmptyState::new("No matching sessions").detail("Try a different search term or category"),
    ];
    for (index, example) in examples.into_iter().enumerate() {
        let style = if index % 2 == 0 {
            theme.row_alt
        } else {
            theme.row
        };
        Block::default().style(style).render(rows[index], buf);
        example.theme(theme).render(
            rows[index].inner(Margin {
                horizontal: 1,
                vertical: 1,
            }),
            buf,
        );
    }
    render_page_footer(area, buf, theme);
}

fn page_frame(area: Rect, buf: &mut ratatui::buffer::Buffer, title: &str, theme: Theme) -> Rect {
    Block::default().style(theme.surface).render(area, buf);
    let inner = area.inner(Margin {
        horizontal: 2,
        vertical: 1,
    });
    Paragraph::new(Line::styled(title.to_string(), theme.title))
        .render(Rect::new(inner.x, inner.y, inner.width, 1), buf);
    Rect::new(
        inner.x,
        inner.y.saturating_add(2),
        inner.width,
        inner.height.saturating_sub(4),
    )
}

fn render_page_footer(area: Rect, buf: &mut ratatui::buffer::Buffer, theme: Theme) {
    KeyHints::new([KeyHint::new("1-5", "page"), KeyHint::new("q", "close")])
        .theme(theme)
        .render(
            Rect::new(
                area.x.saturating_add(2),
                area.bottom().saturating_sub(2),
                area.width.saturating_sub(4),
                1,
            ),
            buf,
        );
}

fn picker_action(key: KeyEvent) -> Option<PickerAction> {
    match key.code {
        KeyCode::Up => Some(PickerAction::MoveUp),
        KeyCode::Down => Some(PickerAction::MoveDown),
        KeyCode::PageUp => Some(PickerAction::PageUp),
        KeyCode::PageDown => Some(PickerAction::PageDown),
        KeyCode::Home => Some(PickerAction::First),
        KeyCode::End => Some(PickerAction::Last),
        KeyCode::Enter => Some(PickerAction::Submit),
        KeyCode::Char(' ') => Some(PickerAction::Toggle),
        KeyCode::Backspace => Some(PickerAction::Backspace),
        KeyCode::Tab => Some(PickerAction::NextCategory),
        KeyCode::BackTab => Some(PickerAction::PreviousCategory),
        KeyCode::Char(character) => Some(PickerAction::AppendQuery(character)),
        _ => None,
    }
}

fn picker_state() -> PickerState<String> {
    let columns = [
        PickerColumn::flexible("title", "Session").width(PickerColumnWidth::Flexible {
            min: 18,
            max: 40,
            weight: 3,
        }),
        PickerColumn::flexible("project", "Project").hide_below(58),
        PickerColumn::fixed("updated", "Updated", 10),
        PickerColumn::fixed("status", "Turn status", 13).hide_below(82),
    ];
    let items = [
        PickerItem::new("new".to_string(), "title", "Start a new session")
            .cell("project", "Not reported")
            .cell("updated", "now")
            .cell("status", "ready")
            .search_text("start create new")
            .pinned(true)
            .category("Local")
            .description("Create a fresh ACP session")
            .details([
                PickerDetail::new("Action", "Create a fresh ACP session"),
                PickerDetail::new("Transcript", "No transcript will be loaded"),
            ]),
        PickerItem::new("parser".to_string(), "title", "Fix parser recovery")
            .cell("project", "nori-cli")
            .cell("updated", "2m ago")
            .cell("status", "working")
            .search_text("fix parser recovery nori cli session 019f")
            .current(true)
            .category("Local")
            .description("Codex is implementing parser recovery")
            .details([
                PickerDetail::new("Agent", "Codex"),
                PickerDetail::new("Path", "/workspace/nori/cli"),
                PickerDetail::new("Current turn", "Implementing parser recovery"),
            ]),
        PickerItem::new("markdown".to_string(), "title", "Improve Markdown tables")
            .cell("project", "external-codex")
            .cell("updated", "18m ago")
            .cell("status", "waiting")
            .search_text("markdown tables codex waiting")
            .category("Local")
            .description("Waiting for user input")
            .details([
                PickerDetail::new("Agent", "Codex"),
                PickerDetail::new("Current turn", "Waiting for user input"),
            ]),
        PickerItem::new("cloud".to_string(), "title", "Slack · Claude")
            .cell("project", "Nori Sessions")
            .cell("updated", "1h ago")
            .cell("status", "ready")
            .search_text("slack claude cloud sessions")
            .category("Cloud")
            .description("The broker owns this remote session")
            .details([
                PickerDetail::new("Origin", "Nori cloud"),
                PickerDetail::new("Ownership", "Broker-managed remote session"),
            ]),
        PickerItem::new("offline".to_string(), "title", "Unavailable legacy session")
            .cell("project", "handroll")
            .cell("updated", "3d ago")
            .cell("status", "offline")
            .search_text("legacy handroll offline")
            .disabled(true)
            .category("Cloud")
            .description("This legacy session cannot be resumed"),
    ];
    PickerState::new("Nori component storybook", columns, items)
        .subtitle("Search ACP sessions or start fresh")
        .mode(PickerMode::Single)
        .search_mode(SearchMode::Fuzzy)
        .categories(["Local", "Cloud"])
        .search_placeholder("Title, project, or session id")
}
