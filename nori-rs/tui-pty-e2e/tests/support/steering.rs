use tui_pty_e2e::Key;
use tui_pty_e2e::TIMEOUT;
use tui_pty_e2e::TuiSession;

pub(super) fn exercise(session: &mut TuiSession) {
    session.submit_input("Sketch the orchard plan").unwrap();
    session
        .wait_for_text("Survey in progress", TIMEOUT)
        .unwrap();
    session.type_input("Count the cedar trees").unwrap();
    session.send_key(Key::Tab).unwrap();
    session
        .wait_for_text("Queued for the next turn", TIMEOUT)
        .unwrap();
    assert!(!session.screen_contents().contains("Cedar count complete"));
    session.submit_input("Focus on the north row").unwrap();
    session
        .wait_for_text("Survey correction received", TIMEOUT)
        .unwrap();
    session
        .wait_for_text("Cedar count complete", TIMEOUT)
        .unwrap();
    let history = session.history_contents();
    let correction = history
        .lines()
        .position(|line| {
            matches!(
                line.trim(),
                "› Focus on the north row" | "Focus on the north row"
            )
        })
        .expect("user correction in history");
    let response = history
        .lines()
        .position(|line| line.contains("Survey correction received"))
        .unwrap();
    assert!(
        correction < response,
        "the correction must precede the response it produced:\n{history}"
    );
    assert_eq!(
        history.matches("Survey correction received").count(),
        1,
        "{history}"
    );
    assert_eq!(
        history.matches("Cedar count complete").count(),
        1,
        "{history}"
    );
}
