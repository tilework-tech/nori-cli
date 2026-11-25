use super::Host;
use super::ManagedClient;
use super::start_config;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use std::fs;
use std::path::Path;
use std::time::Duration;

const PREPARE: &str = "_nori/host/prepare_agent";
const DISCARD: &str = "_nori/host/discard_agent";
const KEY: &str = "nori/preparedAgent";

async fn picker_host() -> (Host, ManagedClient, ManagedClient) {
    let host = Host::new();
    let mut config = String::from("agent = 'maple'\n");
    for name in [
        "maple",
        "elm",
        "waiting",
        "broken",
        "list-only",
        "declines",
        "empty",
        "resume-only",
    ] {
        config.push_str(&format!(
            "[[agents]]\nname = '{name}'\nslug = '{name}'\n[agents.distribution.local]\ncommand = 'python3'\nargs = ['{}/tests/standard_acp/picker_fixture.py','{name}','{}']\n",
            env!("CARGO_MANIFEST_DIR"), host.0.path().display()
        ));
    }
    let (host, endpoint, token) = start_config(host, config);
    let endpoint = endpoint.replace("/managed", "/sessions");
    let mut client = ManagedClient::connect(&endpoint, token.trim())
        .await
        .unwrap();
    let mut other = ManagedClient::connect(&endpoint, token.trim())
        .await
        .unwrap();
    let init = client.request("initialize", json!({})).await.unwrap();
    let option = &init["agentCapabilities"]["_meta"][KEY]["agentOption"];
    assert_eq!(option["currentValue"], "maple");
    assert!(
        option["options"]
            .as_array()
            .unwrap()
            .iter()
            .any(|o| o["value"] == "elm")
    );
    other
        .request("initialize", json!({"observe":true}))
        .await
        .unwrap();
    (host, client, other)
}

fn traffic(dir: &Path) -> Vec<Value> {
    fs::read_to_string(dir.join("traffic"))
        .unwrap_or_default()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

async fn wait_for_release(dir: &Path, pid: &Value) {
    let lease = fs::File::open(dir.join(pid.to_string())).unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match lease.try_lock() {
                Ok(()) => break,
                Err(fs::TryLockError::WouldBlock) => {
                    tokio::time::sleep(Duration::from_millis(10)).await
                }
                Err(error) => panic!("process lock: {error}"),
            }
        }
    })
    .await
    .expect("abandoned adapter still holds its process lock");
}

#[tokio::test]
async fn picker_activates_new_and_saved_rows_on_their_original_connections() {
    let (host, mut client, mut other) = picker_host().await;
    let cwd = host.0.path().canonicalize().unwrap();
    let mut live = Vec::new();
    for (agent, saved) in [
        ("maple", None),
        ("elm", Some("elm-saved")),
        ("empty", Some("empty-saved")),
        ("resume-only", Some("resume-only-saved")),
    ] {
        let selection = if saved.is_none() {
            json!({"cwd":cwd})
        } else {
            json!({"cwd":cwd,"agent":agent})
        };
        let choice = client.request(PREPARE, selection).await.unwrap();
        assert_eq!(choice["agent"], agent);
        assert_eq!(choice["cwd"], json!(cwd));
        let scope = json!({"id":choice["id"]});
        let meta = json!({(KEY):scope});
        let catalog = client
            .request("session/list", json!({"_meta":meta}))
            .await
            .unwrap();
        assert_eq!(
            catalog["sessions"][0]["sessionId"],
            format!("{agent}-saved")
        );
        assert_eq!(catalog["sessions"][0]["_meta"]["sample"], "picker");
        assert_eq!(
            catalog["_meta"][KEY]["agentCapabilities"]["loadSession"]
                .as_bool()
                .unwrap_or(false),
            agent != "resume-only"
        );
        assert!(
            other
                .request("session/list", json!({"_meta":meta}))
                .await
                .is_err()
        );
        assert_eq!(
            client
                .request("session/list", json!({"cwd":cwd}))
                .await
                .unwrap()["sessions"]
                .as_array()
                .unwrap()
                .len(),
            live.len()
        );
        let mut activation = scope;
        if let Some(saved) = saved {
            activation["savedSessionId"] = json!(saved);
        }
        let params = json!({"_meta":{(KEY):activation},"cwd":cwd,"mcpServers":[]});
        client
            .send(json!({"jsonrpc":"2.0","id":"activate","method":"session/new","params":params}))
            .await
            .unwrap();
        let mut replay = Vec::new();
        let id = tokio::time::timeout(Duration::from_secs(20), async {
            loop {
                let frame = client.recv().await.unwrap();
                if frame["id"] == "activate" {
                    break frame["result"]["sessionId"]
                        .as_str()
                        .unwrap_or_else(|| panic!("{frame}"))
                        .to_owned();
                }
                if let Some(text) = frame
                    .pointer("/params/update/content/text")
                    .and_then(Value::as_str)
                {
                    replay.push((frame["params"]["sessionId"].clone(), text.to_string()));
                }
            }
        })
        .await
        .unwrap();
        assert!(uuid::Uuid::parse_str(&id).is_ok());
        let history = if agent == "elm" { 64 } else { 0 };
        assert_eq!(
            replay,
            (0..history)
                .map(|i| (json!(id), format!("saved history {i}")))
                .collect::<Vec<_>>()
        );
        assert!(client.request("session/new", params).await.is_err());
        client
            .request(
                "session/prompt",
                json!({"sessionId":id,"prompt":[{"type":"text","text":"check"}]}),
            )
            .await
            .unwrap();
        let rows = traffic(&cwd)
            .into_iter()
            .filter(|r| r[1] == agent)
            .collect::<Vec<_>>();
        let pid = &rows[0][0];
        let adapter_id = saved
            .map(str::to_owned)
            .unwrap_or_else(|| format!("{agent}-new"));
        assert!(rows.iter().all(|r| &r[0] == pid && r[4] == json!(cwd)));
        assert_eq!(
            rows.iter()
                .map(|r| r[2].as_str().unwrap())
                .collect::<Vec<_>>(),
            [
                "initialize",
                "session/list",
                match (agent, saved) {
                    ("resume-only", _) => "session/resume",
                    (_, Some(_)) => "session/load",
                    (_, None) => "session/new",
                },
                "session/prompt"
            ]
        );
        assert_eq!(rows[1][5], json!(cwd));
        assert_eq!(rows[2][5], json!(cwd));
        assert_eq!(rows[3][3], adapter_id);
        eprintln!(
            "picker proof host={id} adapter={adapter_id} pid={pid} methods={:?}",
            rows.iter().map(|r| &r[2]).collect::<Vec<_>>()
        );
        live.push(id);
    }
    assert_ne!(live[0], live[1]);
}

