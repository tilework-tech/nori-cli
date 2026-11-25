//! `/context` in an attached terminal reports the session as the host sees
//! it: instruction files in the session's folder on the host, and the usage
//! the agent reported, which comes back after the terminal re-attaches.
#![cfg(target_os = "linux")]

#[path = "support/joined_host.rs"]
mod joined_host;

use joined_host::JoinedHost;
use tui_pty_e2e::Key;
use tui_pty_e2e::TIMEOUT;

const PANEL_LAST_ROW: &str = "Local file sizes; inclusion in the current context is not verified.";

#[test]
fn an_attached_terminal_reports_the_hosts_instructions_and_usage_after_reattaching() {
    let host = JoinedHost::new(&[]).with_real_instruction_files();
    std::fs::write(
        host.project().join("AGENTS.md"),
        "Keep the demo orchard rows labelled.\n",
    )
    .unwrap();
    host.start();

    let mut first = host.attach_new();
    let terminal = &mut first.terminal;
    terminal.submit_input("mock:usage 340412 1000000").unwrap();
    terminal
        .wait_for_text(joined_host::ANSWER, TIMEOUT)
        .unwrap();
    terminal.submit_input("/context").unwrap();
    terminal
        .wait_for_text(PANEL_LAST_ROW, TIMEOUT)
        .expect("the context panel renders in attach");
    let screen = terminal.screen_contents();
    assert!(
        screen.contains("AGENTS.md") && !screen.contains("CLAUDE.md"),
        "the instruction file in the host session's folder is listed:\n{screen}"
    );
    assert!(
        screen.contains("340,412 / 1,000,000 tokens"),
        "the agent's usage is reported:\n{screen}"
    );
    terminal.send_key(Key::Ctrl('d')).unwrap();
    assert!(terminal.wait_for_process_exit(TIMEOUT), "Ctrl+D detaches");
    drop(first);

    let mut second = host.attach_first();
    let terminal = &mut second.terminal;
    terminal
        .wait_for_text("34% / 1.00M", TIMEOUT)
        .expect("the footer shows the session's usage again after re-attaching");
    terminal.submit_input("/context").unwrap();
    terminal
        .wait_for_text(PANEL_LAST_ROW, TIMEOUT)
        .expect("the context panel renders after re-attaching");
    let screen = terminal.screen_contents();
    assert!(
        screen.contains("340,412 / 1,000,000 tokens"),
        "the usage comes back from the host after re-attaching:\n{screen}"
    );
}
