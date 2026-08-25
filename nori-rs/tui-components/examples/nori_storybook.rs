#[path = "nori_storybook/menu_storybook.rs"]
mod menu_storybook;
mod support;

use anyhow::Result;
use crossterm::event::Event;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use nori_tui_components::DetailDensity;
use nori_tui_components::DetailEntry;
use nori_tui_components::DetailLayout;
use nori_tui_components::DetailPane;
use nori_tui_components::DetailRowPattern;
use nori_tui_components::DetailTone;
use nori_tui_components::EmptyState;
use nori_tui_components::KeyHint;
use nori_tui_components::KeyHints;
use nori_tui_components::LabelWidth;
use nori_tui_components::Markdown;
use nori_tui_components::MenuShortcut;
use nori_tui_components::MessageLevel;
use nori_tui_components::Picker;
use nori_tui_components::PickerAction;
use nori_tui_components::PickerColumn;
use nori_tui_components::PickerColumnWidth;
use nori_tui_components::PickerDensity;
use nori_tui_components::PickerDetail;
use nori_tui_components::PickerItem;
use nori_tui_components::PickerLoadState;
use nori_tui_components::PickerMode;
use nori_tui_components::PickerOutcome;
use nori_tui_components::PickerState;
use nori_tui_components::ProviderKind;
use nori_tui_components::SearchMode;
use nori_tui_components::SemanticMessage;
use nori_tui_components::Theme;
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
    Details,
    OverlayMenu,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum DetailStory {
    #[default]
    AutoWithHeading,
    Zebra,
    NormalDensity,
    ResponsiveStacked,
    FixedWithHeading,
    WithoutHeading,
}

impl DetailStory {
    fn next(self) -> Self {
        match self {
            Self::AutoWithHeading => Self::Zebra,
            Self::Zebra => Self::NormalDensity,
            Self::NormalDensity => Self::ResponsiveStacked,
            Self::ResponsiveStacked => Self::FixedWithHeading,
            Self::FixedWithHeading => Self::WithoutHeading,
            Self::WithoutHeading => Self::AutoWithHeading,
        }
    }

    fn previous(self) -> Self {
        match self {
            Self::AutoWithHeading => Self::WithoutHeading,
            Self::Zebra => Self::AutoWithHeading,
            Self::NormalDensity => Self::Zebra,
            Self::ResponsiveStacked => Self::NormalDensity,
            Self::FixedWithHeading => Self::ResponsiveStacked,
            Self::WithoutHeading => Self::FixedWithHeading,
        }
    }
}

fn overlay_menu_action(page: Page, key: KeyEvent) -> Option<menu_storybook::MenuAction> {
    if page != Page::OverlayMenu {
        return None;
    }
    match key.code {
        KeyCode::Up => Some(menu_storybook::MenuAction::MoveUp),
        KeyCode::Down => Some(menu_storybook::MenuAction::MoveDown),
        KeyCode::Enter => Some(menu_storybook::MenuAction::ActivateSelected),
        KeyCode::Char(character) if character.eq_ignore_ascii_case(&'k') => {
            Some(menu_storybook::MenuAction::MoveUp)
        }
        KeyCode::Char(character) if character.eq_ignore_ascii_case(&'j') => {
            Some(menu_storybook::MenuAction::MoveDown)
        }
        KeyCode::Char('1') => Some(menu_storybook::MenuAction::InvokeShortcut(
            MenuShortcut::Number(1),
        )),
        KeyCode::Char('2') => Some(menu_storybook::MenuAction::InvokeShortcut(
            MenuShortcut::Number(2),
        )),
        KeyCode::Char('3') => Some(menu_storybook::MenuAction::InvokeShortcut(
            MenuShortcut::Number(3),
        )),
        KeyCode::Char('4') => Some(menu_storybook::MenuAction::InvokeShortcut(
            MenuShortcut::Number(4),
        )),
        KeyCode::Char('5') => Some(menu_storybook::MenuAction::InvokeShortcut(
            MenuShortcut::Number(5),
        )),
        KeyCode::Char(character) if character.is_ascii_alphabetic() => Some(
            menu_storybook::MenuAction::InvokeShortcut(MenuShortcut::Character(character)),
        ),
        _ => None,
    }
}

fn overlay_page_navigation(page: Page, key: KeyEvent) -> Option<Page> {
    if page != Page::OverlayMenu {
        return None;
    }
    match key.code {
        KeyCode::Left => Some(Page::Details),
        KeyCode::Right => Some(Page::Picker),
        _ => None,
    }
}

