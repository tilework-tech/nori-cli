use crate::app::Model;
use crate::commands::{CommandRegistry, filter_commands};

/// Updates autocomplete state based on current textarea input
/// Triggers autocomplete when input starts with "/" at the beginning only
pub fn update_autocomplete_state(model: &mut Model, input: &str, registry: &CommandRegistry) {
    let trimmed = input.trim();

    // Only show autocomplete if input starts with "/"
    if let Some(prefix) = trimmed.strip_prefix('/') {
        let all_commands = registry.get_all_command_names();
        model.autocomplete_filtered_commands = filter_commands(prefix, &all_commands);

        // Show autocomplete if we have matches OR if prefix is empty (show all commands)
        model.show_autocomplete =
            !model.autocomplete_filtered_commands.is_empty() || prefix.is_empty();

        // Reset selection to first item whenever filter changes
        model.autocomplete_selected_index = 0;
    } else {
        // Hide autocomplete when input doesn't start with "/"
        model.show_autocomplete = false;
        model.autocomplete_filtered_commands.clear();
        model.autocomplete_selected_index = 0;
    }
}
