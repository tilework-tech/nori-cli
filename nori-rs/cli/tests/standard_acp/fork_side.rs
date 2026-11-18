use super::Host;
use super::ManagedClient;
use super::admit_fixture;
use super::start_config;
use super::success;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;

const ADAPTER: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/standard_acp/fork_fixture.py"
);
const MARK: &str = "nori.dev/sideSession";

struct Peer {
    host: Host,
    machine: ManagedClient,
    binding: String,
    cwd: PathBuf,
    parent: Value,
    marker: Value,
}

impl Peer {
    async fn new(binding: &str, enabled: bool) -> Self {
        Self::start(binding, enabled, false).await.0
    }

    async fn start(binding: &str, enabled: bool, recovery: bool) -> (Self, ManagedClient) {
        let (host, endpoint, token) = start_config(
            Host::new(),
            format!("[host]\nmanaged = true\nside_sessions = {enabled}\nmax_sessions = 1\n"),
        );
        let mut control =
            ManagedClient::connect(&endpoint.replace("/managed", "/sessions"), &token)
                .await
                .unwrap();
        let discovery = control.request("initialize", json!({})).await.unwrap();
        let advertised =
            discovery.pointer("/agentCapabilities/_meta/nori.dev~1managedHost/forkSideSessions");
        assert_eq!(advertised, enabled.then_some(&json!(1)), "{discovery}");
        let cwd = admit_fixture(
            &mut control,
            &host,
            binding,
            [false, false, true, false],
            ADAPTER,
            recovery,
        )
        .await;
        let mut peer = Self {
            host,
            machine: ManagedClient::connect(&endpoint, &token).await.unwrap(),
            binding: binding.into(),
            cwd,
            parent: Value::Null,
            marker: Value::Null,
        };
        peer.call(
            1,
            "initialize",
            json!({"protocolVersion":1,"clientCapabilities":{}}),
        )
        .await;
        let opened = peer
            .call(2, "session/new", json!({"cwd":peer.cwd,"mcpServers":[]}))
            .await;
        peer.parent = opened["result"]["sessionId"].clone();
        assert!(peer.parent.is_string(), "{opened}");
        peer.marker = json!({"bindingId":binding,"generation":1,"sessionId":peer.parent});
        (peer, control)
    }

    async fn send(&mut self, msg: Value) {
        self.machine
            .send(json!({"bindingId":self.binding,"generation":1,"message":msg}))
            .await
            .unwrap();
    }

    async fn recv(&mut self) -> Value {
        self.recv_within(Duration::from_secs(5)).await
    }

    async fn recv_within(&mut self, deadline: Duration) -> Value {
        let frame = tokio::time::timeout(deadline, self.machine.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(frame["bindingId"], self.binding);
        assert!(
            frame["message"].is_object(),
            "unexpected end of managed connection: {frame}"
        );
        frame["message"].clone()
    }

    async fn call(&mut self, id: i64, method: &str, params: Value) -> Value {
        self.send(json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))
            .await;
        let response = self.recv().await;
        assert_eq!(response["id"], id, "{response}");
        response
    }

    async fn recv_until(&mut self, id: i64, earlier: &mut Vec<Value>) -> Value {
        loop {
            let frame = self.recv().await;
            if frame["id"] == id {
                return frame;
            }
            earlier.push(frame);
        }
    }

    async fn answer_child_permission(&mut self, child: &Value, option: &str) -> Value {
        let permission = self.recv().await;
        assert_eq!(permission["method"], "session/request_permission");
        assert_eq!(permission["params"]["sessionId"], *child);
        assert_eq!(permission["params"]["_meta"][MARK], self.marker);
        assert_eq!(permission["params"]["toolCall"]["toolCallId"], option);
        let decision = json!({"outcome":{"outcome":"selected","optionId":option}});
        self.send(json!({"jsonrpc":"2.0","id":permission["id"],"result":decision}))
            .await;
        decision
    }

