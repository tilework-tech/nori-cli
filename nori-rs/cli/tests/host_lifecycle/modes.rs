//! Session configuration over standard ACP: the `agent`, `model` and `mode`
//! config options, permission modes through `session/set_mode`, and settings
//! that survive a resume.

use super::acp_client::Acp;
use super::*;
use nori_harness::remote_agent::ManagedClient;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;

fn mock_agent_binary() -> String {
    std::env::var("MOCK_ACP_AGENT_BIN").unwrap_or_else(|_| {
        std::path::Path::new(env!("CARGO_BIN_EXE_nori"))
            .with_file_name("mock_acp_agent")
            .display()
            .to_string()
    })
}

/// A host whose default agent is the mock, with a second mock agent and an
/// agent whose adapter is not installed.
fn configured_host(env: &[(&str, &str)]) -> Host {
    let host = Host::new();
    std::fs::write(
        host.0.path().join("config.toml"),
        format!(
            r#"agent = "mock-model"
approval_policy = "on-request"

[[agents]]
name = "Second Mock"
slug = "second-mock"

[agents.distribution.local]
command = "{}"

[[agents]]
name = "Missing Agent"
slug = "missing-agent"

[agents.distribution.local]
command = "nori-test-agent-that-is-not-installed"
"#,
            mock_agent_binary()
        ),
    )
    .unwrap();
    let mut command = host.command(&["host", "--port", "0"]);
    command.envs(env.iter().copied());
    success(bounded_output(&mut command));
    host
}

/// Start a session through the CLI and attach a controlling ACP client to
/// it with `session/resume`, returning the client, the ID and the result.
async fn attached(host: &Host, folder: &std::path::Path) -> (Acp, String, Value) {
    let id = success(host.run(&["host", "new", "--cwd", folder.to_str().unwrap()]))
        .trim()
        .to_owned();
    let mut acp = Acp::new(tokio::task::block_in_place(|| host.client()));
    let result = resume(&mut acp, &id, folder).await;
    (acp, id, result)
}

async fn resume(acp: &mut Acp, id: &str, folder: &std::path::Path) -> Value {
    acp.call(
        "session/resume",
        json!({"sessionId": id, "cwd": canonical(folder)}),
    )
    .await
    .unwrap()
}

fn option<'a>(options: &'a Value, id: &str) -> &'a Value {
    options
        .as_array()
        .unwrap()
        .iter()
        .find(|option| option["id"] == id)
        .unwrap_or_else(|| panic!("no {id} option in {options}"))
}

fn values(option: &Value) -> Vec<String> {
    option["options"]
        .as_array()
        .unwrap()
        .iter()
        .map(|choice| choice["value"].as_str().unwrap().to_owned())
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn an_attached_session_reports_its_agent_model_and_permission_mode() {
    let host = configured_host(&[]);
    let folder = TempDir::new().unwrap();

    let (_acp, _id, result) = attached(&host, folder.path()).await;

    let options = &result["configOptions"];
    let agent = option(options, "agent");
    assert_eq!(agent["category"], "_nori_agent");
    assert_eq!(agent["currentValue"], "mock-model");
    let agents = values(agent);
    assert!(
        ["mock-model", "second-mock", "missing-agent"]
            .iter()
            .all(|slug| agents.contains(&slug.to_string())),
        "{agent}"
    );
    let missing = agent["options"]
        .as_array()
        .unwrap()
        .iter()
        .find(|choice| choice["value"] == "missing-agent")
        .unwrap();
    assert!(
        missing["description"]
            .as_str()
            .is_some_and(|description| description.contains("not installed")),
        "{missing}"
    );
    let model = option(options, "model");
    assert_eq!(model["category"], "model");
    assert_eq!(
        values(model),
        vec![
            "mock-model-default".to_owned(),
            "mock-model-fast".to_owned()
        ]
    );
    let mode = option(options, "mode");
    assert_eq!(mode["category"], "mode");
    assert_eq!(mode["currentValue"], "agent");
    assert_eq!(values(mode), vec!["ask", "agent", "approve-all"]);
    assert_eq!(
        result["modes"],
        json!({
            "currentModeId": "agent",
            "availableModes": [
                {"id": "ask", "name": "Ask", "description": "Ask before every edit and command."},
                {"id": "agent", "name": "Agent", "description": "The agent asks when it judges it needs to."},
                {"id": "approve-all", "name": "Approve all", "description": "Nori approves every permission request."},
            ],
        })
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn changing_the_model_reaches_every_attached_client() {
    let host = configured_host(&[]);
    let folder = TempDir::new().unwrap();
    let (mut controller, id, _) = attached(&host, folder.path()).await;
    let mut watcher = Acp::new(tokio::task::block_in_place(|| host.observer()));
    resume(&mut watcher, &id, folder.path()).await;

    let changed = controller
        .call(
            "session/set_config_option",
            json!({"sessionId": id, "configId": "model", "value": "mock-model-fast"}),
        )
        .await
        .unwrap();

    assert_eq!(
        option(&changed["configOptions"], "model")["currentValue"],
        "mock-model-fast"
    );
    assert_eq!(
        option(&changed["configOptions"], "mode")["currentValue"],
        "agent"
    );
    let update = watcher
        .until(|frame| {
            frame["method"] == "session/update"
                && frame["params"]["update"]["sessionUpdate"] == "config_option_update"
        })
        .await;
    let options = &update["params"]["update"]["configOptions"];
    assert_eq!(option(options, "model")["currentValue"], "mock-model-fast");
    assert_eq!(option(options, "agent")["currentValue"], "mock-model");
}

/// Collect envelopes from a legacy all-session subscription until a
/// permission request or the mock's granted reply appears; true when asked.
async fn asked_for_permission(observer: &mut ManagedClient) -> bool {
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let frame = observer.recv().await.unwrap();
            if frame["method"] != "_nori/session/event" {
                continue;
            }
            let event = &frame["params"]["event"];
            if event["source"] == "acp"
                && event["event"]["message_type"] == "request"
                && event["event"]["request"]["toolCall"].is_object()
            {
                return true;
            }
            if event["event"]["update"]["content"]["text"]
                .as_str()
                .is_some_and(|text| text.starts_with("Permission granted"))
            {
                return false;
            }
        }
    })
    .await
    .unwrap_or_else(|_| panic!("the turn neither asked nor proceeded"))
}

