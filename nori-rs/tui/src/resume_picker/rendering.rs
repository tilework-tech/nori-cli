use nori_tui_components::KeyHint;
use nori_tui_components::Picker;
use nori_tui_components::PickerColumn;
use nori_tui_components::PickerColumnWidth;
use nori_tui_components::PickerDensity;
use nori_tui_components::PickerDetail;
use nori_tui_components::PickerItem;
use nori_tui_components::PickerState as ComponentPickerState;
use nori_tui_components::SearchMode;
use ratatui::widgets::Widget;

use super::PickerState;
use crate::tui::Tui;

pub(super) fn draw_picker(tui: &mut Tui, state: &PickerState) -> std::io::Result<()> {
    let height = tui.terminal.size()?.height;
    tui.draw(height, |frame| {
        let component_state = component_state(state);
        let area = frame.area();
        picker(&component_state).render(area, frame.buffer_mut());
    })
}

pub(super) fn picker(state: &ComponentPickerState<usize>) -> Picker<'_, usize> {
    let hints = if state.search_active {
        vec![
            KeyHint::new("↑↓", "browse"),
            KeyHint::new("type", "filter"),
            KeyHint::new("enter", "resume"),
            KeyHint::new("esc", "stop search"),
            KeyHint::new("ctrl+c", "quit"),
        ]
    } else {
        vec![
            KeyHint::new("↑↓/j/k", "browse"),
            KeyHint::new("/", "search"),
            KeyHint::new("enter", "resume"),
            KeyHint::new("esc", "start new"),
            KeyHint::new("ctrl+c", "quit"),
        ]
    };
    Picker::new(state)
        .theme(crate::style::component_theme())
        .density(PickerDensity::Compact)
        .fullscreen_selection_rails(true)
        .footer_hints(hints)
}

pub(super) fn component_state(state: &PickerState) -> ComponentPickerState<usize> {
    let mut columns = vec![
        PickerColumn::flexible("conversation", "Conversation").width(PickerColumnWidth::Flexible {
            min: 18,
            max: 48,
            weight: 3,
        }),
        PickerColumn::fixed("updated", "Updated", 16),
        PickerColumn::flexible("branch", "Branch")
            .hide_below(64)
            .width(PickerColumnWidth::Flexible {
                min: 10,
                max: 24,
                weight: 1,
            }),
    ];
    if state.show_all {
        columns.push(
            PickerColumn::flexible("cwd", "Working directory")
                .hide_below(96)
                .width(PickerColumnWidth::Flexible {
                    min: 14,
                    max: 32,
                    weight: 2,
                }),
        );
    }

    let items = state
        .filtered_rows
        .iter()
        .enumerate()
        .map(|(index, row)| {
            let cwd = row
                .cwd
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| "Not reported".to_string());
            let branch = row
                .git_branch
                .clone()
                .unwrap_or_else(|| "Not reported".to_string());
            PickerItem::new(index, "conversation", row.preview.clone())
                .cell("updated", super::helpers::format_updated_label(row))
                .cell("branch", &branch)
                .cell("cwd", &cwd)
                .search_text(row.preview.clone())
                .details([
                    PickerDetail::new("Session", row.target.session_id.clone()),
                    PickerDetail::new("Project", row.target.project_id.clone()),
                    PickerDetail::new(
                        "Agent",
                        row.target
                            .agent
                            .clone()
                            .unwrap_or_else(|| "Not reported".to_string()),
                    ),
                    PickerDetail::new("Working directory", cwd),
                    PickerDetail::new("Branch", branch),
                ])
        })
        .collect::<Vec<_>>();

    let mut picker = ComponentPickerState::new("Resume a previous session", columns, items)
        .subtitle(if state.show_all {
            "All saved sessions"
        } else {
            "Saved sessions for this working directory"
        })
        .search_mode(SearchMode::Custom(include_filtered_item))
        .search_placeholder("Session id or conversation");
    picker.query.clone_from(&state.query);
    picker.search_active = state.search_active;
    picker.selected_index = (!picker.items.is_empty()).then_some(state.selected);
    picker
}

fn include_filtered_item(_query: &str, _search_text: &str) -> Option<u32> {
    Some(0)
}