    fn fork(&self, id: i64) -> Value {
        json!({"jsonrpc":"2.0","id":id,"method":"session/fork","params":{
            "cwd":self.cwd,"mcpServers":[],"sessionId":self.parent,"_meta":{(MARK):self.marker}}})
    }

    async fn open_fork(&mut self, id: i64) -> Value {
        self.send(self.fork(id)).await;
        let opened = self.recv().await;
        assert_eq!(opened["id"], id, "{opened}");
        assert_eq!(opened["result"]["_meta"][MARK], self.marker);
        let side = opened["result"]["sessionId"].clone();
        assert!(side.is_string());
        assert_ne!(side, self.parent);
        side
    }

    async fn listed_sessions(&mut self, id: i64) -> Vec<Value> {
        self.call(id, "session/list", json!({})).await["result"]["sessions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| row["sessionId"].clone())
            .collect()
    }

    async fn active_adapter_sessions(&mut self) -> Vec<Value> {
        self.call(700, "_fixture/active", json!({})).await["result"]["sessions"]
            .as_array()
            .unwrap()
            .clone()
    }

    async fn wait_for_adapter_sessions(&mut self, expected: &[Value]) {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if self.active_adapter_sessions().await == expected {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap_or_else(|_| panic!("adapter retained an unwanted session"));
    }

    async fn prompt(&mut self, id: i64, sid: &Value, text: &str) {
        self.send(
            json!({"jsonrpc":"2.0","id":id,"method":"session/prompt","params":{
            "sessionId":sid,"prompt":[{"type":"text","text":text}]}}),
        )
        .await;
    }
}

#[tokio::test]
async fn durable_parent_releases_the_shared_adapter_with_a_fork_permission_pending() {
    let (mut peer, mut control) = Peer::start("shutdown", true, true).await;
    let child = peer.open_fork(3).await;
    peer.prompt(4, &child, "private-turn").await;
    let update = peer.recv().await;
    assert_eq!(update["params"]["sessionId"], child);
    assert_eq!(
        update["params"]["update"]["content"]["text"],
        "private-turn"
    );
    assert_eq!(peer.recv().await["result"]["stopReason"], "end_turn");
    peer.prompt(5, &child, "hold-child").await;
    let permission = peer.recv().await;
    assert_eq!(permission["method"], "session/request_permission");
    assert_eq!(permission["params"]["sessionId"], child);
    assert_eq!(permission["params"]["_meta"][MARK], peer.marker);

    peer.machine
        .send(
            json!({"bindingId":peer.binding,"generation":1,"commandId":"parent-turn","message":{
            "jsonrpc":"2.0","id":6,"method":"session/prompt","params":{
                "sessionId":peer.parent,"prompt":[{"type":"text","text":"parent-answer"}]}}}),
        )
        .await
        .unwrap();
    let parent_update = peer.recv().await;
    assert_eq!(parent_update["params"]["sessionId"], peer.parent);
    assert_eq!(peer.recv().await["result"]["stopReason"], "end_turn");
    let scope = json!({"bindingId":peer.binding,"generation":1});
    let replay = control
        .request("_nori/host/replay", scope.clone())
        .await
        .unwrap();
    let records = replay["records"].as_array().unwrap();
    assert!(records.iter().any(|r| r["message"] == parent_update));
    assert!(records.iter().all(|r| r["sessionId"] == peer.parent));
    let checkpoint = control
        .request("_nori/host/checkpoint", scope)
        .await
        .unwrap();
    assert_eq!(checkpoint["checkpoint"]["mainSessionId"], peer.parent);

    let witness = fs::File::open(peer.cwd.join("adapter.lock")).unwrap();
    assert!(matches!(
        witness.try_lock(),
        Err(fs::TryLockError::WouldBlock)
    ));
    let release = tokio::time::timeout(
        Duration::from_secs(10),
        control.request(
            "_nori/host/release",
            json!({"bindingId":peer.binding,"generation":1,"commandId":"release-parent"}),
        ),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(release["checkpoint"]["mainSessionId"], peer.parent);
    assert!(
        witness.try_lock().is_ok(),
        "shared adapter still holds its lock at release acknowledgment"
    );
}

#[tokio::test]
async fn two_forks_share_the_busy_agent_and_keep_their_traffic_private() {
    let mut peer = Peer::new("iris", true).await;
    let parent = peer.parent.clone();
    peer.prompt(3, &parent, "parent-seed").await;
    assert_eq!(peer.recv().await["params"]["sessionId"], parent);
    let pid = peer.recv().await["result"]["_meta"]["pid"].clone();
    assert!(pid.is_number());
    peer.prompt(4, &parent, "hold-parent").await;
    let parent_permission = peer.recv().await;
    assert_eq!(parent_permission["params"]["sessionId"], parent);
    peer.send(peer.fork(5)).await;
    peer.send(peer.fork(6)).await;
    let first_reply = peer.recv().await;
    let second_reply = peer.recv().await;
    for opened in [&first_reply, &second_reply] {
        assert_eq!(opened["result"]["_meta"][MARK], peer.marker, "{opened}");
        assert_eq!(opened["result"]["modes"]["currentModeId"], "inspect");
    }
    let mut ids = [
        first_reply["id"].as_i64().unwrap(),
        second_reply["id"].as_i64().unwrap(),
    ];
    ids.sort();
    assert_eq!(ids, [5, 6]);
    let first_side = first_reply["result"]["sessionId"].clone();
    let second_side = second_reply["result"]["sessionId"].clone();
    assert_ne!(first_side, second_side);
    assert_ne!(first_side, parent);
    assert_ne!(second_side, parent);

    let mode_change = peer
        .call(
            20,
            "session/set_mode",
            json!({"sessionId":first_side,"modeId":"edit"}),
        )
        .await;
    assert!(mode_change["error"].is_object(), "{mode_change}");
    for (id, sid, word) in [
        (7, &first_side, "write"),
        (8, &second_side, "private-answer"),
    ] {
        peer.prompt(id, sid, word).await;
        let event = peer.recv().await;
        assert_eq!(event["params"]["sessionId"], *sid);
        assert_eq!(event["params"]["_meta"][MARK], peer.marker);
        assert_eq!(
            event["params"]["update"]["content"]["text"],
            format!("parent-seed|{word}")
        );
        let done = peer.recv().await;
        assert_eq!(done["result"]["_meta"]["pid"], pid);
        assert_eq!(done["result"]["_meta"]["mode"], "inspect");
    }
    assert!(!peer.cwd.join("fixture-write").exists());
    peer.prompt(9, &first_side, "hold-a").await;
    let first_permission = peer.recv().await;
    peer.prompt(10, &second_side, "hold-b").await;
    let second_permission = peer.recv().await;
    assert_eq!(first_permission["params"]["sessionId"], first_side);
    assert_eq!(second_permission["params"]["sessionId"], second_side);
    for permission in [&first_permission, &second_permission] {
        assert_eq!(permission["params"]["_meta"][MARK], peer.marker);
    }
    for (request, prompt, word) in [
        (second_permission, 10, "hold-b"),
        (first_permission, 9, "hold-a"),
    ] {
        let decision = json!({"outcome":{"outcome":"selected","optionId":word}});
        peer.send(json!({"jsonrpc":"2.0","id":request["id"],"result":decision}))
            .await;
        let done = peer.recv().await;
        assert_eq!(done["id"], prompt);
        assert_eq!(done["result"]["_meta"]["decision"], decision);
    }
    peer.prompt(21, &first_side, "hold-close").await;
    let closed_side_permission = peer.recv().await;
    assert_eq!(closed_side_permission["params"]["sessionId"], first_side);
    peer.send(
        json!({"jsonrpc":"2.0","id":11,"method":"session/close","params":{"sessionId":first_side}}),
    )
    .await;
    let first = peer.recv().await;
    let second = peer.recv().await;
    assert!(
        [&first, &second]
            .iter()
            .any(|msg| msg["id"] == 11 && msg["result"].is_object())
    );
    assert!([&first, &second].iter().any(|msg| msg["id"] == 21));
    peer.send(
        json!({"jsonrpc":"2.0","id":closed_side_permission["id"],"result":{"outcome":{"outcome":"cancelled"}}}),
    )
    .await;
    // Continue on the original parent request after the child has closed.
    peer.send(json!({"jsonrpc":"2.0","id":parent_permission["id"],"result":{"outcome":{"outcome":"cancelled"}}})).await;
    assert_eq!(peer.recv().await["id"], 4);
    peer.prompt(12, &parent, "parent-finish").await;
    assert_eq!(
        peer.recv().await["params"]["update"]["content"]["text"],
        "parent-seed|parent-finish"
    );
    assert_eq!(peer.recv().await["result"]["_meta"]["mode"], "edit");
    assert_eq!(
        peer.listed_sessions(13).await,
        std::slice::from_ref(&parent)
    );
    assert!(
        peer.call(14, "session/close", json!({"sessionId":parent}))
            .await["result"]
            .is_object()
    );
    peer.wait_for_adapter_sessions(&[]).await;
    peer.prompt(15, &second_side, "late").await;
    assert!(peer.recv().await["error"].is_object());
    let logs = transcripts(&peer.cwd.with_file_name("home"));
    assert_eq!(logs.len(), 1);
    assert!(logs[0].contains("parent-finish"));
    assert!(!logs[0].contains("private-answer"));
}

fn transcripts(root: &Path) -> Vec<String> {
    let mut dirs = vec![root.to_owned()];
    let mut logs = Vec::new();
    while let Some(dir) = dirs.pop() {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                dirs.push(path);
            } else if path.extension().is_some_and(|x| x == "jsonl") {
                logs.push(fs::read_to_string(path).unwrap());
            }
        }
    }
    logs
}

#[tokio::test]
async fn unsupported_or_unconfirmed_forks_offer_rebuilt_and_preserve_parent() {
    for profile in [
        "uncertified",
        "no-fork",
        "no-close",
        "codex",
        "reject-mode",
        "missing-mode",
        "bypass-mode",
        "alias-parent",
    ] {
        let mut peer = Peer::new(profile, true).await;
        peer.send(peer.fork(3)).await;
        let error = peer.recv().await;
        assert_eq!(
            error["error"]["data"]["fallback"], "rebuilt",
            "{profile}: {error}"
        );
        let parent = peer.parent.clone();
        peer.wait_for_adapter_sessions(std::slice::from_ref(&parent))
            .await;
        peer.prompt(4, &parent, "survives").await;
        assert_eq!(peer.recv().await["params"]["sessionId"], parent);
        assert_eq!(peer.recv().await["result"]["_meta"]["mode"], "edit");
    }
}

#[tokio::test]
async fn disabled_side_forks_preserve_ordinary_fork() {
    let mut peer = Peer::new("iris", false).await;
    peer.send(peer.fork(3)).await;
    assert!(peer.recv().await["error"].is_object());
    let result = peer
        .call(
            4,
            "session/fork",
            json!({"cwd":peer.cwd,"sessionId":peer.parent,"mcpServers":[]}),
        )
        .await;
    assert!(result["result"]["sessionId"].is_string());
    assert!(result["result"]["_meta"][MARK].is_null());
    let ordinary = &result["result"]["sessionId"];
    peer.prompt(5, ordinary, "write").await;
    assert_eq!(peer.recv().await["params"]["sessionId"], *ordinary);
    assert_eq!(peer.recv().await["result"]["_meta"]["mode"], "edit");
    assert_eq!(
        fs::read_to_string(peer.cwd.join("fixture-write")).unwrap(),
        "write"
    );
}

#[tokio::test]
async fn private_adapter_ids_and_duplicate_forks_leave_the_live_child_intact() {
    let mut peer = Peer::new("alias-sibling", true).await;
    let side = peer.open_fork(3).await;
    let raw_children: Vec<_> = peer
        .active_adapter_sessions()
        .await
        .into_iter()
        .filter(|id| *id != peer.parent)
        .collect();
    let [raw] = raw_children.as_slice() else {
        panic!("expected one fork: {raw_children:?}");
    };
    assert_ne!(&side, raw);
    let direct = peer
        .call(
            4,
            "session/set_mode",
            json!({"sessionId":raw,"modeId":"edit"}),
        )
        .await;
    assert!(direct["error"].is_object(), "{direct}");
    peer.send(peer.fork(5)).await;
    assert_eq!(peer.recv().await["error"]["data"]["fallback"], "rebuilt");
    peer.prompt(6, &side, "write").await;
    assert_eq!(peer.recv().await["params"]["sessionId"], side);
    assert_eq!(peer.recv().await["result"]["_meta"]["mode"], "inspect");
    assert!(!peer.cwd.join("fixture-write").exists());
}

#[tokio::test]
async fn an_adapter_leaving_read_only_mode_loses_just_that_side() {
    let mut peer = Peer::new("mode-drift", true).await;
    let side = peer.open_fork(3).await;
    let parent = peer.parent.clone();
    peer.prompt(4, &side, "drift").await;
    loop {
        let frame = peer.recv().await;
        if frame["id"] == 4 {
            assert!(frame["error"].is_object(), "{frame}");
            break;
        }
        assert_eq!(frame["params"]["sessionId"], side, "{frame}");
        assert_eq!(frame["params"]["_meta"][MARK], peer.marker, "{frame}");
    }
    peer.prompt(5, &side, "write").await;
    assert!(peer.recv().await["error"].is_object());
    assert!(!peer.cwd.join("fixture-write").exists());
    peer.wait_for_adapter_sessions(std::slice::from_ref(&parent))
        .await;
    peer.prompt(6, &parent, "still-here").await;
    assert_eq!(peer.recv().await["params"]["sessionId"], parent);
    assert_eq!(peer.recv().await["result"]["_meta"]["mode"], "edit");
}

#[tokio::test]
async fn stopping_host_releases_shared_adapter_with_two_outstanding_fork_prompts() {
    let mut peer = Peer::new("shutdown", true).await;
    for (open, prompt) in [(3, 4), (5, 6)] {
        let side = peer.open_fork(open).await;
        peer.prompt(prompt, &side, "hold-shutdown").await;
        let permission = peer.recv().await;
        assert_eq!(permission["method"], "session/request_permission");
        assert_eq!(permission["params"]["sessionId"], side);
    }
    let witness = fs::File::open(peer.cwd.join("adapter.lock")).unwrap();
    assert!(matches!(
        witness.try_lock(),
        Err(std::fs::TryLockError::WouldBlock)
    ));
    success(peer.host.run(&["host", "stop"]));
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match witness.try_lock() {
                Ok(()) => break,
                Err(std::fs::TryLockError::WouldBlock) => {
                    tokio::time::sleep(Duration::from_millis(10)).await
                }
                Err(error) => panic!("could not check adapter lifetime: {error}"),
            }
        }
    })
    .await
    .unwrap_or_else(|_| panic!("adapter process survived host shutdown"));
}

