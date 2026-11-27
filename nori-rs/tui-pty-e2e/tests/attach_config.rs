//! `/model` and `/config` in `nori attach` show the session's own ACP
//! configuration and change it on the host. The agent's next answer says
//! which model and effort it ran with, so a change counts only once the
//! agent behaves differently.
#![cfg(target_os = "linux")]
#[path = "support/agent_host.rs"]
mod agent_host;
use agent_host::*;
use tui_pty_e2e::Key;
use tui_pty_e2e::TIMEOUT;
use tui_pty_e2e::TuiSession;

const GATES: &[&str] = &["fixture-config-options-only", "fixture-settings-report"];

fn new_session(host: &AgentHost, tui: &mut TuiSession, count: usize) {
    ask(tui, "/new");
    let deadline = std::time::Instant::now() + TIMEOUT;
    while host
        .trace("amber")
        .iter()
        .filter(|line| line.as_str() == "session/new -")
        .count()
        < count
    {
        assert!(std::time::Instant::now() < deadline, "no session/new");
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    tui.wait_for_text("amber · ready", TIMEOUT).unwrap();
}

/// Close the config panel that reopens after a confirmed change.
fn close_panel(tui: &mut TuiSession) {
    tui.wait_for_text("Session config", TIMEOUT).unwrap();
    tui.press(
        Key::Escape,
        |screen| !screen.contains("Session config"),
        "the config panel closes",
    )
    .unwrap();
}

/// Pick `value` for the option the open value picker titled `title` edits.
fn pick(tui: &mut TuiSession, title: &str, value: &str) {
    wait_for_picker(tui, title);
    assert!(focus(tui, title, value), "{title} offers {value}");
    tui.send_key(Key::Enter).unwrap();
}

/// Wait for the picker whose title line is exactly `title`.
fn wait_for_picker(tui: &mut TuiSession, title: &str) {
    tui.wait_for(
        |screen| screen.lines().any(|line| line.trim() == title),
        TIMEOUT,
    )
    .unwrap_or_else(|error| panic!("no {title} picker: {error}"));
}

fn picker_rows(tui: &TuiSession, title: &str) -> Vec<String> {
    tui.screen_contents()
        .lines()
        .skip_while(|line| line.trim() != title)
        .filter_map(|line| {
            let line = line.trim_matches(|c: char| c.is_whitespace() || c == '▏' || c == '▕');
            let line = line.trim_start_matches('›').trim_start();
            let (number, rest) = line.split_once(". ")?;
            number.parse::<i32>().ok()?;
            Some(rest.trim().to_owned())
        })
        .collect()
}

#[test]
fn a_model_the_host_confirms_answers_the_next_prompt() {
    let host = AgentHost::new(GATES);
    host.start();
    let mut attached = host.attach();
    let tui = &mut attached.terminal;
    close_list(tui);
    new_session(&host, tui, 1);
    ask(tui, "whoami");
    tui.wait_for_text(
        "amber is answering (1) with model initial, effort low.",
        TIMEOUT,
    )
    .unwrap();

    ask(tui, "/model");
    pick(tui, "Model", "Amber");
    tui.wait_for_text("option set: Model=Amber", TIMEOUT)
        .unwrap();
    close_panel(tui);
    ask(tui, "whoami");
    tui.wait_for_text(
        "amber is answering (2) with model amber, effort low.",
        TIMEOUT,
    )
    .unwrap();
}

#[test]
fn config_lists_the_agents_own_options_and_changes_one() {
    let host = AgentHost::new(GATES);
    host.start();
    let mut attached = host.attach();
    let tui = &mut attached.terminal;
    close_list(tui);
    new_session(&host, tui, 1);

    ask(tui, "/config");
    wait_for_picker(tui, "Session config");
    let rows = picker_rows(tui, "Session config");
    assert_eq!(
        rows,
        ["Model (Initial)", "Effort (Low)"],
        "the agent's own options; the agent and permission mode have their own commands"
    );
    assert!(focus(tui, "Session config", "Effort"));
    tui.send_key(Key::Enter).unwrap();
    pick(tui, "Effort", "High");
    tui.wait_for_text("option set: Effort=High", TIMEOUT)
        .unwrap();
    tui.wait_for_text("Effort (High)", TIMEOUT)
        .expect("the config panel reopens with the confirmed value");
    close_panel(tui);
    ask(tui, "whoami");
    tui.wait_for_text(
        "amber is answering (1) with model initial, effort high.",
        TIMEOUT,
    )
    .unwrap();
}

#[test]
fn each_session_keeps_its_own_configuration_across_switches() {
    let host = AgentHost::new(GATES);
    host.start();
    let mut attached = host.attach();
    let tui = &mut attached.terminal;
    close_list(tui);
    new_session(&host, tui, 1);
    ask(tui, "/model");
    pick(tui, "Model", "Amber");
    tui.wait_for_text("option set: Model=Amber", TIMEOUT)
        .unwrap();
    close_panel(tui);

    new_session(&host, tui, 2);
    tui.wait_for(
        |screen| !screen.contains("option set: Model=Amber"),
        TIMEOUT,
    )
    .expect("the second session is on screen");
    tui.settle(PAUSE);
    ask(tui, "/config");
    wait_for_picker(tui, "Session config");
    assert_eq!(
        picker_rows(tui, "Session config"),
        ["Model (Initial)", "Effort (Low)"],
        "the second session shows its own model"
    );
    close_panel(tui);
    ask(tui, "whoami");
    tui.wait_for_text("with model initial, effort low.", TIMEOUT)
        .unwrap();

    tui.send_key(Key::Alt('s')).unwrap();
    tui.wait_for_text("Sessions", TIMEOUT).unwrap();
    assert!(
        focus(tui, "Sessions", "new session"),
        "the list offers the first session"
    );
    tui.send_key(Key::Enter).unwrap();
    tui.wait_for_text("option set: Model=Amber", TIMEOUT)
        .expect("the first session is back on screen");
    ask(tui, "/config");
    wait_for_picker(tui, "Session config");
    assert_eq!(
        picker_rows(tui, "Session config"),
        ["Model (Amber)", "Effort (Low)"],
        "the first session still shows the model it confirmed"
    );
}

#[test]
fn a_watcher_cannot_change_the_configuration() {
    let host = AgentHost::new(GATES);
    host.start();
    let mut controller = host.attach();
    close_list(&mut controller.terminal);
    new_session(&host, &mut controller.terminal, 1);
    let mut watcher = host.attach();
    let tui = &mut watcher.terminal;
    tui.wait_for_text("amber", TIMEOUT).unwrap();
    tui.send_key(Key::Enter).unwrap();
    tui.wait_for_text("watching", TIMEOUT).unwrap();

    for command in ["/model", "/config"] {
        ask(tui, command);
        tui.wait_for_text(
            &format!("{command}: watching · Alt+T to take control"),
            TIMEOUT,
        )
        .unwrap();
        assert!(
            !tui.screen_contents().contains("Session config"),
            "{command} opens nothing for a watcher"
        );
        // A refused command stays in the composer.
        for _ in 0..command.len() {
            tui.send_key(Key::Backspace).unwrap();
            tui.settle(PAUSE / 4);
        }
    }
    assert!(
        !host
            .trace("amber")
            .iter()
            .any(|line| line.starts_with("session/set_config_option")),
        "nothing changed on the agent"
    );
}

#[test]
fn attach_offers_the_models_plain_nori_offers_for_the_same_agent() {
    let host = AgentHost::new(GATES);
    host.start();
    let mut attached = host.attach();
    let tui = &mut attached.terminal;
    close_list(tui);
    new_session(&host, tui, 1);
    ask(tui, "/model");
    wait_for_picker(tui, "Model");
    let joined = picker_rows(tui, "Model");

    let mut plain = host.plain();
    ask(&mut plain, "/model");
    wait_for_picker(&mut plain, "Model");
    let owned = picker_rows(&plain, "Model");
    plain.terminate_and_wait(std::time::Duration::from_secs(2));

    assert_eq!(joined, ["Initial (current)", "Amber"]);
    assert_eq!(
        owned
            .iter()
            .filter(|row| !row.starts_with("Use custom model"))
            .collect::<Vec<_>>(),
        joined.iter().collect::<Vec<_>>(),
        "plain nori also offers a custom model, which needs a restart on this machine"
    );
}

#[test]
fn a_terminal_that_attaches_later_sees_the_confirmed_model() {
    let host = AgentHost::new(GATES);
    host.start();
    let mut attached = host.attach();
    let tui = &mut attached.terminal;
    close_list(tui);
    new_session(&host, tui, 1);
    ask(tui, "/model");
    pick(tui, "Model", "Amber");
    tui.wait_for_text("option set: Model=Amber", TIMEOUT)
        .unwrap();
    close_panel(tui);
    drop(attached);

    let mut later = host.attach();
    let tui = &mut later.terminal;
    assert!(focus(tui, "Sessions", "new session"));
    tui.send_key(Key::Enter).unwrap();
    tui.wait_for_text("amber · ready", TIMEOUT).unwrap();
    ask(tui, "/model");
    wait_for_picker(tui, "Model");
    assert_eq!(picker_rows(tui, "Model"), ["Initial", "Amber (current)"]);
}

/// Change the model of the session on screen to Amber, then change the
/// session's agent in place to Birch, which advertises no options.
fn replace_after_a_model_change(host: &AgentHost, tui: &mut TuiSession) {
    close_list(tui);
    new_session(host, tui, 1);
    ask(tui, "/model");
    pick(tui, "Model", "Amber");
    tui.wait_for_text("option set: Model=Amber", TIMEOUT)
        .unwrap();
    close_panel(tui);
    ask(tui, "/agent");
    tui.wait_for_text("Choose an agent", TIMEOUT).unwrap();
    assert!(focus(tui, "Choose an agent", "Birch"));
    tui.send_key(Key::Enter).unwrap();
    tui.wait_for_text("This session now runs Birch.", TIMEOUT)
        .unwrap();
}

const NO_MODEL: &str = "This agent has no model selector";

#[test]
fn an_agent_replaced_by_one_without_options_leaves_no_model_behind() {
    let gates = [GATES, &["fixture-no-options-birch"]].concat();
    let host = AgentHost::new(&gates);
    host.start();
    let mut first = host.attach();
    replace_after_a_model_change(&host, &mut first.terminal);
    tui_shows_no_model(&mut first.terminal);
    drop(first);

    let mut later = host.attach();
    let tui = &mut later.terminal;
    assert!(focus(tui, "Sessions", "new session"));
    tui.send_key(Key::Enter).unwrap();
    tui.wait_for_text("birch · ready", TIMEOUT).unwrap();
    tui_shows_no_model(tui);
}

fn tui_shows_no_model(tui: &mut TuiSession) {
    ask(tui, "/model");
    tui.wait_for_text(NO_MODEL, TIMEOUT)
        .expect("no model is left from the previous agent");
    tui.press(
        Key::Escape,
        |screen| !screen.contains(NO_MODEL),
        "the model message closes",
    )
    .unwrap();
}

#[test]
fn a_watcher_adopts_the_controllers_confirmed_model() {
    let host = AgentHost::new(GATES);
    host.start();
    let mut controller = host.attach();
    let tui = &mut controller.terminal;
    close_list(tui);
    new_session(&host, tui, 1);
    let mut watcher = host.attach();
    assert!(focus(&mut watcher.terminal, "Sessions", "new session"));
    watcher.terminal.send_key(Key::Enter).unwrap();
    watcher.terminal.wait_for_text("watching", TIMEOUT).unwrap();

    ask(tui, "/model");
    pick(tui, "Model", "Amber");
    tui.wait_for_text("option set: Model=Amber", TIMEOUT)
        .unwrap();

    close_panel(tui);
    let watcher = &mut watcher.terminal;
    watcher.settle(PAUSE);
    watcher.send_key(Key::Alt('t')).unwrap();
    watcher.wait_for_text("controlling", TIMEOUT).unwrap();
    watcher.settle(PAUSE);
    ask(watcher, "/model");
    wait_for_picker(watcher, "Model");
    assert_eq!(
        picker_rows(watcher, "Model"),
        ["Initial", "Amber (current)"],
        "the former watcher shows the model it saw confirmed"
    );
}
