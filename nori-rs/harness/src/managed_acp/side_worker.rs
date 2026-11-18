//! ACP dialogue with one independently supervised, read-only adapter.

use std::path::Path;
use std::time::Duration;

use anyhow::Context;
use anyhow::Result;
use anyhow::ensure;
use serde_json::Value;
use serde_json::json;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tokio_util::sync::DropGuard;

use super::SideLaunch;
use crate::backend::managed_additions::ManagedSessionAdditions;
use crate::managed_acp::StandardAcpConnection;
use crate::managed_acp::connect;

type Events = mpsc::Sender<(String, Option<Value>)>;

fn failure(id: Value, code: i32, reason: &str) -> Value {
    json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":reason}})
}

async fn exchange(
    acp: &mut StandardAcpConnection,
    method: &str,
    params: Value,
    expected_mode: Option<&str>,
) -> Result<Value> {
    // Setup requests run sequentially before any client requests are admitted.
    acp.input
        .send(json!({"jsonrpc":"2.0","id":0,"method":method,"params":params}))
        .await?;
    loop {
        let message = acp
            .output
            .recv()
            .await
            .context("side adapter disconnected during setup")?;
        if message["method"] == "session/update"
            && message["params"]["sessionId"] == params["sessionId"]
            && let Some(expected) = expected_mode
        {
            ensure!(
                matches_fixed_mode(
                    &message["params"]["update"],
                    expected,
                    params["configId"].as_str()
                ),
                "side adapter reported a different permission mode"
            );
        }
        if message.get("method").is_some() {
            if let Some(id) = message.get("id") {
                acp.input
                    .send(failure(
                        id.clone(),
                        -32601,
                        "client services unavailable during side setup",
                    ))
                    .await?;
            }
            continue;
        }
        ensure!(message["id"] == 0, "unexpected side setup response");
        if let Some(error) = message.get("error") {
            anyhow::bail!(
                "{method} failed: {}",
                error["message"].as_str().unwrap_or("adapter error")
            );
        }
        ensure!(message["result"].is_object(), "invalid side setup result");
        return Ok(message["result"].clone());
    }
}

fn option<'a>(result: &'a Value, category: &str) -> Option<&'a Value> {
    result["configOptions"]
        .as_array()?
        .iter()
        .find(|item| item["category"] == category)
}

fn matches_fixed_mode(state: &Value, expected: &str, config_id: Option<&str>) -> bool {
    if state["sessionUpdate"] == "current_mode_update" {
        return state["currentModeId"] == expected;
    }
    let Some(options) = state.get("configOptions") else {
        return state["sessionUpdate"] != "config_option_update";
    };
    matches_config_option(options, "mode", config_id, expected)
}

fn matches_config_option(
    options: &Value,
    category: &str,
    id: Option<&str>,
    expected: &str,
) -> bool {
    let Some(options) = options.as_array() else {
        return false;
    };
    let mut matches = options
        .iter()
        .filter(|item| item["category"] == category || id.is_some_and(|id| item["id"] == id));
    let Some(option) = matches.next() else {
        return id.is_none();
    };
    option["category"] == category
        && id.is_none_or(|id| option["id"] == id)
        && option["currentValue"] == expected
        && matches.next().is_none()
}

