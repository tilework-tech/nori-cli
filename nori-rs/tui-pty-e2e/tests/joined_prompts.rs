//! Custom prompts come from the host: a terminal attached with homes of its
//! own lists the prompts in the host's `NORI_HOME/commands`, expands a
//! markdown prompt, and runs a script prompt on the host.
#![cfg(target_os = "linux")]

#[path = "support/joined_host.rs"]
mod joined_host;

use std::path::Path;
use std::time::Duration;
use std::time::Instant;

use joined_host::JoinedHost;
use tui_pty_e2e::TIMEOUT;

fn delivered(log: &Path) -> Vec<String> {
    std::fs::read_to_string(log)
        .unwrap_or_default()
        .lines()
        .map(|line| serde_json::from_str::<String>(line).expect("prompt log line"))
        .collect()
}

fn wait_delivered(log: &Path, wanted: impl Fn(&[String]) -> bool) -> Vec<String> {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let prompts = delivered(log);
        if wanted(&prompts) || Instant::now() > deadline {
            return prompts;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
fn an_attached_terminal_lists_expands_and_runs_the_hosts_own_prompts() {
    let logs = tempfile::tempdir().unwrap();
    let log = logs.path().join("prompts.jsonl");
    let host = JoinedHost::new(&[("MOCK_AGENT_PROMPT_LOG", log.to_str().unwrap())]);
    let commands = host.nori_home.path().join("commands");
    std::fs::create_dir(&commands).unwrap();
    std::fs::write(
        commands.join("plan-orchard.md"),
        "---\ndescription: Plan one orchard row\n---\nPlan the $1 row of the demo orchard.\n",
    )
    .unwrap();
    std::fs::write(
        commands.join("orchard-where.sh"),
        "#!/bin/bash\necho \"ran in $(pwd)\"\ncat \"$HOME/orchard-marker.txt\"\n",
    )
    .unwrap();
    std::fs::write(
        host.home.path().join("orchard-marker.txt"),
        "marker from the host home\n",
    )
    .unwrap();
    host.start();

    let mut attached = host.attach_new();
    let terminal = &mut attached.terminal;
    terminal.type_input("/prompts:").unwrap();
    terminal
        .wait_for(
            |screen| {
                screen.contains("prompts:plan-orchard")
                    && screen.contains("Plan one orchard row")
                    && screen.contains("prompts:orchard-where")
            },
            TIMEOUT,
        )
        .expect("the popup lists the prompts in the host's home");

    terminal.type_input("plan-orchard cedar").unwrap();
    terminal.send_str("\r").unwrap();
    let prompts = wait_delivered(&log, |prompts| {
        prompts
            .iter()
            .any(|prompt| prompt.contains("Plan the cedar row of the demo orchard."))
    });
    assert!(
        prompts
            .iter()
            .any(|prompt| prompt.contains("Plan the cedar row of the demo orchard.")),
        "the markdown prompt is expanded and sent: {prompts:?}\n{}",
        terminal.screen_contents()
    );

    terminal
        .wait_for_text(joined_host::ANSWER, TIMEOUT)
        .unwrap();
    terminal.submit_input("/prompts:orchard-where").unwrap();
    let project = host.project().canonicalize().unwrap();
    let ran_on_host = |prompt: &String| {
        prompt.contains(&format!("ran in {}", project.display()))
            && prompt.contains("marker from the host home")
    };
    let prompts = wait_delivered(&log, |prompts| prompts.iter().any(ran_on_host));
    assert!(
        prompts.iter().any(ran_on_host),
        "the script ran on the host in the session folder: {prompts:?}\n{}",
        terminal.screen_contents()
    );
}
