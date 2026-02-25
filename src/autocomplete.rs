use crate::app::Model;
use crate::commands::{CommandRegistry, filter_commands};
use tui_components::selection::{
    SelectionItem, SelectionList, SelectionListConfig, standard_popup_hint_line,
};

/// Updates autocomplete state based on current textarea input
/// Triggers autocomplete when input starts with "/" at the beginning only
pub fn update_autocomplete_state(model: &mut Model, input: &str, registry: &CommandRegistry) {
    let trimmed = input.trim();

    // Only show autocomplete if input starts with "/"
    if let Some(prefix) = trimmed.strip_prefix('/') {
        let all_commands = registry.get_all_command_names();
        let filtered_commands = filter_commands(prefix, &all_commands);

        // Show autocomplete if we have matches OR if prefix is empty (show all commands)
        model.show_autocomplete = !filtered_commands.is_empty() || prefix.is_empty();

        if model.show_autocomplete {
            // Create SelectionItems for the filtered commands
            let items: Vec<SelectionItem<String>> = filtered_commands
                .into_iter()
                .enumerate()
                .map(|(i, cmd)| SelectionItem {
                    data: cmd.clone(),
                    name: format!("/{cmd}"),
                    description: None,
                    selected_description: None,
                    is_current: i == 0,
                    display_shortcut: None,
                    search_value: Some(cmd.to_lowercase()),
                })
                .collect();

            let config = SelectionListConfig::new()
                .with_title("Commands")
                .with_footer_hint(standard_popup_hint_line());

            model.autocomplete_selection_list = SelectionList::new(config, items, Box::new(()));
        }
    } else {
        // Hide autocomplete when input doesn't start with "/"
        model.show_autocomplete = false;
        // Clear the autocomplete list
        let config = SelectionListConfig::new()
            .with_title("Commands")
            .with_footer_hint(standard_popup_hint_line());
        model.autocomplete_selection_list = SelectionList::new(config, vec![], Box::new(()));
    }
}
