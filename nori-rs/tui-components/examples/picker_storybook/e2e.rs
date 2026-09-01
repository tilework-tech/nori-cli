#[path = "../support/e2e.rs"]
mod e2e;
use anyhow::Result;
use e2e::TuiSession;
use e2e::assert_screen;

#[test]
#[ignore = "requires built storybooks and TUI_PUPPETEERING_DIR; run scripts/storybook-e2e.sh"]
fn navigation_and_search() -> Result<()> {
    let tui = TuiSession::start("picker_storybook", 120, 40)?;
    tui.expect("Picker storybook")?;
    tui.key("Down")?;
    tui.key("Enter")?;
    tui.expect("Selected: parser")?;
    assert_screen!(tui, "selected_120x40");
    tui.send("/markdown")?;
    tui.expect("markdown")?;
    tui.key("Enter")?;
    tui.expect("Selected: markdown")?;
    assert_screen!(tui, "search_120x40");
    Ok(())
}
