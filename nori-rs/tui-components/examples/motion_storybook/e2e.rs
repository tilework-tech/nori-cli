#[path = "../support/e2e.rs"]
mod e2e;
#[allow(dead_code)]
mod story;

use std::time::Duration;

use anyhow::Result;
use e2e::TuiSession;
use e2e::assert_screen;
use pretty_assertions::assert_eq;
use ratatui::Terminal;
use ratatui::backend::TestBackend;

#[test]
fn bare_zoom_snapshots() {
    for (width, height) in [(100, 34), (36, 20)] {
        for muster in [false, true] {
            for (name, time) in [("dots", 0), ("zoom", 2400), ("braille", 3000)] {
                let mut story = story::Story::new(muster);
                story.advance(Duration::from_millis(time));
                let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
                terminal
                    .draw(|frame| frame.render_widget(story.widget(), frame.area()))
                    .unwrap();
                let variant = if muster { "muster" } else { "platoon" };
                insta::assert_snapshot!(
                    format!("{variant}_{name}_{width}x{height}"),
                    terminal.backend().to_string()
                );
            }
        }
    }
}

#[test]
fn loop_preserves_overshoot_and_reverses_at_both_endpoints() {
    let mut story = story::Story::new(false);
    story.advance(Duration::from_secs(3));
    assert_eq!((story.phase(), story.zooming_in), (1.0, false));
    story.advance(Duration::from_secs(3));
    assert_eq!((story.phase(), story.zooming_in), (0.0, true));
    story.advance(Duration::from_secs(7));
    assert_eq!((story.phase(), story.zooming_in), (1.0 / 3.0, true));
}

#[test]
#[ignore = "requires built storybooks and TUI_PUPPETEERING_DIR; run scripts/storybook-e2e.sh"]
fn bare_zoom_terminal() -> Result<()> {
    let tui = TuiSession::start("motion_storybook", 100, 34)?;
    tui.expect("·")?;
    tui.key("s")?;
    tui.expect("⠂")?;
    assert_screen!(tui, "platoon_zoom_100x34");
    Ok(())
}
