//! `@` mentions in plain `nori` list files from the folder of the session on
//! screen, and `/new DIR` starts the next session in that folder, which Tab
//! completes from the same folder.
#![cfg(unix)]
#[path = "support/two_agents.rs"]
mod two_agents;
use tui_pty_e2e::Key;
use tui_pty_e2e::TIMEOUT;
use tui_pty_e2e::TuiSession;
use two_agents::*;

/// Type an `@` query, wait for `shown` in the popup and check `hidden` is
/// not offered, then clear the composer.
fn mention(tui: &mut TuiSession, query: &str, shown: &str, hidden: &str) {
    tui.send_str(&format!("@{query}")).unwrap();
    tui.wait_for_text(shown, TIMEOUT)
        .unwrap_or_else(|error| panic!("@{query} offers {shown}: {error}"));
    tui.settle(PAUSE);
    let screen = tui.screen_contents();
    assert!(
        !screen.contains(hidden),
        "{hidden} is not offered:\n{screen}"
    );
    tui.send_key(Key::Ctrl('c')).unwrap();
    tui.wait_for(|screen| !screen.contains(&format!("@{query}")), TIMEOUT)
        .unwrap();
}

#[test]
fn mentions_follow_the_session_on_screen_and_new_starts_in_the_named_folder() {
    let (home, mut tui) = terminal(&[]);
    std::fs::write(home.path().join("amber-notes.md"), "first").unwrap();
    let other = tempfile::tempdir().unwrap();
    let other_folder = other.path().canonicalize().unwrap();
    std::fs::write(other_folder.join("birch-notes.md"), "second").unwrap();
    // The fixture traces into its own working folder, so this trace only
    // fills if the new session's agent runs there.
    std::fs::write(other_folder.join("fixture-traffic-amber"), "").unwrap();

    mention(&mut tui, "notes", "amber-notes.md", "birch-notes.md");

    ask(&mut tui, &format!("/new {}", other_folder.display()));
    ask(&mut tui, "whoami");
    let deadline = std::time::Instant::now() + TIMEOUT;
    while !std::fs::read_to_string(other_folder.join("fixture-traffic-amber"))
        .unwrap()
        .contains("session/prompt")
    {
        assert!(
            std::time::Instant::now() < deadline,
            "no session answered in {other_folder:?}"
        );
        tui.settle(std::time::Duration::from_millis(50));
    }
    mention(&mut tui, "notes", "birch-notes.md", "amber-notes.md");

    tui.send_key(Key::Alt(',')).unwrap();
    tui.settle(PAUSE);
    mention(&mut tui, "notes", "amber-notes.md", "birch-notes.md");
    tui.send_key(Key::Alt('.')).unwrap();
    tui.settle(PAUSE);
    mention(&mut tui, "notes", "birch-notes.md", "amber-notes.md");
}

#[test]
fn new_with_a_missing_folder_is_refused_and_keeps_the_text() {
    let (home, mut tui) = terminal(&[]);
    ask(&mut tui, "/new no-such-folder");
    let missing = home.path().canonicalize().unwrap().join("no-such-folder");
    tui.wait_for_text(&format!("No folder at {}", missing.display()), TIMEOUT)
        .unwrap();
    tui.wait_for(
        |screen| {
            // The composer shows a slash command with its own `/` marker.
            screen
                .lines()
                .any(|line| line.trim() == "/ new no-such-folder")
        },
        TIMEOUT,
    )
    .unwrap();
    assert_eq!(pids(home.path(), "amber").len(), 1, "no session started");
}

/// Type `typed`, press Tab, and wait for the composer to show `completed`,
/// then clear the composer.
fn complete(tui: &mut TuiSession, typed: &str, completed: &str) {
    tui.send_str(typed).unwrap();
    tui.settle(PAUSE);
    tui.send_key(Key::Tab).unwrap();
    tui.wait_for(
        |screen| {
            screen
                .lines()
                .any(|line| line.trim() == format!("/ {completed}"))
        },
        TIMEOUT,
    )
    .unwrap_or_else(|error| panic!("{typed} completes to {completed}: {error}"));
    tui.send_key(Key::Ctrl('c')).unwrap();
    tui.wait_for(|screen| !screen.contains(completed), TIMEOUT)
        .unwrap();
}

#[test]
fn tab_completes_new_folders_from_the_session_on_screen() {
    let (home, mut tui) = terminal(&[]);
    let orchard = home.path().canonicalize().unwrap().join("orchard");
    std::fs::create_dir_all(orchard.join("pears")).unwrap();
    std::fs::write(orchard.join("fixture-traffic-amber"), "").unwrap();

    complete(&mut tui, "/new orc", "new orchard/");

    ask(&mut tui, "/new orchard");
    ask(&mut tui, "whoami");
    let deadline = std::time::Instant::now() + TIMEOUT;
    while !std::fs::read_to_string(orchard.join("fixture-traffic-amber"))
        .unwrap()
        .contains("session/prompt")
    {
        assert!(
            std::time::Instant::now() < deadline,
            "no session answered in {orchard:?}"
        );
        tui.settle(std::time::Duration::from_millis(50));
    }
    complete(&mut tui, "/new pe", "new pears/");

    tui.send_key(Key::Alt(',')).unwrap();
    tui.settle(PAUSE);
    complete(&mut tui, "/new orc", "new orchard/");
}