#[tokio::test]
async fn a_fork_reply_after_parent_close_cannot_publish_or_leak_a_child() {
    let mut peer = Peer::new("late-fork", true).await;
    peer.send(peer.fork(3)).await;
    let progress = peer.recv().await;
    assert_eq!(progress["params"]["sessionId"], peer.parent, "{progress}");
    assert_eq!(
        progress["params"]["update"]["content"]["text"],
        "fork started"
    );
    assert_eq!(
        peer.listed_sessions(5).await,
        std::slice::from_ref(&peer.parent)
    );
    peer.send(
        json!({"jsonrpc":"2.0","id":4,"method":"session/close","params":{"sessionId":peer.parent}}),
    )
    .await;

    let mut replies = [peer.recv().await, peer.recv().await];
    assert!(
        replies.iter().all(|frame| frame["id"].is_i64()),
        "unexpected callback: {replies:?}"
    );
    replies.sort_by_key(|frame| frame["id"].as_i64());
    assert_eq!(replies[0]["id"], 3);
    assert!(replies[0]["error"].is_object(), "{:?}", replies[0]);
    assert_eq!(replies[1]["id"], 4);
    assert!(replies[1]["result"].is_object(), "{:?}", replies[1]);
    peer.wait_for_adapter_sessions(&[]).await;
}

