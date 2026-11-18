use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use acp::schema::ProtocolVersion;
use acp::schema::v1::*;
use agent_client_protocol as acp;
use futures::SinkExt;
use futures::StreamExt;
use futures::channel::mpsc;
use nori_harness::remote_agent::ManagedClient;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use tokio::sync::Barrier;

use super::Host;
use super::success;

const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/standard_acp/fixture.py");
const BINDINGS: [&str; 2] = ["orchid", "aster"];

#[derive(Clone, Default)]
struct Frames(Arc<Mutex<Vec<Value>>>);

impl Frames {
    fn record(&self, mut message: Value) {
        // SDK-generated request IDs differ on each run. Correlation itself is
        // exercised by simultaneous prompts with different permission outcomes.
        message.as_object_mut().unwrap().remove("id");
        self.0.lock().unwrap().push(message);
    }

    fn observed(&self) -> Vec<Value> {
        self.0.lock().unwrap().clone()
    }
}

async fn exercise(
    transport: impl acp::ConnectTo<acp::Client> + 'static,
    name: &'static str,
    cwd: &Path,
    barrier: Arc<Barrier>,
    check_unsupported: bool,
) {
    let (permissions, mut incoming) = tokio::sync::mpsc::unbounded_channel();
    let scenario = acp::Client
        .builder()
        .on_receive_notification(
            async |_: SessionNotification, _| Ok(()),
            acp::on_receive_notification!(),
        )
        .on_receive_request(
            async move |req: RequestPermissionRequest, responder, _| {
                permissions.send((req, responder)).unwrap();
                Ok(())
            },
            acp::on_receive_request!(),
        )
        .connect_with(transport, async move |cx| {
            let init = cx
                .send_request(
                    InitializeRequest::new(ProtocolVersion::V1)
                        .client_info(Implementation::new("fixture-client", "1")),
                )
                .block_task()
                .await?;
            assert_eq!(init.agent_info.unwrap().name, name);
            let new = || {
                NewSessionRequest::new(cwd).mcp_servers(vec![McpServer::Stdio(
                    McpServerStdio::new("client-owned", "/synthetic/tool"),
                )])
            };
            let first = cx.send_request(new()).block_task().await?.session_id;
            assert_eq!(first.to_string(), "0");
            let ask = |sid: &SessionId, text: &str| {
                cx.send_request(
                    PromptRequest::new(sid.clone(), vec![text.to_owned().into()])
                        .meta(Meta::from_iter([("fixture.tag".into(), json!(text))])),
                )
            };
            let a = ask(&first, "alpha");
            let (permission_a, answer_a) = incoming.recv().await.unwrap();
            assert_eq!(permission_a.session_id, first);
            assert_eq!(permission_a.tool_call.fields.title.as_deref(), Some(name));

            let second = if name == "orchid" {
                cx.send_request(ForkSessionRequest::new(first.clone(), cwd))
                    .block_task()
                    .await?
                    .session_id
            } else {
                cx.send_request(new()).block_task().await?.session_id
            };
            assert_eq!(second.to_string(), "1");
            let b = ask(&second, "beta");
            let (permission_b, answer_b) = incoming.recv().await.unwrap();
            assert_eq!(permission_b.session_id, second);
            barrier.wait().await;
            answer_b.respond(RequestPermissionResponse::new(
                RequestPermissionOutcome::Cancelled,
            ))?;
            let done = b.block_task().await?;
            assert_eq!(
                done.meta.unwrap()["decision"],
                json!({"outcome":"cancelled"})
            );
            let option = format!("allow-{name}");
            answer_a.respond(RequestPermissionResponse::new(
                RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(option.clone())),
            ))?;
            let done = a.block_task().await?;
            assert_eq!(
                done.meta.unwrap()["decision"],
                json!({"outcome":"selected","optionId":option})
            );

            let cancel_me = ask(&second, "cancel me");
            let (permission, pending) = incoming.recv().await.unwrap();
            assert_eq!(permission.session_id, second);
            cx.send_notification(CancelNotification::new(second.clone()))?;
            assert_eq!(
                cancel_me.block_task().await?.stop_reason,
                StopReason::Cancelled
            );
            pending.respond(RequestPermissionResponse::new(
                RequestPermissionOutcome::Cancelled,
            ))?;
            cx.send_request(CloseSessionRequest::new(second))
                .block_task()
                .await?;
            cx.send_request(SetSessionModeRequest::new(first.clone(), "explore"))
                .block_task()
                .await?;
            let error = cx
                .send_request(SetSessionModeRequest::new(first.clone(), "missing"))
                .block_task()
                .await
                .unwrap_err();
            assert_eq!(
                error.data,
                Some(json!({"requested":"missing","agent":name}))
            );
            cx.send_request(SetSessionConfigOptionRequest::new(
                first.clone(),
                "speed",
                "fast",
            ))
            .block_task()
            .await?;
            assert_eq!(
                ask(&first, "finish").block_task().await?.stop_reason,
                StopReason::EndTurn
            );
            cx.send_request(CloseSessionRequest::new(first.clone()))
                .block_task()
                .await?;
            cx.send_request(LoadSessionRequest::new(first.clone(), cwd))
                .block_task()
                .await?;
            cx.send_request(CloseSessionRequest::new(first.clone()))
                .block_task()
                .await?;
            cx.send_request(ResumeSessionRequest::new(first.clone(), cwd))
                .block_task()
                .await?;
            if check_unsupported && name == "aster" {
                let error = cx
                    .send_request(ForkSessionRequest::new(first.clone(), cwd))
                    .block_task()
                    .await
                    .unwrap_err();
                assert_eq!(error.code, ErrorCode::MethodNotFound);
            }
            cx.send_request(CloseSessionRequest::new(first))
                .block_task()
                .await?;
            if name == "orchid" {
                let listed = cx
                    .send_request(ListSessionsRequest::new())
                    .block_task()
                    .await?;
                let [saved] = listed.sessions.as_slice() else {
                    panic!("expected one saved session");
                };
                assert_eq!(saved.cwd, cwd);
                let child = cx
                    .send_request(ForkSessionRequest::new(saved.session_id.clone(), cwd))
                    .block_task()
                    .await?;
                assert_eq!(child.meta.unwrap()["parent"], saved.session_id.to_string());
            }
            Ok(())
        });
    tokio::time::timeout(Duration::from_secs(12), scenario)
        .await
        .unwrap()
        .unwrap();
}