fn main() -> Result<()> {
    let mut terminal = StorybookTerminal::enter()?;
    let theme = terminal.theme;
    let mut page = Page::default();
    let mut density = PickerDensity::Normal;
    let mut detail_story = DetailStory::default();
    let mut state = picker_state();
    let mut menu_state = menu_storybook::MenuStoryState::new(menu_storybook::MenuStory::Action)?;
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
                Page::Details => render_details(content, frame.buffer_mut(), theme, detail_story),
                Page::OverlayMenu => {
                    menu_storybook::render(content, frame.buffer_mut(), theme, &mut menu_state)
                }
            }
        })?;

        let Some(Event::Key(key)) = terminal.next_event()? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        if page == Page::OverlayMenu {
            if let Some(next_page) = overlay_page_navigation(page, key) {
                page = next_page;
                continue;
            }
            match key.code {
                KeyCode::Esc | KeyCode::Char('q') => break,
                KeyCode::Tab | KeyCode::Char(']') => menu_state.next_story()?,
                KeyCode::BackTab | KeyCode::Char('[') => menu_state.previous_story()?,
                _ => {
                    if let Some(action) = overlay_menu_action(page, key) {
                        menu_state.handle(action);
                    }
                }
            }
            continue;
        }
        match key.code {
            KeyCode::Esc if page == Page::Picker && state.search_active => {
                state.handle(PickerAction::DeactivateSearch);
            }
            KeyCode::Esc | KeyCode::Char('q')
                if !picker_owns_global_shortcuts(page, state.search_active) =>
            {
                break;
            }
            KeyCode::Char('1') if !picker_owns_global_shortcuts(page, state.search_active) => {
                page = Page::Picker;
            }
            KeyCode::Char('2') if !picker_owns_global_shortcuts(page, state.search_active) => {
                page = Page::Markdown;
            }
            KeyCode::Char('3') if !picker_owns_global_shortcuts(page, state.search_active) => {
                page = Page::Primitives;
            }
            KeyCode::Char('4') if !picker_owns_global_shortcuts(page, state.search_active) => {
                page = Page::States;
            }
            KeyCode::Char('5') if !picker_owns_global_shortcuts(page, state.search_active) => {
                page = Page::Details;
            }
            KeyCode::Char('6') if !picker_owns_global_shortcuts(page, state.search_active) => {
                page = Page::OverlayMenu;
            }
            KeyCode::Tab if page == Page::Details => {
                detail_story = detail_story.next();
            }
            KeyCode::BackTab if page == Page::Details => {
                detail_story = detail_story.previous();
            }
            KeyCode::Char('d') if page == Page::Picker && !state.search_active => {
                density = match density {
                    PickerDensity::Compact => PickerDensity::Normal,
                    PickerDensity::Normal => PickerDensity::Compact,
                };
                notice = format!("Density changed to {density:?}").to_lowercase();
            }
            KeyCode::Char('m') if page == Page::Picker && !state.search_active => {
                state.mode = match state.mode {
                    PickerMode::Single => PickerMode::Multi,
                    PickerMode::Toggle | PickerMode::Multi => PickerMode::Single,
                };
                notice = format!("Selection mode changed to {:?}", state.mode).to_lowercase();
            }
            KeyCode::Char('s') if page == Page::Picker && !state.search_active => {
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
                if let Some(action) = picker_action(key, state.search_active) {
                    match state.handle(action) {
                        PickerOutcome::Selected(key) => notice = format!("Selected {key}"),
                        PickerOutcome::Submitted(keys) => {
                            notice = format!("Submitted {} sessions", keys.len())
                        }
                        PickerOutcome::Cancelled => break,
                        PickerOutcome::Unchanged
                        | PickerOutcome::SelectionChanged(_)
                        | PickerOutcome::Toggled { .. }
                        | PickerOutcome::SearchModeChanged(_)
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

fn picker_owns_global_shortcuts(page: Page, search_active: bool) -> bool {
    page == Page::Picker && search_active
}

fn render_navigation(area: Rect, buf: &mut ratatui::buffer::Buffer, page: Page, theme: Theme) {
    Block::default().style(theme.surface).render(area, buf);
    if page == Page::OverlayMenu {
        Paragraph::new(Line::from(vec![
            Span::styled("← previous page", theme.muted),
            Span::raw("   "),
            Span::styled("Overlay menu", theme.accent),
            Span::raw("   "),
            Span::styled("next page →", theme.muted),
        ]))
        .alignment(Alignment::Center)
        .render(Rect::new(area.x, area.y, area.width, 1), buf);
        return;
    }
    let labels = [
        (Page::Picker, "1 Picker"),
        (Page::Markdown, "2 Markdown"),
        (Page::Primitives, "3 Primitives"),
        (Page::States, "4 States"),
        (Page::Details, "5 Details"),
        (Page::OverlayMenu, "6 Overlay menu"),
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

fn render_details(area: Rect, buf: &mut ratatui::buffer::Buffer, theme: Theme, story: DetailStory) {
    let inner = page_frame(area, buf, "Detail pane", theme);
    let sections = Layout::vertical([Constraint::Length(4), Constraint::Min(1)]).split(inner);
    let (story_name, story_description) = match story {
        DetailStory::AutoWithHeading => (
            "Default · compact columns + heading",
            "Automatic label width with the approved compact two-column layout.",
        ),
        DetailStory::Zebra => (
            "Zebra · compact columns + heading",
            "Full-width row bands group wrapped logical entries and reset by section.",
        ),
        DetailStory::NormalDensity => (
            "Normal density · columns + heading",
            "Adjacent entries receive one blank row without doubling section spacing.",
        ),
        DetailStory::ResponsiveStacked => (
            "Responsive · stack below 120 columns",
            "Narrow labels sit above values, which retain a two-cell inset.",
        ),
        DetailStory::FixedWithHeading => (
            "Fixed label width + heading",
            "A caller-selected gutter keeps the value column stable across panes.",
        ),
        DetailStory::WithoutHeading => (
            "Default · heading omitted",
            "The pane surface and two-column alignment do not depend on a heading.",
        ),
    };

    let entries = detail_entries();
    let pane = DetailPane::new(&entries).theme(theme);
    let pane = match story {
        DetailStory::AutoWithHeading => pane.heading("Session details"),
        DetailStory::Zebra => pane
            .heading("Session details")
            .row_pattern(DetailRowPattern::Zebra),
        DetailStory::NormalDensity => pane
            .heading("Session details")
            .density(DetailDensity::Normal),
        DetailStory::ResponsiveStacked => pane
            .heading("Session details")
            .layout(DetailLayout::Responsive { stack_below: 120 })
            .row_pattern(DetailRowPattern::Zebra),
        DetailStory::FixedWithHeading => pane
            .heading("Session details")
            .label_width(LabelWidth::Fixed(12)),
        DetailStory::WithoutHeading => pane,
    };
    let required_height = pane.required_height(sections[1].width);
    Paragraph::new(vec![
        Line::styled(story_name, theme.title),
        Line::styled(story_description, theme.muted),
        Line::styled(
            format!(
                "Required height at {} columns: {required_height} rows",
                sections[1].width
            ),
            theme.muted,
        ),
    ])
    .render(sections[0], buf);
    pane.render(sections[1], buf);

    KeyHints::new([
        KeyHint::new("Tab/Shift-Tab", "detail story"),
        KeyHint::new("1-6", "page"),
        KeyHint::new("q", "close"),
    ])
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

fn detail_entries() -> Vec<DetailEntry> {
    vec![
        DetailEntry::key_value("Thread", "Fix parser recovery"),
        DetailEntry::key_value("Agent", "Codex").tone(DetailTone::Provider(ProviderKind::Codex)),
        DetailEntry::key_value("Origin", "Local · nori-cli"),
        DetailEntry::key_value("Created", "Aug 16, 2026 · 9:42 AM"),
        DetailEntry::key_value("Modified", "Aug 17, 2026 · 11:08 AM"),
        DetailEntry::key_value("Turns", "48"),
        DetailEntry::key_value("Transcript", "1.8 MB"),
        DetailEntry::Rule,
        DetailEntry::muted(
            "Latest prompt",
            "Recover malformed session transcripts without changing user-visible behavior.",
        ),
        DetailEntry::muted(
            "Latest response",
            "Inspecting parser recovery fixtures and structured error handling.",
        ),
    ]
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
    KeyHints::new([KeyHint::new("1-6", "page"), KeyHint::new("q", "close")])
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

fn picker_action(key: KeyEvent, search_active: bool) -> Option<PickerAction> {
    match key.code {
        KeyCode::Up => Some(PickerAction::MoveUp),
        KeyCode::Down => Some(PickerAction::MoveDown),
        KeyCode::PageUp => Some(PickerAction::PageUp),
        KeyCode::PageDown => Some(PickerAction::PageDown),
        KeyCode::Home => Some(PickerAction::First),
        KeyCode::End => Some(PickerAction::Last),
        KeyCode::Enter => Some(PickerAction::Submit),
        KeyCode::Char(' ') if !search_active => Some(PickerAction::Toggle),
        KeyCode::Backspace if search_active => Some(PickerAction::Backspace),
        KeyCode::Tab => Some(PickerAction::NextCategory),
        KeyCode::BackTab => Some(PickerAction::PreviousCategory),
        KeyCode::Char('f')
            if !search_active && key.modifiers == crossterm::event::KeyModifiers::CONTROL =>
        {
            Some(PickerAction::ActivateSearch)
        }
        KeyCode::Char('f' | '/') if !search_active && key.modifiers.is_empty() => {
            Some(PickerAction::ActivateSearch)
        }
        KeyCode::Char('k') if !search_active && key.modifiers.is_empty() => {
            Some(PickerAction::MoveUp)
        }
        KeyCode::Char('j') if !search_active && key.modifiers.is_empty() => {
            Some(PickerAction::MoveDown)
        }
        KeyCode::Char(character) if search_active => Some(PickerAction::AppendQuery(character)),
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
        PickerColumn::fixed("type", "Type", 12),
        PickerColumn::flexible("project", "Project").hide_below(58),
        PickerColumn::fixed("updated", "Updated", 10),
        PickerColumn::fixed("status", "Turn status", 13).hide_below(82),
    ];
    let items = [
        PickerItem::new("new".to_string(), "title", "Start a new session")
            .cell("type", "Nori")
            .cell_tone("type", ProviderKind::Nori)
            .cell("project", "Not reported")
            .cell("updated", "now")
            .cell("status", "ready")
            .search_text("start create new")
            .pinned(true)
            .category("Nori")
            .description("Create a fresh ACP session")
            .details([
                PickerDetail::new("Action", "Create a fresh ACP session"),
                PickerDetail::new("Transcript", "No transcript will be loaded"),
            ]),
        PickerItem::new("parser".to_string(), "title", "Fix parser recovery")
            .cell("type", "Codex")
            .cell_tone("type", ProviderKind::Codex)
            .cell("project", "nori-cli")
            .cell("updated", "2m ago")
            .cell("status", "working")
            .search_text("fix parser recovery nori cli session 019f")
            .current(true)
            .category("Codex")
            .description("Codex is implementing parser recovery")
            .details([
                PickerDetail::new("Agent", "Codex"),
                PickerDetail::new("Path", "/workspace/nori/cli"),
                PickerDetail::new("Current turn", "Implementing parser recovery"),
            ]),
        PickerItem::new("markdown".to_string(), "title", "Improve Markdown tables")
            .cell("type", "Gemini")
            .cell_tone("type", ProviderKind::Gemini)
            .cell("project", "external-codex")
            .cell("updated", "18m ago")
            .cell("status", "waiting")
            .search_text("markdown tables codex waiting")
            .category("Gemini")
            .description("Waiting for user input")
            .details([
                PickerDetail::new("Agent", "Codex"),
                PickerDetail::new("Current turn", "Waiting for user input"),
            ]),
        PickerItem::new("cloud".to_string(), "title", "Slack · Claude")
            .cell("type", "Claude")
            .cell_tone("type", ProviderKind::Claude)
            .cell("project", "Nori Sessions")
            .cell("updated", "1h ago")
            .cell("status", "ready")
            .search_text("slack claude cloud sessions")
            .category("Claude")
            .description("The broker owns this remote session")
            .details([
                PickerDetail::new("Origin", "Nori cloud"),
                PickerDetail::new("Ownership", "Broker-managed remote session"),
            ]),
        PickerItem::new("offline".to_string(), "title", "Unavailable legacy session")
            .cell("type", "Antigravity")
            .cell_tone("type", ProviderKind::Antigravity)
            .cell("project", "handroll")
            .cell("updated", "3d ago")
            .cell("status", "offline")
            .search_text("legacy handroll offline")
            .disabled(true)
            .category("Antigravity")
            .description("This legacy session cannot be resumed"),
    ];
    PickerState::new("Nori component storybook", columns, items)
        .subtitle("Search ACP sessions or start fresh")
        .mode(PickerMode::Single)
        .search_mode(SearchMode::Fuzzy)
        .categories(["Claude", "Codex", "Gemini", "Antigravity", "Nori"])
        .category_tone("Claude", ProviderKind::Claude)
        .category_tone("Codex", ProviderKind::Codex)
        .category_tone("Gemini", ProviderKind::Gemini)
        .category_tone("Antigravity", ProviderKind::Antigravity)
        .category_tone("Nori", ProviderKind::Nori)
        .search_placeholder("Title, project, or session id")
}

#[cfg(test)]
#[path = "nori_storybook/navigation_tests.rs"]
mod navigation_tests;