#[tokio::test]
async fn declined_fork_close_preserves_the_pending_permission_and_allows_retry() {
    let mut peer = Peer::new("close-retry", true).await;
    let side = peer.open_fork(3).await;
    peer.prompt(4, &side, "hold-retry").await;
    let permission = peer.recv().await;
    assert_eq!(permission["method"], "session/request_permission");
    assert_eq!(permission["params"]["sessionId"], side);
    let close = json!({"sessionId":side});
    peer.send(json!({"jsonrpc":"2.0", "id":5, "method":"session/close", "params":close}))
        .await;
    let blocked = peer
        .call(
            8,
            "session/set_mode",
            json!({"sessionId":side,"modeId":"inspect"}),
        )
        .await;
    assert_eq!(blocked["error"]["code"], -32015);
    let decision = json!({"outcome":{"outcome":"selected","optionId":"hold-retry"}});
    peer.send(json!({"jsonrpc":"2.0","id":permission["id"],"result":decision}))
        .await;
    let finished = peer.recv().await;
    assert_eq!(finished["id"], 4);
    assert_eq!(finished["result"]["_meta"]["decision"], decision);
    let refused = peer.recv().await;
    assert_eq!(refused["id"], 5);
    assert_eq!(refused["error"]["code"], -32031);
    assert_eq!(refused["error"]["data"], json!({"retry":true}));

    assert!(peer.call(6, "session/close", close).await["result"].is_object());
    let parent = peer.parent.clone();
    peer.wait_for_adapter_sessions(std::slice::from_ref(&parent))
        .await;
    peer.prompt(7, &parent, "parent-after-retry").await;
    assert_eq!(peer.recv().await["params"]["sessionId"], parent);
    assert_eq!(peer.recv().await["result"]["_meta"]["mode"], "edit");
}