async fn direct(name: &'static str, cwd: &Path, barrier: Arc<Barrier>) -> Vec<Value> {
    let frames = Frames::default();
    let capture = frames.clone();
    let agent = acp::AcpAgent::new(McpServer::Stdio(McpServerStdio::new(name, "python3").args(
        vec![FIXTURE.into(), name.into(), cwd.to_str().unwrap().into()],
    )))
    .with_debug(move |line, direction| {
        if direction == acp::LineDirection::Stdout {
            capture.record(serde_json::from_str(line).unwrap());
        }
    });
    exercise(agent, name, cwd, barrier, false).await;
    frames.observed()
}

type Outgoing = mpsc::UnboundedSender<Value>;
type Feed = mpsc::UnboundedSender<String>;

fn scope_transport(
    name: &'static str,
    outgoing: Outgoing,
) -> (Feed, impl acp::ConnectTo<acp::Client>) {
    let (feed, incoming) = mpsc::unbounded::<String>();
    let transport = acp::Lines::new(
        outgoing
            .sink_map_err(io::Error::other)
            .with(move |line: String| async move {
                let message: Value = serde_json::from_str(&line).unwrap();
                Ok::<_, io::Error>(json!({"bindingId":name,"generation":1,"message":message}))
            }),
        incoming.map(Ok),
    );
    (feed, transport)
}

async fn multiplexer(
    mut client: ManagedClient,
    mut outgoing: mpsc::UnboundedReceiver<Value>,
    mut feeds: HashMap<&'static str, (Feed, Frames)>,
) -> anyhow::Result<()> {
    loop {
        tokio::select! {
            msg = outgoing.next() => {
                let Some(msg) = msg else { return Ok(()); };
                client.send(msg).await?;
            }
            frame = client.recv() => {
                let frame = frame?;
                assert_eq!(frame["generation"], 1);
                let binding = frame["bindingId"].as_str().unwrap();
                if frame["closed"] == true {
                    feeds.remove(binding);
                    continue;
                }
                let (feed, frames) = &feeds[binding];
                frames.record(frame["message"].clone());
                feed.unbounded_send(frame["message"].to_string())?;
            }
        }
    }
}

