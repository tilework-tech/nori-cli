#[path = "../support/e2e.rs"]
mod e2e;
use anyhow::Result;
use e2e::TuiSession;
use e2e::assert_screen;

#[test]
#[ignore = "requires built storybooks and TUI_PUPPETEERING_DIR; run scripts/storybook-e2e.sh"]
fn detail_pane() -> Result<()> {
    let tui = TuiSession::start("nori_storybook", 120, 40)?;
    tui.expect("Nori component storybook")?;
    tui.send("5")?;
    tui.expect("Default · compact columns + heading")?;
    assert_screen!(tui, "details_120x40");
    Ok(())
}

#[test]
#[ignore = "requires built storybooks and TUI_PUPPETEERING_DIR; run scripts/storybook-e2e.sh"]
fn narrow_detail_pane() -> Result<()> {
    let tui = TuiSession::start("nori_storybook", 48, 30)?;
    tui.expect("Nori component storybook")?;
    tui.send("5")?;
    tui.expect("Default · compact columns + heading")?;
    tui.key("Tab")?;
    tui.key("Tab")?;
    tui.key("Tab")?;
    tui.expect("Responsive · stack below 120 columns")?;
    assert_screen!(tui, "details_stacked_48x30");
    Ok(())
}

#[test]
#[ignore = "requires built storybooks and TUI_PUPPETEERING_DIR; run scripts/storybook-e2e.sh"]
fn overlay_menu() -> Result<()> {
    let tui = TuiSession::start("nori_storybook", 120, 40)?;
    tui.expect("Nori component storybook")?;
    tui.send("6")?;
    tui.expect("Choose how to continue")?;
    assert_screen!(tui, "overlay_120x40");
    tui.key("Tab")?;
    tui.expect("Shortcuts activate immediately")?;
    assert_screen!(tui, "overlay_shortcuts_120x40");
    Ok(())
}