#[tokio::test]
async fn declined_parent_close_keeps_its_fork_usable_until_the_parent_closes() {
    let mut peer = Peer::new("close-retry", true).await;
    let side = peer.open_fork(3).await;
    let parent = peer.parent.clone();
    peer.prompt(4, &parent, "hold-parent").await;
    let permission = peer.recv().await;
    assert_eq!(permission["params"]["sessionId"], parent);

    let close = json!({"sessionId": parent});
    let refused = peer.call(5, "session/close", close.clone()).await;
    assert_eq!(refused["error"]["code"], -32031);
    peer.prompt(6, &side, "surviving-fork").await;
    let update = peer.recv().await;
    assert_eq!(update["params"]["sessionId"], side, "{update}");
    assert_eq!(peer.recv().await["result"]["_meta"]["mode"], "inspect");

    peer.send(json!({
        "jsonrpc": "2.0", "id": permission["id"],
        "result": {"outcome": {"outcome": "cancelled"}}
    }))
    .await;
    assert_eq!(peer.recv().await["id"], 4);
    assert!(peer.call(7, "session/close", close).await["result"].is_object());
    peer.wait_for_adapter_sessions(&[]).await;
}

#[tokio::test]
async fn close_lifecycle_retries_only_the_refused_child_after_parent_close() {
    let mut peer = Peer::new("close-retry", true).await;
    let parent = peer.parent.clone();
    let other = peer
        .call(3, "session/new", json!({"cwd":peer.cwd,"mcpServers":[]}))
        .await["result"]["sessionId"]
        .clone();
    let child = peer.open_fork(4).await;
    let raw_child = peer
        .active_adapter_sessions()
        .await
        .into_iter()
        .find(|sid| *sid != parent && *sid != other)
        .unwrap();
    let sibling = peer.open_fork(5).await;
    peer.prompt(6, &child, "hold-inspect").await;
    let permission = peer.recv().await;
    assert_eq!(permission["params"]["sessionId"], child);

    let close = json!({"sessionId":parent});
    for id in [7, 8] {
        peer.send(json!({"jsonrpc":"2.0","id":id,"method":"session/close","params":close}))
            .await;
        let mut early = Vec::new();
        let refused = peer.recv_until(id, &mut early).await;
        // Inspect the adapter immediately after the acknowledgment, even if the
        // host has already terminated the pending prompt.
        peer.send(json!({"jsonrpc":"2.0","id":700,"method":"_fixture/active","params":{}}))
            .await;
        let inventory = peer.recv_until(700, &mut early).await;
        assert!(
            early.is_empty(),
            "premature child frames {early:?}; close {refused}; adapter {inventory}"
        );
        assert_eq!(
            refused["error"]["data"]["parentClosed"], true,
            "{refused}, {inventory}"
        );
        assert_eq!(refused["error"]["data"]["retry"], true);
        assert_eq!(inventory["result"]["sessions"], json!([other, raw_child]));
        assert_eq!(
            inventory["result"]["closeAttempts"][parent.as_str().unwrap()],
            1
        );
    }
    peer.prompt(9, &sibling, "ended").await;
    assert_eq!(peer.recv().await["error"]["code"], -32002);
    peer.send(peer.fork(10)).await;
    assert!(peer.recv().await["error"].is_object());
    let rebuilt = peer
        .call(
            15,
            "session/new",
            json!({"cwd":peer.cwd,"mcpServers":[],"_meta":{(MARK):peer.marker}}),
        )
        .await;
    assert_eq!(rebuilt["error"]["code"], -32602, "{rebuilt}");
    assert_eq!(peer.listed_sessions(11).await, std::slice::from_ref(&other));

    let decision = json!({"outcome":{"outcome":"selected","optionId":"hold-inspect"}});
    peer.send(json!({"jsonrpc":"2.0","id":permission["id"],"result":decision}))
        .await;
    let done = peer.recv().await;
    assert_eq!(done["id"], 6);
    assert_eq!(done["result"]["_meta"]["decision"], decision);
    assert_eq!(
        peer.call(12, "session/close", close).await["result"],
        json!({})
    );
    assert_eq!(
        peer.active_adapter_sessions().await,
        std::slice::from_ref(&other)
    );
    peer.prompt(13, &child, "ended").await;
    assert_eq!(peer.recv().await["error"]["code"], -32002);
    peer.prompt(14, &other, "still-here").await;
    assert_eq!(
        peer.recv().await["params"]["update"]["content"]["text"],
        "still-here"
    );
    assert_eq!(peer.recv().await["result"]["_meta"]["mode"], "edit");
}