fn start(managed: bool) -> (Host, String, String) {
    let host = Host::new();
    fs::create_dir(host.0.path().join("hosted-sessions")).unwrap();
    fs::write(
        host.0.path().join("config.toml"),
        format!("[host]\nmanaged = {managed}\n"),
    )
    .unwrap();
    success(host.run(&["host", "--port", "0"]));
    let state: Value =
        serde_json::from_slice(&fs::read(host.0.path().join("host/state.json")).unwrap()).unwrap();
    let token = fs::read_to_string(host.0.path().join("host/token")).unwrap();
    (
        host,
        state["endpoint"]
            .as_str()
            .unwrap()
            .replace("/sessions", "/managed"),
        token,
    )
}

async fn admit(
    client: &mut ManagedClient,
    host: &Host,
    name: &str,
    additions: [bool; 4],
) -> PathBuf {
    let [echo, status, recording, mcp] = additions;
    let root = host.0.path().join("hosted-sessions").join(name);
    let cwd = root.join("workspace");
    let home = root.join("home");
    fs::create_dir_all(&cwd).unwrap();
    fs::create_dir_all(&home).unwrap();
    client.request("_nori/session/ensure", json!({
        "bindingId":name, "generation":1, "commandId":format!("launch-{name}"),
        "conversationId":uuid::Uuid::new_v4(), "spec":{
            "projectId":"fixture-project", "runId":format!("run-{name}"),
            "agent":name, "model":name, "cwd":cwd, "home":home,
            "adapter":{"path":which::which("python3").unwrap(),"version":"fixture-1","args":[FIXTURE,name]},
            "additions":{"promptEcho":echo,"statusEvents":status,"recording":recording,"clientMcp":mcp}
        }
    })).await.unwrap();
    cwd
}

#[tokio::test]
async fn scoped_clients_preserve_agent_sessions_and_permissions() {
    let (host, endpoint, token) = start(true);
    let mut control = ManagedClient::connect(&endpoint.replace("/managed", "/sessions"), &token)
        .await
        .unwrap();
    let discovery = control.request("initialize", json!({})).await.unwrap();
    assert_eq!(
        discovery["agentCapabilities"]["_meta"]["nori.dev/managedHost"]["standardAcp"],
        1
    );
    let machine = ManagedClient::connect(&endpoint, &token).await.unwrap();
    let orchid_cwd = admit(&mut control, &host, BINDINGS[0], [false; 4]).await;
    let aster_cwd = admit(&mut control, &host, BINDINGS[1], [false; 4]).await;
    let barrier = Arc::new(Barrier::new(2));
    let (first, second) = tokio::join!(
        direct(BINDINGS[0], &orchid_cwd, barrier.clone()),
        direct(BINDINGS[1], &aster_cwd, barrier)
    );
    assert_eq!(first[1]["result"]["models"]["currentModelId"], "orchid");

    let (send, recv) = mpsc::unbounded();
    let (left, orchid) = scope_transport(BINDINGS[0], send.clone());
    let (right, aster) = scope_transport(BINDINGS[1], send);
    let captures = [Frames::default(), Frames::default()];
    let feeds = HashMap::from([
        (BINDINGS[0], (left, captures[0].clone())),
        (BINDINGS[1], (right, captures[1].clone())),
    ]);
    let pump = tokio::spawn(multiplexer(machine, recv, feeds));
    let barrier = Arc::new(Barrier::new(2));
    tokio::join!(
        exercise(orchid, BINDINGS[0], &orchid_cwd, barrier.clone(), true),
        exercise(aster, BINDINGS[1], &aster_cwd, barrier, true),
    );
    pump.await.unwrap().unwrap();

    assert_eq!(captures[0].observed(), first);
    let mut second_observed = captures[1].observed();
    second_observed.retain(|frame| frame["error"]["code"] != -32601);
    assert_eq!(second_observed, second);
    for cwd in [orchid_cwd, aster_cwd] {
        assert!(
            !cwd.with_file_name("home")
                .join(".nori/cli/transcripts")
                .exists()
        );
    }
    assert!(!host.0.path().join("transcripts").exists());
}

