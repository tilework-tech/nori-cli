//! The `ask` permission mode puts the adapter in its own strictest mode, so
//! every turn asks, while `agent` leaves the asking to the adapter. Adapters
//! with no mode strict enough are not offered `ask`.
//!
//! The mock stands in for each built-in adapter under its slug and
//! advertises that adapter's real mode ids; it asks before every tool call
//! only in the modes named by `MOCK_AGENT_ASKING_MODES`. Unless a test says
//! otherwise each mock starts in its asking mode, so a turn that does not ask
//! shows the host moved the adapter out of it.

use super::acp_client::Acp;
use super::*;
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

/// A built-in agent slug run by the mock with that adapter's mode ids, the
/// first being the one it starts in, and the ids in which it asks every time.
fn adapter(name: &str, slug: &str, modes: &str, asking: &str) -> String {
    format!(
        r#"
[[agents]]
name = "{name}"
slug = "{slug}"

[agents.distribution.local]
command = "{}"

[agents.distribution.local.env]
MOCK_AGENT_MODES = "{modes}"
MOCK_AGENT_ASKING_MODES = "{asking}"
"#,
        mock_agent_binary()
    )
}

const CLAUDE_MODES: &str = "default,auto,acceptEdits,plan";
const CODEX_MODES: &str = "read-only,agent,workspace-write,agent-full-access";

/// A running host offering Claude Code, Codex and OpenCode, starting
/// `agent` by default, with `extra` added to its top-level configuration.
fn host_with(agent: &str, extra: &str) -> Host {
    host_with_modes(
        agent,
        &format!("approval_policy = \"on-request\"\n{extra}"),
        CLAUDE_MODES,
        CODEX_MODES,
    )
}

/// A running host with `top` as its top-level configuration and Claude Code
/// and Codex advertising `claude_modes` and `codex_modes`.
fn host_with_modes(agent: &str, top: &str, claude_modes: &str, codex_modes: &str) -> Host {
    let host = Host::new();
    std::fs::write(
        host.0.path().join("config.toml"),
        format!(
            "agent = \"{agent}\"\n{top}\n{}{}{}",
            adapter("Claude Code", "claude-code", claude_modes, "default"),
            adapter("Codex", "codex", codex_modes, "read-only"),
            // OpenCode's modes change behavior; none of them asks every time.
            adapter("OpenCode", "opencode", "build,plan", ""),
        ),
    )
    .unwrap();
    success(host.run(&["host", "--port", "0"]));
    host
}

fn controller(host: &Host) -> Acp {
    Acp::new(tokio::task::block_in_place(|| host.client()))
}

/// A new session in `folder`, switched to `agent` before its first prompt
/// unless it is the host's default.
async fn new_session(acp: &mut Acp, folder: &std::path::Path, agent: &str) -> String {
    let created = acp
        .call(
            "session/new",
            json!({"cwd": canonical(folder), "mcpServers": []}),
        )
        .await
        .unwrap();
    let id = created["sessionId"].as_str().unwrap().to_owned();
    if created["configOptions"]
        .as_array()
        .unwrap()
        .iter()
        .any(|option| option["id"] == "agent" && option["currentValue"] != agent)
    {
        switch_agent(acp, &id, agent).await.unwrap();
    }
    id
}

async fn switch_agent(
    acp: &mut Acp,
    id: &str,
    agent: &str,
) -> Result<Value, super::acp_client::RpcError> {
    acp.call(
        "session/set_config_option",
        json!({"sessionId": id, "configId": "agent", "value": agent}),
    )
    .await
}

/// The session's options and modes, as a resuming client sees them.
async fn configuration(acp: &mut Acp, id: &str, folder: &std::path::Path) -> Value {
    acp.call(
        "session/resume",
        json!({"sessionId": id, "cwd": canonical(folder)}),
    )
    .await
    .unwrap()
}

