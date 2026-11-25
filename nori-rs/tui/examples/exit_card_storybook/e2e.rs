#[path = "../../../tui-components/examples/support/e2e.rs"]
mod e2e;
use anyhow::Result;
use e2e::TuiSession;
use e2e::assert_screen;

#[test]
#[ignore = "requires built storybooks and TUI_PUPPETEERING_DIR; run scripts/storybook-e2e.sh"]
fn typical_at_80() -> Result<()> {
    let tui = TuiSession::start("exit_card_storybook", 100, 24)?;
    tui.expect("Exit card specimen")?;
    tui.expect("Width 80")?;
    assert_screen!(tui, "typical_80");
    Ok(())
}

#[test]
#[ignore = "requires built storybooks and TUI_PUPPETEERING_DIR; run scripts/storybook-e2e.sh"]
fn many_tools_at_40() -> Result<()> {
    let tui = TuiSession::start("exit_card_storybook", 60, 36)?;
    tui.expect("Exit card specimen")?;
    tui.send("t")?;
    tui.expect("14 tools")?;
    for _ in 0..3 {
        tui.key("Left")?;
    }
    tui.expect("Width 40")?;
    assert_screen!(tui, "many_tools_40");
    Ok(())
}