#[tokio::test]
async fn local_host_keeps_managed_socket_disabled() {
    let (host, endpoint, token) = start(false);
    assert!(ManagedClient::connect(&endpoint, &token).await.is_err());
    assert!(success(host.run(&["host", "ls"])).contains("STATUS"));
}

fn contains_transcript(directory: &Path) -> bool {
    directory
        .read_dir()
        .into_iter()
        .flatten()
        .flatten()
        .any(|entry| {
            let path = entry.path();
            if path.is_dir() {
                contains_transcript(&path)
            } else {
                path.extension().is_some_and(|ext| ext == "jsonl")
                    && fs::read_to_string(path).unwrap().contains("finish")
            }
        })
}

#[tokio::test]
async fn managed_switches_control_host_additions() {
    for enabled in 0..4 {
        let (host, endpoint, token) = start(true);
        let mut machine = ManagedClient::connect(&endpoint, &token).await.unwrap();
        let mut control =
            ManagedClient::connect(&endpoint.replace("/managed", "/sessions"), &token)
                .await
                .unwrap();
        control.request("initialize", json!({})).await.unwrap();
        let additions = std::array::from_fn(|i| i == enabled);
        let cwd = admit(&mut control, &host, "aster", additions).await;

        let mut updates = Vec::new();
        let mut injected = false;
        let calls = [
            (
                "initialize",
                json!({"protocolVersion":1,"clientCapabilities":{}}),
            ),
            ("session/new", json!({"cwd":cwd,"mcpServers":[]})),
            (
                "session/prompt",
                json!({"sessionId":"0","prompt":[{"type":"text","text":"finish"}]}),
            ),
            ("session/close", json!({"sessionId":"0"})),
        ];
        for (id, (method, params)) in calls.into_iter().enumerate() {
            machine
                .send(json!({"bindingId":"aster","generation":1,"message":{
                    "jsonrpc":"2.0", "id":id, "method":method, "params":params
                }}))
                .await
                .unwrap();
            loop {
                let frame = tokio::time::timeout(Duration::from_secs(5), machine.recv())
                    .await
                    .unwrap()
                    .unwrap();
                let msg = &frame["message"];
                if msg["method"] == "session/update" {
                    updates.push(msg["params"]["update"].clone());
                    continue;
                }
                assert_eq!(msg["id"], id);
                assert!(msg.get("result").is_some(), "{frame}");
                if method == "session/new" {
                    injected = msg["result"]["_meta"]["mcpServers"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|s| s["name"] == "nori-client");
                }
                break;
            }
        }
        let echo = updates.iter().any(|update| {
            update["sessionUpdate"] == "user_message_chunk"
                && update["_meta"]["nori.dev/promptEchoId"].is_string()
        });
        let status = updates
            .iter()
            .filter_map(|update| update["_meta"]["nori"]["status"].as_str())
            .collect::<Vec<_>>();
        assert_eq!(echo, additions[0], "{updates:?}");
        assert_eq!(
            status,
            if additions[1] {
                vec!["working", "idle"]
            } else {
                vec![]
            }
        );
        assert_eq!(
            contains_transcript(&cwd.with_file_name("home").join(".nori/cli/transcripts")),
            additions[2]
        );
        assert_eq!(injected, additions[3]);
    }
}