#[tokio::test]
async fn picker_replacement_discard_disconnect_and_errors_leave_other_sessions_running() {
    let (host, mut client, mut other) = picker_host().await;
    let cwd = host.0.path().canonicalize().unwrap();
    let sibling = client
        .request("session/new", json!({"cwd":cwd,"mcpServers":[]}))
        .await
        .unwrap();
    let waiting = other
        .request(PREPARE, json!({"cwd":cwd,"agent":"waiting"}))
        .await
        .unwrap();
    let pid = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let Some(row) = traffic(&cwd)
                .into_iter()
                .find(|r| r[1] == "waiting" && r[2] == "session/list")
            {
                break row[0].clone();
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    other
        .send(
            json!({"jsonrpc":"2.0","id":"pending-list","method":"session/list",
        "params":{"_meta":{(KEY):{"id":waiting["id"]}}}}),
        )
        .await
        .unwrap();
    other
        .send(json!({"jsonrpc":"2.0","id":"switch","method":PREPARE,
        "params":{"cwd":cwd,"agent":"elm"}}))
        .await
        .unwrap();
    let ready = tokio::time::timeout(Duration::from_secs(5), async {
        let mut replacement = None;
        let mut cancelled = false;
        loop {
            let frame = other.recv().await.unwrap();
            if frame["id"] == "pending-list" {
                assert_eq!(frame["error"]["code"], -32002);
                cancelled = true;
            } else if frame["id"] == "switch" {
                replacement = frame.get("result").cloned();
            }
            if cancelled && let Some(result) = replacement.take() {
                break result;
            }
        }
    })
    .await
    .expect("a blocked catalog prevented switching agents");
    wait_for_release(&cwd, &pid).await;
    assert!(
        other
            .request(DISCARD, json!({"id":waiting["id"]}))
            .await
            .is_err()
    );
    let list = |choice: &Value| json!({"_meta":{(KEY):{"id":choice["id"]}}});
    other.request("session/list", list(&ready)).await.unwrap();
    assert!(
        client
            .request(DISCARD, json!({"id":ready["id"]}))
            .await
            .is_err()
    );
    other
        .request(DISCARD, json!({"id":ready["id"]}))
        .await
        .unwrap();
    wait_for_release(
        &cwd,
        &traffic(&cwd).iter().find(|r| r[1] == "elm").unwrap()[0],
    )
    .await;
    let broken = other
        .request(PREPARE, json!({"cwd":cwd,"agent":"broken"}))
        .await
        .unwrap();
    let error = other
        .request("session/list", list(&broken))
        .await
        .unwrap_err();
    assert!(error.to_string().contains("catalog unavailable"), "{error}");
    let choice = other
        .request(PREPARE, json!({"cwd":cwd,"agent":"list-only"}))
        .await
        .unwrap();
    other.request("session/list", list(&choice)).await.unwrap();
    let params = json!({"cwd":cwd,"mcpServers":[],"_meta":{(KEY):{"id":choice["id"],"savedSessionId":"list-only-saved"}}});
    assert!(other.request("session/new", params).await.is_err());
    let declined = other
        .request(PREPARE, json!({"cwd":cwd,"agent":"declines"}))
        .await
        .unwrap();
    let failed = other.request("session/new", json!({"cwd":cwd,"_meta":{(KEY):{"id":declined["id"],"savedSessionId":"declines-saved"}}})).await.unwrap_err();
    assert!(failed.to_string().contains("restore declined"), "{failed}");
    let live = other
        .request("session/list", json!({"cwd":cwd}))
        .await
        .unwrap();
    assert_eq!(live["sessions"].as_array().unwrap().len(), 1, "{live}");
    let last = other
        .request(PREPARE, json!({"cwd":cwd,"agent":"maple"}))
        .await
        .unwrap();
    other.request("session/list", list(&last)).await.unwrap();
    let last_pid = traffic(&cwd)
        .into_iter()
        .rev()
        .find(|r| r[1] == "maple")
        .unwrap()[0]
        .clone();
    let lease = fs::File::open(cwd.join(last_pid.to_string())).unwrap();
    assert!(matches!(
        lease.try_lock(),
        Err(fs::TryLockError::WouldBlock)
    ));
    drop(other);
    wait_for_release(&cwd, &last_pid).await;
    let rows = traffic(&cwd);
    wait_for_release(&cwd, &rows.iter().find(|r| r[1] == "list-only").unwrap()[0]).await;
    assert!(
        !rows
            .iter()
            .any(|r| r[1] == "list-only" && r[2] == "session/new")
    );
    client.request("session/prompt", json!({"sessionId":sibling["sessionId"],"prompt":[{"type":"text","text":"still here"}]})).await.unwrap();
}
