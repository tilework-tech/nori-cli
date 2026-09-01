#[path = "../support/e2e.rs"]
mod e2e;
use anyhow::Result;
use e2e::TuiSession;
use e2e::assert_screen;

#[test]
#[ignore = "requires built storybooks and TUI_PUPPETEERING_DIR; run scripts/storybook-e2e.sh"]
fn summary() -> Result<()> {
    let tui = TuiSession::start("status_card_storybook", 120, 40)?;
    tui.expect("Status card specimen")?;
    tui.expect("Background: None")?;
    assert_screen!(tui, "summary_120x40");
    Ok(())
}

#[test]
#[ignore = "requires built storybooks and TUI_PUPPETEERING_DIR; run scripts/storybook-e2e.sh"]
fn narrow_full_status() -> Result<()> {
    let tui = TuiSession::start("status_card_storybook", 48, 30)?;
    tui.expect("Status card specimen")?;
    tui.send("v")?;
    tui.expect("Content: Full")?;
    assert_screen!(tui, "full_48x30");
    Ok(())
}