async fn set_mode(
    acp: &mut Acp,
    id: &str,
    mode: &str,
) -> Result<Value, super::acp_client::RpcError> {
    acp.call("session/set_mode", json!({"sessionId": id, "modeId": mode}))
        .await
}

/// Run one turn named `text`; true when it asked for permission, which is
/// then allowed once so the turn can finish.
async fn turn_asks(acp: &mut Acp, id: &str, text: &str) -> bool {
    let is_end = |frame: &Value| frame["id"] == text && frame.get("method").is_none();
    let ended = |frame: &Value| {
        assert_eq!(
            frame["result"]["stopReason"], "end_turn",
            "the turn did not run: {frame}"
        );
    };
    acp.send(json!({"jsonrpc":"2.0","id":text,"method":"session/prompt","params":{"sessionId":id,"prompt":[{"type":"text","text":text}]}}))
        .await;
    let frame = acp
        .until(|frame| frame["method"] == "session/request_permission" || is_end(frame))
        .await;
    if is_end(&frame) {
        ended(&frame);
        return false;
    }
    acp.send(json!({"jsonrpc":"2.0","id":frame["id"],"result":{"outcome":{"outcome":"selected","optionId":"allow"}}}))
        .await;
    ended(&acp.until(is_end).await);
    true
}