async fn select_option(
    acp: &mut StandardAcpConnection,
    result: &mut Value,
    sid: &str,
    category: &str,
    value: &str,
) -> Result<()> {
    let item = option(result, category).context("missing advertised side option")?;
    if category != "mode" && item["currentValue"] == value {
        return Ok(());
    }
    let id = item["id"]
        .as_str()
        .context("missing side configuration ID")?;
    let mut response = exchange(
        acp,
        "session/set_config_option",
        json!({"sessionId":sid,"configId":id,"value":value}),
        (category == "mode").then_some(value),
    )
    .await?;
    ensure!(
        matches_config_option(&response["configOptions"], category, Some(id), value),
        "side adapter did not apply the requested {category}"
    );
    result["configOptions"] = response["configOptions"].take();
    Ok(())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CodexAuth {
    Unverified,
    SubscriptionFile,
}

struct FixedMode {
    subscription_file: bool,
    value: &'static str,
    config_id: Option<String>,
}

async fn open(
    acp: &mut StandardAcpConnection,
    mut init: Value,
    cwd: &Path,
    model: Option<&str>,
    codex_auth: Option<CodexAuth>,
) -> Result<(Value, String, FixedMode)> {
    // A side process can ask for permissions. Filesystem and terminal services
    // supplied by an ACP client would run outside its read-only mount.
    init["clientCapabilities"] = json!({});
    let initialized = exchange(acp, "initialize", init, None).await?;
    if let Some(auth) = codex_auth {
        // This pin reports its account after the initialize response.
        // Account labels and API key status cannot identify the parent's credential.
        const UNSUPPORTED: &str =
            "unsupported Codex side session: credential scope cannot be confirmed";
        ensure!(
            initialized["agentCapabilities"]["_meta"]["authStatus"].is_object(),
            UNSUPPORTED
        );
        loop {
            let message = acp.output.recv().await.context(UNSUPPORTED)?;
            ensure!(
                message.get("id").is_none() && message["method"].is_string(),
                UNSUPPORTED
            );
            if message["method"] == "_auth/status_update" {
                ensure!(
                    message["params"]["authStatus"]["kind"]
                        == match auth {
                            CodexAuth::Unverified => "none",
                            CodexAuth::SubscriptionFile => "account",
                        },
                    UNSUPPORTED
                );
                break;
            }
        }
    }
    let mut result = exchange(acp, "session/new", json!({"cwd":cwd,"mcpServers":[]}), None).await?;
    ensure!(
        codex_auth != Some(CodexAuth::Unverified),
        "unsupported Codex side session: session/new succeeded without a verified credential scope"
    );
    let sid = result["sessionId"]
        .as_str()
        .filter(|id| !id.is_empty())
        .context("missing side adapter session ID")?
        .to_owned();
    if let Some(model) = model {
        if option(&result, "model").is_some() {
            select_option(acp, &mut result, &sid, "model", model).await?;
        } else if result["models"]["currentModelId"]
            .as_str()
            .is_some_and(|current| current != model)
        {
            exchange(
                acp,
                "session/set_model",
                json!({"sessionId":sid,"modelId":model}),
                None,
            )
            .await?;
            result["models"]["currentModelId"] = json!(model);
        }
    }

    let select = option(&result, "mode");
    let choices = select
        .map_or(
            &result["modes"]["availableModes"],
            |option| &option["options"],
        )
        .as_array()
        .context("side adapter has no selectable permission mode")?;
    let value = ["read-only", "plan"]
        .into_iter()
        .find(|name| {
            choices.iter().any(|entry| {
                if select.is_some() {
                    entry["value"] == *name
                        || entry["options"]
                            .as_array()
                            .is_some_and(|group| group.iter().any(|item| item["value"] == *name))
                } else {
                    entry["id"] == *name
                }
            })
        })
        .context("side adapter has no supported restrictive mode")?;
    let config_id = if let Some(option) = select {
        Some(
            option["id"]
                .as_str()
                .context("missing mode option ID")?
                .to_owned(),
        )
    } else {
        None
    };
    if config_id.is_some() {
        if !result["modes"].is_null() {
            ensure!(
                result["modes"]["availableModes"]
                    .as_array()
                    .is_some_and(|modes| modes.iter().any(|mode| mode["id"] == value)),
                "side adapter has no equivalent legacy permission mode"
            );
        }
        select_option(acp, &mut result, &sid, "mode", value).await?;
        if !result["modes"].is_null() {
            result["modes"]["currentModeId"] = json!(value);
        }
    } else if result["modes"]["currentModeId"] != value {
        exchange(
            acp,
            "session/set_mode",
            json!({"sessionId":sid,"modeId":value}),
            Some(value),
        )
        .await?;
        result["modes"]["currentModeId"] = json!(value);
    }
    Ok((
        result,
        sid,
        FixedMode {
            value,
            config_id,
            subscription_file: codex_auth == Some(CodexAuth::SubscriptionFile),
        },
    ))
}

pub(super) struct SideInput {
    pub sender: mpsc::Sender<Value>,
    _cancel: DropGuard,
}

pub(super) fn start_side(
    launch: SideLaunch,
    init_params: Value,
    request_id: Value,
    public_sid: String,
    events: Events,
) -> SideInput {
    let (sender, mut requests) = mpsc::channel(32);
    let cancel = CancellationToken::new();
    let input = SideInput {
        sender,
        _cancel: cancel.clone().drop_guard(),
    };
    tokio::spawn(async move {
        let SideLaunch { connection, owner } = launch;
        let spec = connection.spec;
        let additions = ManagedSessionAdditions::new(
            connection.home.join(".nori/cli"),
            connection.cwd.clone(),
            spec.agent.clone(),
            false,
            false,
        );
        let mut acp = connect(
            connection.stdout,
            connection.stdin,
            connection.cwd.clone(),
            additions,
            false,
            false,
        );
        let send = |message| events.send((public_sid.clone(), Some(message)));
        let run = async {
            let codex_auth = spec.uses_private_codex_home().then(|| {
                if spec.has_subscription_file() {
                    CodexAuth::SubscriptionFile
                } else {
                    CodexAuth::Unverified
                }
            });
            let setup = tokio::time::timeout(
                Duration::from_secs(30),
                open(
                    &mut acp,
                    init_params,
                    &connection.cwd,
                    spec.model.as_deref(),
                    codex_auth,
                ),
            )
            .await
            .context(if codex_auth.is_some() {
                "unsupported Codex side session: authentication setup timed out"
            } else {
                "side adapter setup timed out"
            })
            .and_then(|inner| inner);
            match setup {
                Ok((mut result, internal_sid, mode)) => {
                    result["sessionId"] = json!(public_sid);
                    if send(json!({"jsonrpc":"2.0","id":request_id,"result":result}))
                        .await
                        .is_ok()
                    {
                        serve(
                            &mut acp,
                            &mut requests,
                            &events,
                            &public_sid,
                            &internal_sid,
                            &mode,
                        )
                        .await
                    } else {
                        None
                    }
                }
                Err(error) => Some(failure(request_id, -32000, &format!("{error:#}"))),
            }
        };
        let final_reply = tokio::select! {
            biased;
            _ = cancel.cancelled() => None,
            reply = run => reply,
        };
        drop(acp);
        owner.stop().await;
        if let Some(message) = final_reply {
            let _ = send(message).await;
        }
        let _ = events.send((public_sid, None)).await;
    });
    input
}

// Codex ACP handles this command locally before starting a model turn.
fn is_codex_logout(request: &Value) -> bool {
    let block = &request["params"]["prompt"][0];
    if block["type"] != "text" {
        return false;
    }
    let space = |ch: char| ch.is_whitespace() || ch == '\u{feff}';
    block["text"]
        .as_str()
        .and_then(|text| text.trim_start_matches(space).strip_prefix('/'))
        .and_then(|command| command.trim_start_matches(space).split(space).next())
        .is_some_and(|name| name.eq_ignore_ascii_case("logout"))
}

async fn serve(
    acp: &mut StandardAcpConnection,
    requests: &mut mpsc::Receiver<Value>,
    events: &Events,
    public_sid: &str,
    internal_sid: &str,
    mode: &FixedMode,
) -> Option<Value> {
    loop {
        tokio::select! {
            request = requests.recv() => {
                let mut request = request?;
                if let Some(method) = request["method"].as_str() {
                    if mode.subscription_file && method == "session/prompt" && is_codex_logout(&request) {
                        events.send((public_sid.into(), Some(failure(request["id"].clone(), -32602, "the binding owns this Codex subscription")))).await.ok()?;
                        continue;
                    }
                    if method == "session/close" {
                        return Some(json!({"jsonrpc":"2.0","id":request["id"],"result":{}}));
                    }
                    if (method == "session/set_mode" && request["params"]["modeId"] != mode.value)
                        || (method == "session/set_config_option"
                            && !(mode.config_id.as_deref().is_some_and(|id| request["params"]["configId"] == id)
                                && request["params"]["value"] == mode.value)) {
                        events.send((public_sid.into(), Some(failure(request["id"].clone(), -32602, "side session permission mode is fixed")))).await.ok()?;
                        continue;
                    }
                    request["params"]["sessionId"] = json!(internal_sid);
                }
                acp.input.try_send(request).ok()?;
            }
            message = acp.output.recv() => {
                let mut message = message?;
                if let Some(method) = message["method"].as_str() {
                    if mode.subscription_file && method == "_auth/status_update" && message["params"]["authStatus"]["kind"] != "account" {
                        return None;
                    }
                    if !matches!(method, "session/update" | "session/request_permission") || message["params"]["sessionId"] != internal_sid {
                        if let Some(id) = message.get("id") {
                            acp.input.try_send(failure(id.clone(), -32601, "client service unavailable on a side session")).ok()?;
                        }
                        continue;
                    }
                    if method == "session/update"
                        && !matches_fixed_mode(&message["params"]["update"], mode.value, mode.config_id.as_deref()) {
                        return None;
                    }
                    message["params"]["sessionId"] = json!(public_sid);
                } else if !matches_fixed_mode(&message["result"], mode.value, mode.config_id.as_deref()) {
                    return None;
                }
                events.send((public_sid.into(), Some(message))).await.ok()?;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    async fn open_with_modes(
        initial: Value,
        setting_reply: Value,
        setting_update: Option<Value>,
    ) -> Result<Value> {
        let (input, _requests) = mpsc::channel(4);
        let (responses, output) = mpsc::channel(4);
        for result in [json!({"protocolVersion":1}), initial] {
            responses.try_send(json!({"id":0,"result":result})).unwrap();
        }
        if let Some(update) = setting_update {
            responses.try_send(json!({"method":"session/update","params":{"sessionId":"side","update":update}})).unwrap();
        }
        responses
            .try_send(json!({"id":0,"result":setting_reply}))
            .unwrap();
        drop(responses);
        open(
            &mut StandardAcpConnection { input, output },
            json!({}),
            Path::new("/work"),
            None,
            None,
        )
        .await
        .map(|(result, _, _)| result)
    }

    #[tokio::test]
    async fn subscribed_codex_requires_account_status_before_open() {
        for kind in ["none", "api_key", "gateway", "account"] {
            let (input, _requests) = mpsc::channel(8);
            let (responses, output) = mpsc::channel(8);
            for message in [
                json!({"id":0,"result":{"agentCapabilities":{"_meta":{"authStatus":{}}}}}),
                json!({"method":"_auth/status_update","params":{"authStatus":{"kind":kind}}}),
                json!({"id":0,"result":{"sessionId":"fixture","modes":{"currentModeId":"read-only","availableModes":[{"id":"read-only"}]}}}),
            ] {
                responses.try_send(message).unwrap();
            }
            let result = open(
                &mut StandardAcpConnection { input, output },
                json!({}),
                Path::new("/work"),
                None,
                Some(CodexAuth::SubscriptionFile),
            )
            .await;
            assert_eq!(result.is_ok(), kind == "account");
        }
    }

    #[tokio::test]
    async fn pinned_codex_only_probes_the_unauthenticated_boundary() {
        for (kind, reply, attempted_new, reason) in [
            ("account", json!({}), false, "unsupported"),
            (
                "none",
                json!({"error":{"code":-32000,"message":"Authentication required"}}),
                true,
                "Authentication required",
            ),
            (
                "none",
                json!({"result":{"sessionId":"fixture","modes":{"currentModeId":"read-only","availableModes":[{"id":"read-only"}]}}}),
                true,
                "unsupported",
            ),
        ] {
            let (input, mut requests) = mpsc::channel(4);
            let (responses, output) = mpsc::channel(4);
            responses
                .try_send(
                    json!({"id":0,"result":{"agentCapabilities":{"_meta":{"authStatus":{}}}}}),
                )
                .unwrap();
            responses
                .try_send(
                    json!({"method":"_auth/status_update","params":{"authStatus":{"kind":kind}}}),
                )
                .unwrap();
            let mut reply = reply;
            reply["id"] = json!(0);
            responses.try_send(reply).unwrap();
            drop(responses);
            let mut connection = StandardAcpConnection { input, output };
            let error = open(
                &mut connection,
                json!({}),
                Path::new("/work"),
                None,
                Some(CodexAuth::Unverified),
            )
            .await
            .err()
            .expect("credential scope is unverified");
            assert!(error.to_string().contains(reason), "{error}");
            assert_eq!(requests.try_recv().unwrap()["method"], "initialize");
            assert_eq!(requests.try_recv().is_ok(), attempted_new);
        }
    }

    fn mode_options(current: &str) -> Value {
        json!([{"type":"select","category":"mode","id":"mode","currentValue":current,
                 "options":[{"value":"auto","name":"Auto"},{"value":"plan","name":"Plan"}]}])
    }

    fn dual_modes() -> Value {
        json!({"sessionId":"side", "configOptions":mode_options("auto"),
               "modes":{"currentModeId":"auto", "availableModes":[{"id":"auto","name":"Auto"},{"id":"plan","name":"Plan"}]}})
    }

    #[tokio::test]
    async fn confirmed_side_mode_is_reported_in_both_acp_selectors() {
        let mut expected = dual_modes();
        expected["configOptions"] = mode_options("plan");
        expected["modes"]["currentModeId"] = json!("plan");
        let result = open_with_modes(
            dual_modes(),
            json!({"configOptions":mode_options("plan")}),
            None,
        )
        .await
        .unwrap();
        assert_eq!(result, expected);
    }

    #[tokio::test]
    async fn side_mode_requires_confirmation_from_the_selected_config_option() {
        let mut wrong_option = mode_options("plan");
        wrong_option[0]["id"] = json!("other");
        let mut initial = dual_modes();
        initial["configOptions"] = mode_options("plan");
        let conflicting = json!([mode_options("plan")[0], mode_options("auto")[0]]);
        for returned in [mode_options("auto"), json!([]), wrong_option, conflicting] {
            assert!(
                open_with_modes(initial.clone(), json!({"configOptions":returned}), None)
                    .await
                    .is_err()
            );
        }
    }

    #[tokio::test]
    async fn removing_the_pinned_mode_from_a_config_update_refuses_the_side() {
        let update = json!({"sessionUpdate":"config_option_update", "configOptions":[]});
        assert!(
            open_with_modes(
                dual_modes(),
                json!({"configOptions":mode_options("plan")}),
                Some(update)
            )
            .await
            .is_err()
        );
    }

    #[tokio::test]
    async fn side_mode_requires_an_equivalent_legacy_id_when_both_selectors_are_present() {
        let mut session = dual_modes();
        session["modes"]["availableModes"][1]["id"] = json!("draft");
        assert!(
            open_with_modes(session, json!({"configOptions":mode_options("plan")}), None)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn a_permission_prompt_mode_is_insufficient_for_a_rebuilt_side() {
        let session = json!({"sessionId":"side", "modes":{"currentModeId":"default",
            "availableModes":[{"id":"default","name":"Read only"}]}});
        assert!(open_with_modes(session, json!({}), None).await.is_err());
    }

    #[tokio::test]
    async fn explicit_mode_fallback_refuses_the_side_even_with_a_successful_setter_reply() {
        let legacy = json!({"sessionId":"side", "modes":dual_modes()["modes"]});
        let fallback = json!({"sessionUpdate":"current_mode_update","currentModeId":"auto"});
        for (session, reply) in [
            (legacy, json!({})),
            (dual_modes(), json!({"configOptions":mode_options("plan")})),
        ] {
            assert!(
                open_with_modes(session, reply, Some(fallback.clone()))
                    .await
                    .is_err()
            );
        }
    }
}
