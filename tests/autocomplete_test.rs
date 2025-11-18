use nori_cli::app::Model;
use nori_cli::autocomplete::update_autocomplete_state;
use nori_cli::commands::CommandRegistry;

#[test]
fn test_autocomplete_shows_on_slash() {
    // Arrange: Create model and registry
    let mut model = Model::default();
    let registry = CommandRegistry::default();

    // Act: Update autocomplete state with "/" input
    update_autocomplete_state(&mut model, "/", &registry);

    // Assert: Autocomplete is visible with all commands
    assert!(
        model.show_autocomplete,
        "Autocomplete should be visible when input is '/'"
    );
    assert_eq!(
        model.autocomplete_selection_list.selected_index(),
        Some(0),
        "Should select first item"
    );
}

#[test]
fn test_autocomplete_filters_on_prefix() {
    // Arrange: Create model and registry
    let mut model = Model::default();
    let registry = CommandRegistry::default();

    // Act: Update autocomplete state with "/mod" input
    update_autocomplete_state(&mut model, "/mod", &registry);

    // Assert: Autocomplete shows only matching command
    assert!(
        model.show_autocomplete,
        "Autocomplete should be visible with matching prefix"
    );
    assert_eq!(
        model.autocomplete_selection_list.selected_index(),
        Some(0),
        "Should have a selection"
    );
    if let Some(item) = model.autocomplete_selection_list.selected_item() {
        assert_eq!(item.data, "model", "Should filter to matching command");
    }
}

#[test]
fn test_autocomplete_hides_on_non_slash() {
    // Arrange: Create model and registry
    let mut model = Model::default();
    let registry = CommandRegistry::default();

    // Act: Update autocomplete state with regular text
    update_autocomplete_state(&mut model, "hello world", &registry);

    // Assert: Autocomplete is not visible
    assert!(
        !model.show_autocomplete,
        "Autocomplete should be hidden for non-slash input"
    );
    assert!(
        model.autocomplete_selection_list.selected_index().is_none(),
        "Should have no selection when empty"
    );
}

#[test]
fn test_autocomplete_hides_on_empty_string() {
    // Arrange: Create model and registry
    let mut model = Model::default();
    let registry = CommandRegistry::default();

    // Act: Update autocomplete state with empty string
    update_autocomplete_state(&mut model, "", &registry);

    // Assert: Autocomplete is not visible
    assert!(
        !model.show_autocomplete,
        "Autocomplete should be hidden for empty input"
    );
}

#[test]
fn test_autocomplete_resets_selection_on_filter_change() {
    // Arrange: Create model and registry
    let mut model = Model::default();
    let registry = CommandRegistry::default();

    // Act: Update autocomplete state (changes filter)
    update_autocomplete_state(&mut model, "/e", &registry);

    // Assert: Selection is reset to first item
    assert_eq!(
        model.autocomplete_selection_list.selected_index(),
        Some(0),
        "Selection should reset to 0 when filter changes"
    );
}
