#[path = "../support/e2e.rs"]
mod e2e;
use anyhow::Result;
use e2e::TuiSession;
use e2e::assert_screen;

#[test]
#[ignore = "requires built storybooks and TUI_PUPPETEERING_DIR; run scripts/storybook-e2e.sh"]
fn prose_and_tables() -> Result<()> {
    let tui = TuiSession::start("markdown_storybook", 120, 40)?;
    tui.expect("Markdown storybook · page 1 of 3")?;
    assert_screen!(tui, "prose_120x40");
    tui.send("2")?;
    tui.expect("Markdown storybook · page 2 of 3")?;
    assert_screen!(tui, "tables_120x40");
    Ok(())
}

#[test]
#[ignore = "requires built storybooks and TUI_PUPPETEERING_DIR; run scripts/storybook-e2e.sh"]
fn narrow_tables() -> Result<()> {
    let tui = TuiSession::start("markdown_storybook", 48, 30)?;
    tui.expect("Markdown storybook · page 1 of 3")?;
    tui.send("2")?;
    tui.expect("Markdown storybook · page 2 of 3")?;
    assert_screen!(tui, "tables_48x30");
    Ok(())
}