#[tokio::test(flavor = "multi_thread")]
async fn approve_all_answers_permission_requests_until_the_mode_changes_back() {
    let host = configured_host(&[("MOCK_AGENT_REQUEST_PERMISSION", "1")]);
    let folder = TempDir::new().unwrap();
    let (mut acp, id, _) = attached(&host, folder.path()).await;
    let mut observer = tokio::task::block_in_place(|| host.observer());
    observer
        .request("_nori/session/subscribe", json!({"session_id": "*"}))
        .await
        .unwrap();

    acp.call(
        "session/set_mode",
        json!({"sessionId": id, "modeId": "approve-all"}),
    )
    .await
    .unwrap();
    acp.until(|frame| {
        frame["params"]["update"]["sessionUpdate"] == "current_mode_update"
            && frame["params"]["update"]["currentModeId"] == "approve-all"
    })
    .await;
    acp.prompt_alias(&id, "run the command").await;
    assert!(
        !asked_for_permission(&mut observer).await,
        "asked under approve-all"
    );

    acp.call(
        "session/set_config_option",
        json!({"sessionId": id, "configId": "mode", "value": "agent"}),
    )
    .await
    .unwrap();
    acp.prompt_alias(&id, "run it again").await;
    assert!(asked_for_permission(&mut observer).await, "agent mode asks");
}

#[tokio::test(flavor = "multi_thread")]
async fn watchers_and_unknown_modes_cannot_change_a_session() {
    let host = configured_host(&[]);
    let folder = TempDir::new().unwrap();
    let (mut controller, id, _) = attached(&host, folder.path()).await;
    let mut watcher = Acp::new(tokio::task::block_in_place(|| host.observer()));

    let from_watcher = watcher
        .call(
            "session/set_mode",
            json!({"sessionId": id, "modeId": "approve-all"}),
        )
        .await
        .unwrap_err();
    let unknown = controller
        .call(
            "session/set_mode",
            json!({"sessionId": id, "modeId": "yolo"}),
        )
        .await
        .unwrap_err();
    let unknown_option = controller
        .call(
            "session/set_config_option",
            json!({"sessionId": id, "configId": "mode", "value": "yolo"}),
        )
        .await
        .unwrap_err();

    assert_eq!(from_watcher.code, -32016, "{from_watcher:?}");
    assert_eq!(unknown.code, -32602, "{unknown:?}");
    assert_eq!(unknown_option.code, -32602, "{unknown_option:?}");
    let result = resume(&mut controller, &id, folder.path()).await;
    assert_eq!(result["modes"]["currentModeId"], "agent");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_agent_can_change_until_the_first_prompt() {
    let host = configured_host(&[("MOCK_AGENT_WRITE_FILE", "marker.txt")]);
    let folder = TempDir::new().unwrap();
    let (mut acp, id, _) = attached(&host, folder.path()).await;

    let switched = acp
        .call(
            "session/set_config_option",
            json!({"sessionId": id, "configId": "agent", "value": "second-mock"}),
        )
        .await
        .unwrap();
    assert_eq!(
        option(&switched["configOptions"], "agent")["currentValue"],
        "second-mock"
    );
    let listing = acp.call("_nori/session/list", json!({})).await.unwrap();
    let row = listing["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["session_id"] == id.as_str())
        .unwrap();
    assert_eq!(row["agent"], "second-mock", "the host runs the new agent");

    acp.prompt_alias(&id, "write a marker").await;
    tokio::task::block_in_place(|| wait_for_file(&folder.path().join("marker.txt")));
    let refused = acp
        .call(
            "session/set_config_option",
            json!({"sessionId": id, "configId": "agent", "value": "mock-model"}),
        )
        .await
        .unwrap_err();
    assert_eq!(refused.code, -32602, "{refused:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_changed_mode_and_model_survive_a_host_restart() {
    let host = configured_host(&[]);
    let folder = TempDir::new().unwrap();
    let (mut acp, id, _) = attached(&host, folder.path()).await;
    acp.call(
        "session/set_config_option",
        json!({"sessionId": id, "configId": "model", "value": "mock-model-fast"}),
    )
    .await
    .unwrap();
    acp.call(
        "session/set_mode",
        json!({"sessionId": id, "modeId": "approve-all"}),
    )
    .await
    .unwrap();
    acp.prompt_alias(&id, "remember this").await;
    acp.until(super::acp_client::is_update_mentioning(
        &id,
        "remember this",
    ))
    .await;
    drop(acp);
    success(host.run(&["host", "stop"]));
    success(host.run(&["host", "--port", "0"]));

    let mut acp = Acp::new(tokio::task::block_in_place(|| host.client()));
    let result = resume(&mut acp, &id, folder.path()).await;

    assert_eq!(
        option(&result["configOptions"], "model")["currentValue"],
        "mock-model-fast"
    );
    assert_eq!(result["modes"]["currentModeId"], "approve-all");
    assert_eq!(
        option(&result["configOptions"], "mode")["currentValue"],
        "approve-all"
    );
}