fn mode_ids(modes: &Value) -> Vec<String> {
    modes["availableModes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|mode| mode["id"].as_str().unwrap().to_owned())
        .collect()
}

fn mode_values(options: &Value) -> Vec<String> {
    let mode = options
        .as_array()
        .unwrap()
        .iter()
        .find(|option| option["id"] == "mode")
        .unwrap_or_else(|| panic!("no mode option in {options}"));
    mode["options"]
        .as_array()
        .unwrap()
        .iter()
        .map(|choice| choice["value"].as_str().unwrap().to_owned())
        .collect()
}

/// Agent mode never asks with this mock; ask asks on every turn, not just
/// until the first permission is allowed; back in agent mode it stops.
async fn ask_differs_from_agent(agent: &str) {
    let host = host_with("claude-code", "");
    let folder = TempDir::new().unwrap();
    let mut acp = controller(&host);
    let id = new_session(&mut acp, folder.path(), agent).await;

    let under_agent = turn_asks(&mut acp, &id, "first").await;
    // Into ask through session/set_mode, back through the mode option.
    set_mode(&mut acp, &id, "ask").await.unwrap();
    let first_ask = turn_asks(&mut acp, &id, "second").await;
    let second_ask = turn_asks(&mut acp, &id, "third").await;
    acp.call(
        "session/set_config_option",
        json!({"sessionId": id, "configId": "mode", "value": "agent"}),
    )
    .await
    .unwrap();
    let back_to_agent = turn_asks(&mut acp, &id, "fourth").await;

    assert_eq!(
        [under_agent, first_ask, second_ask, back_to_agent],
        [false, true, true, false],
        "{agent}: agent, ask, ask, agent"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn ask_makes_claude_code_ask_every_turn_and_agent_does_not() {
    ask_differs_from_agent("claude-code").await;
}

#[tokio::test(flavor = "multi_thread")]
async fn ask_makes_codex_ask_every_turn_and_agent_does_not() {
    ask_differs_from_agent("codex").await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_host_default_of_ask_makes_new_sessions_ask() {
    let host = host_with("claude-code", "\n[host]\ndefault_mode = \"ask\"\n");
    let folder = TempDir::new().unwrap();
    let mut acp = controller(&host);

    let id = new_session(&mut acp, folder.path(), "claude-code").await;
    let configuration = configuration(&mut acp, &id, folder.path()).await;

    assert_eq!(configuration["modes"]["currentModeId"], "ask");
    assert!(
        turn_asks(&mut acp, &id, "first").await,
        "the first turn asks"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn ask_still_asks_after_a_host_restart() {
    // Codex starts again in its own default, which does not ask.
    let host = host_with_modes(
        "claude-code",
        "approval_policy = \"on-request\"",
        CLAUDE_MODES,
        "agent,read-only,workspace-write,agent-full-access",
    );
    let folder = TempDir::new().unwrap();
    let mut acp = controller(&host);
    let id = new_session(&mut acp, folder.path(), "codex").await;
    set_mode(&mut acp, &id, "ask").await.unwrap();
    assert!(turn_asks(&mut acp, &id, "before").await, "asks before");
    drop(acp);
    success(host.run(&["host", "stop"]));
    success(host.run(&["host", "--port", "0"]));

    let mut acp = controller(&host);
    let resumed = configuration(&mut acp, &id, folder.path()).await;

    assert_eq!(resumed["modes"]["currentModeId"], "ask");
    assert!(turn_asks(&mut acp, &id, "after").await, "asks after");
}

#[tokio::test(flavor = "multi_thread")]
async fn an_adapter_with_no_mode_that_asks_every_time_is_not_offered_ask() {
    let host = host_with("claude-code", "");
    let folder = TempDir::new().unwrap();
    let mut acp = controller(&host);
    let id = new_session(&mut acp, folder.path(), "opencode").await;

    let refused = set_mode(&mut acp, &id, "ask").await.unwrap_err();
    let after = configuration(&mut acp, &id, folder.path()).await;

    assert_eq!(refused.code, -32602, "{refused:?}");
    assert_eq!(after["modes"]["currentModeId"], "agent");
    assert_eq!(mode_ids(&after["modes"]), ["agent", "approve-all"]);
    assert_eq!(
        mode_values(&after["configOptions"]),
        ["agent", "approve-all"]
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_default_of_ask_is_agent_for_an_adapter_that_cannot_ask_and_stays_agent() {
    let host = host_with("opencode", "\n[host]\ndefault_mode = \"ask\"\n");
    let folder = TempDir::new().unwrap();
    let mut acp = controller(&host);

    let id = new_session(&mut acp, folder.path(), "opencode").await;
    let created = configuration(&mut acp, &id, folder.path()).await;
    drop(acp);
    success(host.run(&["host", "stop"]));
    success(host.run(&["host", "--port", "0"]));
    let mut acp = controller(&host);
    let restarted = configuration(&mut acp, &id, folder.path()).await;

    assert_eq!(created["modes"]["currentModeId"], "agent");
    assert_eq!(mode_ids(&created["modes"]), ["agent", "approve-all"]);
    assert_eq!(restarted["modes"]["currentModeId"], "agent");
}

#[tokio::test(flavor = "multi_thread")]
async fn an_adapter_version_without_its_strict_mode_is_not_offered_ask() {
    let host = host_with_modes(
        "claude-code",
        "approval_policy = \"on-request\"",
        "auto,acceptEdits,plan",
        CODEX_MODES,
    );
    let folder = TempDir::new().unwrap();
    let mut acp = controller(&host);
    let id = new_session(&mut acp, folder.path(), "claude-code").await;

    let refused = set_mode(&mut acp, &id, "ask").await.unwrap_err();
    let after = configuration(&mut acp, &id, folder.path()).await;

    assert_eq!(refused.code, -32602, "{refused:?}");
    assert_eq!(after["modes"]["currentModeId"], "agent");
    assert_eq!(mode_ids(&after["modes"]), ["agent", "approve-all"]);
    assert_eq!(
        mode_values(&after["configOptions"]),
        ["agent", "approve-all"]
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn switching_agent_in_ask_keeps_asking_where_the_new_adapter_can() {
    let host = host_with("claude-code", "");
    let folder = TempDir::new().unwrap();
    let mut acp = controller(&host);
    let id = new_session(&mut acp, folder.path(), "claude-code").await;
    set_mode(&mut acp, &id, "ask").await.unwrap();

    let switched = switch_agent(&mut acp, &id, "codex").await.unwrap();

    assert_eq!(
        switched["configOptions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|option| option["id"] == "mode")
            .unwrap()["currentValue"],
        "ask"
    );
    assert!(turn_asks(&mut acp, &id, "first").await, "codex asks");
}

#[tokio::test(flavor = "multi_thread")]
async fn switching_agent_in_ask_to_an_adapter_that_cannot_ask_continues_in_agent() {
    let host = host_with("claude-code", "");
    let folder = TempDir::new().unwrap();
    let mut acp = controller(&host);
    let id = new_session(&mut acp, folder.path(), "claude-code").await;
    set_mode(&mut acp, &id, "ask").await.unwrap();
    let mut watching = Acp::new(tokio::task::block_in_place(|| host.observer()));
    configuration(&mut watching, &id, folder.path()).await;

    switch_agent(&mut acp, &id, "opencode").await.unwrap();
    let after = configuration(&mut acp, &id, folder.path()).await;

    assert_eq!(after["modes"]["currentModeId"], "agent");
    assert_eq!(mode_ids(&after["modes"]), ["agent", "approve-all"]);
    watching
        .until(|frame| {
            frame["params"]["update"]["sessionUpdate"] == "current_mode_update"
                && frame["params"]["update"]["currentModeId"] == "agent"
        })
        .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_claude_code_session_offers_all_three_modes() {
    let host = host_with("claude-code", "");
    let folder = TempDir::new().unwrap();
    let mut acp = controller(&host);
    let id = new_session(&mut acp, folder.path(), "claude-code").await;

    let configuration = configuration(&mut acp, &id, folder.path()).await;

    assert_eq!(
        configuration["modes"],
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
async fn a_session_started_without_an_acp_client_gets_its_adapter_mode() {
    // Claude Code starts in a mode that does not ask; the folder's policy is
    // ask.
    let host = host_with_modes(
        "claude-code",
        "approval_policy = \"untrusted\"",
        "auto,default,acceptEdits,plan",
        CODEX_MODES,
    );
    let folder = TempDir::new().unwrap();
    let mut observer = tokio::task::block_in_place(|| host.observer());
    observer
        .request("_nori/session/subscribe", json!({"session_id": "*"}))
        .await
        .unwrap();

    let id = success(host.run(&["host", "new", "--cwd", folder.path().to_str().unwrap()]))
        .trim()
        .to_owned();
    let mut prompter = controller(&host);
    prompter.prompt_alias(&id, "first").await;

    let asked = tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            let frame = observer.recv().await.unwrap();
            let event = &frame["params"]["event"];
            if frame["method"] == "_nori/session/event"
                && event["source"] == "acp"
                && event["event"]["message_type"] == "request"
                && event["event"]["request"]["toolCall"].is_object()
            {
                return;
            }
        }
    })
    .await;
    assert!(asked.is_ok(), "the turn did not ask");
}

/// Against the real adapters, fetched by the host as usual: each built-in
/// adapter Nori maps a strict mode for still offers it, so sessions on it
/// are offered `ask`. Needs network access and each adapter's sign-in; run
/// with `cargo test -p nori-cli --test host_lifecycle -- --ignored real_`.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "starts the real Claude Code and Codex adapters"]
async fn real_adapters_offer_ask() {
    let host = Host::new();
    std::fs::write(
        host.0.path().join("config.toml"),
        "agent = \"claude-code\"\napproval_policy = \"on-request\"\n",
    )
    .unwrap();
    success(host.run(&["host", "--port", "0"]));
    let mut acp = controller(&host);
    let mut offered = Vec::new();
    for agent in ["claude-code", "codex"] {
        let folder = TempDir::new().unwrap();
        let id = new_session(&mut acp, folder.path(), agent).await;
        let configuration = configuration(&mut acp, &id, folder.path()).await;
        offered.push((agent, mode_ids(&configuration["modes"])));
    }

    let all = vec![
        "ask".to_owned(),
        "agent".to_owned(),
        "approve-all".to_owned(),
    ];
    assert_eq!(offered, [("claude-code", all.clone()), ("codex", all)]);
}
