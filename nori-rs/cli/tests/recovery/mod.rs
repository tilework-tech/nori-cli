use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::process::Child;
use std::process::Command;
use std::process::Stdio;
use std::time::Duration;

use nori_harness::remote_agent::ManagedClient;
use serde_json::Value;
use serde_json::json;
use tokio::time::sleep;
use tokio::time::timeout;

mod storage;

const DEADLINE: Duration = Duration::from_secs(30);

pub struct Host {
    process: Option<Child>,
    home: PathBuf,
    managed: bool,
}

impl Host {
    pub async fn start(home: &Path, managed: bool) -> Self {
        Self::with_env(home, managed, false, &[]).await
    }

    pub async fn with_env(
        home: &Path,
        managed: bool,
        side_sessions: bool,
        env: &[(&str, &Path)],
    ) -> Self {
        let config = format!(
            "[features]\nmulti_session = true\n[host]\nmanaged = {managed}\nside_sessions = {side_sessions}\n"
        );
        fs::write(home.join("config.toml"), config).unwrap();
        let process = Command::new(env!("CARGO_BIN_EXE_nori"))
            .env_clear()
            .envs(env.iter().copied())
            .env("HOME", home)
            .env("NORI_HOME", home)
            .env("PATH", std::env::var_os("PATH").unwrap())
            .current_dir(home)
            .args(["host", "--foreground", "--port", "0"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let mut host = Self {
            process: Some(process),
            home: home.to_owned(),
            managed,
        };
        timeout(DEADLINE, async {
            loop {
                let process = host.process.as_mut().unwrap();
                assert!(
                    process.try_wait().unwrap().is_none(),
                    "host exited during startup"
                );
                if let Some(state) = host_state(home)
                    && state["pid"] == i64::from(process.id())
                {
                    break;
                }
                sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("host did not publish its endpoint");
        host
    }

    pub fn crash(&mut self) {
        if let Some(mut process) = self.process.take() {
            let _ = process.kill();
            process.wait().unwrap();
        }
    }

    pub async fn connect(&self) -> Wire {
        let state = host_state(&self.home).unwrap();
        let endpoint = state["endpoint"].as_str().unwrap();
        let token = fs::read_to_string(self.home.join("host/token")).unwrap();
        let mut client = ManagedClient::connect(endpoint, token.trim())
            .await
            .unwrap();
        client.request("initialize", json!({})).await.unwrap();
        let acp = if self.managed {
            let endpoint = format!("{}/managed", endpoint.strip_suffix("/sessions").unwrap());
            Some(
                ManagedClient::connect(&endpoint, token.trim())
                    .await
                    .unwrap(),
            )
        } else {
            None
        };
        Wire {
            client,
            acp,
            next_id: 10,
        }
    }
}

fn host_state(home: &Path) -> Option<Value> {
    serde_json::from_slice(&fs::read(home.join("host/state.json")).ok()?).ok()
}

impl Drop for Host {
    fn drop(&mut self) {
        self.crash();
    }
}

pub struct Binding {
    pub id: String,
    pub conversation_id: String,
    roots: PathBuf,
}

impl Binding {
    pub fn new(host_home: &Path) -> Self {
        let id = uuid::Uuid::new_v4().to_string();
        Self::at(host_home, id, uuid::Uuid::new_v4().to_string())
    }

    fn at(host_home: &Path, id: String, conversation_id: String) -> Self {
        let roots = host_home.join("hosted-sessions").join(&id);
        fs::create_dir_all(&roots).unwrap();
        for name in ["workspace", "home", "config"] {
            fs::create_dir(roots.join(name)).unwrap();
        }
        Self {
            id,
            conversation_id,
            roots: roots.canonicalize().unwrap(),
        }
    }

    pub fn relocated(&self, host_home: &Path) -> Self {
        Self::at(host_home, self.id.clone(), self.conversation_id.clone())
    }

    pub fn disable_load(&self) {
        fs::write(self.roots.join("workspace/no-load"), "").unwrap();
    }

    pub fn spec(&self) -> Value {
        let root = &self.roots;
        json!({
            "agent": "recovery-fixture",
            "projectId": "fixture-project",
            "runId": "fixture-run",
            "cwd": root.join("workspace"),
            "home": root.join("home"),
            "additions": {"promptEcho":false,"statusEvents":false,"recording":false,"clientMcp":false},
            "adapter": {
                "path": Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/recovery/adapter.py"),
                "version": "1"
            }
        })
    }

    pub fn side_effects(&self) -> Vec<String> {
        match fs::read_to_string(self.roots.join("workspace/executions")) {
            Ok(log) => log.lines().map(str::to_owned).collect(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(error) => panic!("cannot read fixture side effects: {error}"),
        }
    }

    pub fn unblock(&self) {
        fs::write(self.roots.join("workspace/unblock"), "").unwrap();
    }
}

pub struct Wire {
    pub client: ManagedClient,
    pub acp: Option<ManagedClient>,
    next_id: i64,
}

impl Wire {
    pub async fn start(&mut self, b: &Binding, generation: i64) -> String {
        self.client
            .request(
                "_nori/session/ensure",
                json!({
                    "commandId": "start", "conversationId": b.conversation_id, "recoveryVersion": 1,
                    "bindingId": b.id, "generation": generation, "spec": b.spec(),
                }),
            )
            .await
            .unwrap();
        self.initialize(b, generation).await;
        self.new_session(b, generation).await
    }

    pub async fn initialize(&mut self, b: &Binding, generation: i64) -> Value {
        self.send(
            b,
            generation,
            "initialize",
            json!({"protocolVersion": 1,"clientCapabilities":{}}),
            None,
        )
        .await;
        self.expect_reply().await
    }

    pub async fn load(&mut self, b: &Binding, generation: i64, session: &str) {
        self.send(
            b,
            generation,
            "session/load",
            json!({
                "sessionId":session, "cwd": b.roots.join("workspace"), "mcpServers":[]
            }),
            None,
        )
        .await;
        self.expect_reply().await;
    }

    pub async fn new_session(&mut self, b: &Binding, generation: i64) -> String {
        self.send(
            b,
            generation,
            "session/new",
            json!({"cwd": b.roots.join("workspace"),"mcpServers":[]}),
            None,
        )
        .await;
        self.expect_reply().await["sessionId"]
            .as_str()
            .unwrap()
            .to_owned()
    }

    async fn send(
        &mut self,
        b: &Binding,
        generation: i64,
        method: &str,
        params: Value,
        command: Option<&str>,
    ) {
        self.next_id += 1;
        let mut frame = json!({
            "bindingId": b.id, "generation": generation,
            "message": {"jsonrpc":"2.0","id":self.next_id,"method":method,"params":params}
        });
        if let Some(command) = command {
            frame["commandId"] = json!(command);
        }
        self.acp.as_mut().unwrap().send(frame).await.unwrap();
    }

    pub async fn send_prompt(
        &mut self,
        b: &Binding,
        generation: i64,
        session: &str,
        command: &str,
        text: &str,
    ) {
        self.send(
            b,
            generation,
            "session/prompt",
            json!({"sessionId":session,"prompt":[{"type":"text","text":text}]}),
            Some(command),
        )
        .await
    }

    pub async fn text_event(&mut self, text: &str) -> Value {
        timeout(DEADLINE, async {
            loop {
                let frame = self.acp.as_mut().unwrap().recv().await.unwrap();
                let message = &frame["message"];
                assert!(message.get("error").is_none(), "{frame}");
                if message["method"] == "session/update"
                    && message.pointer("/params/update/content/text") == Some(&json!(text)) {
                    return json!({"source":frame["source"],"sessionId":message["params"]["sessionId"],"message":message});
                }
            }
        }).await.expect("missing fixture output")
    }

    pub async fn prompt_reply(&mut self) -> Value {
        timeout(DEADLINE, async {
            loop {
                let frame = self.acp.as_mut().unwrap().recv().await.unwrap();
                if frame["message"]["id"] == self.next_id {
                    return frame["message"].clone();
                }
            }
        })
        .await
        .expect("missing ACP response")
    }

    async fn expect_reply(&mut self) -> Value {
        let reply = self.prompt_reply().await;
        assert!(reply.get("result").is_some(), "{reply}");
        reply["result"].clone()
    }

    pub async fn replay(&mut self, b: &Binding, generation: i64) -> Vec<Value> {
        self.client
            .request(
                "_nori/host/replay",
                json!({"bindingId":b.id,"generation":generation}),
            )
            .await
            .unwrap()["records"]
            .as_array()
            .unwrap()
            .clone()
    }

    pub async fn ack(&mut self, b: &Binding, generation: i64, record: &Value) {
        self.client
            .request(
                "_nori/host/ack",
                json!({"bindingId":b.id,"generation":generation,"source":record["source"]}),
            )
            .await
            .unwrap();
    }

    pub async fn checkpoint(&mut self, b: &Binding, generation: i64) -> Value {
        self.client
            .request(
                "_nori/host/checkpoint",
                json!({"bindingId":b.id,"generation":generation}),
            )
            .await
            .unwrap()["checkpoint"]
            .clone()
    }

    pub async fn import(&mut self, b: &Binding, generation: i64, checkpoint: Value) -> Value {
        self.client.request("_nori/host/import",json!({"bindingId":b.id,"generation":generation,"checkpoint":checkpoint,"launchSpec":b.spec()})).await.unwrap()
    }

    pub async fn release(&mut self, b: &Binding, generation: i64, command: &str) -> Value {
        self.client
            .request(
                "_nori/host/release",
                json!({"bindingId":b.id,"generation":generation,"commandId":command}),
            )
            .await
            .unwrap()
    }
}