async fn exchange(machine: &mut ManagedClient, message: Value) -> Value {
    machine
        .send(json!({"bindingId":"aster","generation":1,"message":message}))
        .await
        .unwrap();
    let frame = tokio::time::timeout(Duration::from_secs(5), machine.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(frame["bindingId"], "aster");
    assert_eq!(frame["generation"], 1);
    frame["message"].clone()
}

#[tokio::test]
async fn agent_can_accept_initialize_after_rejection_and_decides_close_retries() {
    let (host, endpoint, token) = start(true);
    let mut control = ManagedClient::connect(&endpoint.replace("/managed", "/sessions"), &token)
        .await
        .unwrap();
    control.request("initialize", json!({})).await.unwrap();
    let cwd = admit(&mut control, &host, "aster", [false; 4]).await;
    let mut machine = ManagedClient::connect(&endpoint, &token).await.unwrap();
    let request = |id, method: &str, params: Value| json!({"jsonrpc":"2.0","id":id,"method":method,"params":params});

    assert_eq!(
        exchange(
            &mut machine,
            request(
                1,
                "initialize",
                json!({"protocolVersion":1,"_meta":{"fixture.rejectInitialize":true}})
            )
        )
        .await,
        json!({"jsonrpc":"2.0","id":1,"error":{"code":-32603,"message":"fixture unavailable","data":{"retry":true}}})
    );
    let initialized = exchange(
        &mut machine,
        request(2, "initialize", json!({"protocolVersion":1})),
    )
    .await;
    assert_eq!(initialized["result"]["agentInfo"]["name"], "aster");
    let opened = exchange(
        &mut machine,
        request(3, "session/new", json!({"cwd":cwd,"mcpServers":[]})),
    )
    .await;
    let sid = opened["result"]["sessionId"].as_str().unwrap();
    let close = |id| request(id, "session/close", json!({"sessionId":sid}));
    assert_eq!(
        exchange(&mut machine, close(4)).await["result"]["_meta"]["closed"],
        sid
    );
    assert_eq!(
        exchange(&mut machine, close(5)).await,
        json!({"jsonrpc":"2.0","id":5,"error":{"code":-32002,"message":"unknown session","data":{"sid":sid}}})
    );
}

#[tokio::test]
async fn malformed_machine_envelopes_leave_an_open_agent_usable() {
    let (host, endpoint, token) = start(true);
    let mut control = ManagedClient::connect(&endpoint.replace("/managed", "/sessions"), &token)
        .await
        .unwrap();
    control.request("initialize", json!({})).await.unwrap();
    let cwd = admit(&mut control, &host, "aster", [false; 4]).await;

    let mut machine = ManagedClient::connect(&endpoint, &token).await.unwrap();
    let scoped = |message: Value| json!({"bindingId":"aster","generation":1,"message":message});
    tokio::time::timeout(Duration::from_secs(5), async {
        machine
            .send(scoped(json!({
                "jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":1}
            })))
            .await
            .unwrap();
        assert_eq!(
            machine.recv().await.unwrap()["message"]["result"]["agentInfo"]["name"],
            "aster"
        );

        let incomplete = json!({"jsonrpc":"2.0","id":"incomplete","params":{}});
        let error = exchange(&mut machine, incomplete).await;
        assert_eq!(error["id"], "incomplete");
        assert_eq!(error["error"]["code"], -32600);

        machine.send(json!({"bindingId":"unused","generation":1,"message":{
            "jsonrpc":"2.0","id":17,"params":{}
        }})).await.unwrap();
        let unopened = machine.recv().await.unwrap();
        assert_eq!(unopened["bindingId"], "unused");
        assert_eq!(unopened["generation"], 1);
        assert_eq!(unopened["message"]["id"], 17);
        assert_eq!(unopened["message"]["error"]["code"], -32600);

        machine.send(json!({"message":{}})).await.unwrap();
        let error = machine.recv().await.unwrap();
        assert_eq!(error["error"]["code"], -32600);
        assert_eq!(error.get("id"), Some(&Value::Null));

        machine
            .send(json!({"bindingId":"synthetic-other","generation":2}))
            .await
            .unwrap();
        let error = machine.recv().await.unwrap();
        assert_eq!(error["bindingId"], "synthetic-other");
        assert_eq!(error["generation"], 2);
        assert_eq!(error["message"]["error"]["code"], -32600);

        machine.send(json!({
            "bindingId":"aster","generation":2,
            "message":{"jsonrpc":"2.0","id":"stale","method":"initialize","params":{"protocolVersion":1}}
        })).await.unwrap();
        let error = machine.recv().await.unwrap();
        assert_eq!(error["message"]["id"], "stale");
        assert_eq!(error["message"]["error"]["code"], -32001);
        assert_eq!(machine.recv().await.unwrap(), json!({"bindingId":"aster","generation":2,"closed":true}));

        machine
            .send(scoped(json!({
                "jsonrpc":"2.0","id":2,"method":"session/new","params":{"cwd":cwd,"mcpServers":[]}
            })))
            .await
            .unwrap();
        assert_eq!(
            machine.recv().await.unwrap()["message"]["result"]["sessionId"],
            "0"
        );
    })
    .await
    .unwrap();
}
