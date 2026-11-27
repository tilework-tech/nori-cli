//! `/mention` in `nori attach` starts an `@` mention that the connected host
//! completes from its own disk: the session's folder, or with no session
//! the host's own folder. Files in the attaching terminal's folder never
//! appear.
#![cfg(target_os = "linux")]
#[path = "support/agent_host.rs"]
mod agent_host;
use agent_host::*;
use tui_pty_e2e::TIMEOUT;
use tui_pty_e2e::TuiSession;

const HOST_FILE: &str = "harbor-chart-on-host.md";
const CLIENT_FILE: &str = "harbor-chart-on-client.md";

fn mention(tui: &mut TuiSession) {
    ask(tui, "/mention");
    tui.wait_for(|screen| screen.contains("〉@"), TIMEOUT)
        .expect("/mention starts an @ mention");
    tui.send_str("harbor").unwrap();
    tui.wait_for_text(HOST_FILE, TIMEOUT)
        .expect("the host's file is offered");
    tui.settle(PAUSE);
    assert!(
        !tui.screen_contents().contains(CLIENT_FILE),
        "a file on the attaching machine is never offered"
    );
}

#[test]
fn mention_with_no_session_offers_the_hosts_files() {
    let host = AgentHost::new(&[]);
    std::fs::write(host.project().join(HOST_FILE), "synthetic").unwrap();
    host.start();
    let mut attached = host.attach();
    std::fs::write(attached.folder().join(CLIENT_FILE), "synthetic").unwrap();
    let tui = &mut attached.terminal;
    close_list(tui);
    mention(tui);
}

#[test]
fn mention_in_a_session_offers_its_folder_on_the_host() {
    let host = AgentHost::new(&[]);
    std::fs::write(host.project().join(HOST_FILE), "synthetic").unwrap();
    host.start();
    let mut attached = host.attach();
    std::fs::write(attached.folder().join(CLIENT_FILE), "synthetic").unwrap();
    let tui = &mut attached.terminal;
    close_list(tui);
    ask(tui, "/new");
    host.wait_for_trace("amber", "session/new -");
    tui.wait_for_text("amber · ready", TIMEOUT).unwrap();
    mention(tui);
}