#[tokio::test]
async fn close_lifecycle_delivers_child_callbacks_through_refusal_and_stops_at_confirmation() {
    let mut peer = Peer::new("stream-close", true).await;
    let parent = peer.parent.clone();
    let child = peer.open_fork(3).await;
    let sibling = peer.open_fork(4).await;
    peer.prompt(5, &sibling, "hold-witness").await;
    let witness = peer.recv().await;
    assert_eq!(witness["params"]["sessionId"], sibling);

    peer.prompt(6, &child, "stream-close").await;
    assert_eq!(
        peer.recv().await["params"]["update"]["content"]["text"],
        "first"
    );
    let close = json!({"sessionId":child});
    peer.send(json!({"jsonrpc":"2.0","id":7,"method":"session/close","params":close}))
        .await;
    let last = peer.recv().await;
    assert_eq!(last["params"]["update"]["content"]["text"], "lilac");
    assert_eq!(last["params"]["sessionId"], child);
    assert_eq!(last["params"]["_meta"][MARK], peer.marker);
    peer.answer_child_permission(&child, "read-finish").await;
    let refused = peer.recv().await;
    assert_eq!(refused["id"], 7);
    assert_eq!(refused["error"]["code"], -32031);
    let resumed = peer.recv().await;
    assert_eq!(resumed["params"]["sessionId"], child);
    assert_eq!(resumed["params"]["_meta"][MARK], peer.marker);
    assert_eq!(
        resumed["params"]["update"]["content"]["text"],
        "read-finish"
    );
    let decision = peer.answer_child_permission(&child, "read-after").await;
    let done = peer.recv().await;
    assert_eq!(done["id"], 6);
    assert_eq!(done["result"]["_meta"]["decision"], decision);

    assert_eq!(
        peer.call(9, "session/close", close).await["result"],
        json!({})
    );
    assert_eq!(peer.active_adapter_sessions().await.len(), 2);
    let decision = json!({"outcome":{"outcome":"cancelled"}});
    peer.send(json!({"jsonrpc":"2.0","id":witness["id"],"result":decision}))
        .await;
    let done = peer.recv().await;
    assert_eq!(done["id"], 5);
    assert_eq!(done["result"]["_meta"]["decision"], decision);
    peer.prompt(10, &sibling, "sibling-answer").await;
    assert_eq!(
        peer.recv().await["params"]["update"]["content"]["text"],
        "sibling-answer"
    );
    assert_eq!(peer.recv().await["result"]["_meta"]["mode"], "inspect");
    assert!(
        peer.call(11, "session/close", json!({"sessionId":parent}))
            .await["result"]
            .is_object()
    );
    assert!(peer.active_adapter_sessions().await.is_empty());
}

