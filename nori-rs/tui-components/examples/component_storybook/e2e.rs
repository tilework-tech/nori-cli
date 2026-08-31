#[path = "../support/e2e.rs"]
mod e2e;
use anyhow::Result;
use e2e::TuiSession;
use e2e::assert_screen;

#[test]
#[ignore = "requires built storybooks and TUI_PUPPETEERING_DIR; run scripts/storybook-e2e.sh"]
fn primitives() -> Result<()> {
    let tui = TuiSession::start("component_storybook", 120, 40)?;
    tui.expect("Component storybook")?;
    tui.expect("close storybook")?;
    assert_screen!(tui, "primitives_120x40");
    Ok(())
}
