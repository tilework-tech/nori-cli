//! `_nori/host/prompts` and `_nori/session/prompts/run`: the custom prompts
//! in the host's own home are listed to every terminal on that host, and
//! its scripts run on the host, in the session's folder.

use super::acp_v1::connect;
use super::acp_v1::controller;
use super::acp_v1::host_with;
use super::acp_v1::new_session;
use super::acp_v1::watcher;
use super::*;
use pretty_assertions::assert_eq;
use serde_json::json;

fn write_commands(host: &Host, files: &[(&str, &str)]) {
    let commands = host.0.path().join("commands");
    std::fs::create_dir_all(&commands).unwrap();
    for (name, body) in files {
        std::fs::write(commands.join(name), body).unwrap();
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn initialize_advertises_prompts() {
    let host = host_with("", &[]);
    let mut client = connect(&host);
    let result = client.request("initialize", json!({})).await.unwrap();
    assert_eq!(
        result["capabilities"]["nori_sessions"]["prompts"],
        json!(true)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_watcher_lists_the_prompts_kept_in_the_hosts_home() {
    let host = host_with("", &[]);
    write_commands(
        &host,
        &[
            (
                "harvest.md",
                "---\ndescription: Pick what is ripe\nargument-hint: BED=<name>\n---\nHarvest the $BED bed.\n",
            ),
            ("weather.sh", "echo sunny\n"),
            ("notes.txt", "not a prompt\n"),
        ],
    );
    let mut watching = watcher(&host);

    let listed = watching
        .call("_nori/host/prompts", json!({}))
        .await
        .unwrap();

    assert_eq!(
        listed,
        json!({"prompts": [
            {
                "name": "harvest",
                "description": "Pick what is ripe",
                "argumentHint": "BED=<name>",
                "kind": {"type": "markdown"},
                "content": "Harvest the $BED bed.\n",
            },
            {
                "name": "weather",
                "description": null,
                "argumentHint": null,
                "kind": {"type": "script", "interpreter": "bash"},
                "content": "",
            },
        ]})
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_script_runs_on_the_host_in_the_sessions_folder() {
    let host = host_with("", &[]);
    write_commands(&host, &[("where.sh", "echo \"$1 in $(pwd)\"\n")]);
    let folder = TempDir::new().unwrap();
    let mut acp = controller(&host);
    let (id, _) = new_session(&mut acp, folder.path()).await;

    let ran = acp
        .call(
            "_nori/session/prompts/run",
            json!({"sessionId": id, "name": "where", "args": ["seedlings"]}),
        )
        .await
        .unwrap();

    assert_eq!(
        ran,
        json!({"stdout": format!("seedlings in {}\n", canonical(folder.path()))})
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failing_script_reports_why() {
    let host = host_with("", &[]);
    write_commands(&host, &[("broken.sh", "echo no water >&2\nexit 3\n")]);
    let folder = TempDir::new().unwrap();
    let mut acp = controller(&host);
    let (id, _) = new_session(&mut acp, folder.path()).await;

    let failed = acp
        .call(
            "_nori/session/prompts/run",
            json!({"sessionId": id, "name": "broken", "args": []}),
        )
        .await
        .unwrap_err();

    assert!(
        failed.message.contains("exit code 3") && failed.message.contains("no water"),
        "{failed:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_watcher_cannot_run_a_script() {
    let host = host_with("", &[]);
    let marker = host.0.path().join("ran");
    write_commands(
        &host,
        &[("touch.sh", &format!("touch {}\n", marker.display()))],
    );
    let folder = TempDir::new().unwrap();
    let mut acp = controller(&host);
    let (id, _) = new_session(&mut acp, folder.path()).await;
    let mut watching = watcher(&host);

    let refused = watching
        .call(
            "_nori/session/prompts/run",
            json!({"sessionId": id, "name": "touch", "args": []}),
        )
        .await
        .unwrap_err();

    assert_eq!(refused.code, -32016);
    assert!(!marker.exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn only_a_script_in_the_hosts_commands_can_be_run() {
    let host = host_with("", &[]);
    write_commands(&host, &[("harvest.md", "Harvest.\n")]);
    let folder = TempDir::new().unwrap();
    let mut acp = controller(&host);
    let (id, _) = new_session(&mut acp, folder.path()).await;

    for name in ["missing", "harvest", "../commands/harvest"] {
        let refused = acp
            .call(
                "_nori/session/prompts/run",
                json!({"sessionId": id, "name": name, "args": []}),
            )
            .await
            .unwrap_err();
        assert_eq!(refused.code, -32602, "{name}: {refused:?}");
    }
}