#[tokio::test]
async fn parent_close_can_retry_an_unanswered_adapter_close() {
    let mut peer = Peer::new("silent-close", true).await;
    let parent = peer.parent.clone();
    peer.open_fork(3).await;
    let other = peer
        .call(4, "session/new", json!({"cwd":peer.cwd,"mcpServers":[]}))
        .await["result"]["sessionId"]
        .clone();
    let request = |id| json!({"jsonrpc":"2.0","id":id,"method":"session/close","params":{"sessionId":parent}});
    peer.send(request(5)).await;
    let reopen = peer
        .call(
            8,
            "session/load",
            json!({"sessionId":parent,"cwd":peer.cwd,"mcpServers":[]}),
        )
        .await;
    assert!(
        matches!(reopen["error"]["code"].as_i64(), Some(-32015 | -32002)),
        "{reopen}"
    );
    let expired = peer.recv_within(Duration::from_secs(20)).await;
    assert_eq!(expired["id"], 5);
    assert_eq!(
        expired["error"]["data"],
        json!({"parentClosed":true,"retry":true})
    );
    peer.send(request(6)).await;
    let confirmed = peer.recv().await;
    assert_eq!(confirmed["id"], 6);
    assert_eq!(confirmed["result"], json!({}));
    let inventory = peer.call(7, "_fixture/active", json!({})).await;
    assert_eq!(inventory["result"]["sessions"], json!([other]));
    assert_eq!(
        inventory["result"]["closeAttempts"][parent.as_str().unwrap()],
        1
    );
}
