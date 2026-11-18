use super::*;
use pretty_assertions::assert_eq;

const MARK: &str = "nori.dev/sideSession";

fn request(id: i64, method: &str, params: Value) -> Value {
    json!({"jsonrpc":"2.0","id":id,"method":method,"params":params})
}

fn parent_mark(parent: &str) -> Value {
    json!({"bindingId":"aster","generation":1,"sessionId":parent})
}

fn open_side(id: i64, cwd: &Path, mark: Value) -> Value {
    request(
        id,
        "session/new",
        json!({"cwd":cwd,"mcpServers":[],"_meta":{(MARK):mark}}),
    )
}

#[tokio::test]
async fn disabled_host_refuses_marked_opens_and_side_restoration_but_preserves_regular_sessions() {
    let (host, old_side) = if read_only_launch_available() {
        let mut previous = SideHost::start(false, None).await;
        let id = side_id(&previous.start_side(4, "0").await);
        previous.close(5, &id).await;
        let SideHost {
            _host: host,
            machine,
            ..
        } = previous;
        drop(machine);
        success(host.run(&["host", "stop"]));
        (host, id)
    } else {
        (Host::new(), format!("nori-side-{}", uuid::Uuid::new_v4()))
    };
    let (host, endpoint, token) = start_config(host, "[host]\nmanaged=true\n".into());
    let mut control = ManagedClient::connect(&endpoint.replace("/managed", "/sessions"), &token)
        .await
        .unwrap();
    control.request("initialize", json!({})).await.unwrap();
    let cwd = admit(&mut control, &host, "aster", [false; 4]).await;
    let mut machine = ManagedClient::connect(&endpoint, &token).await.unwrap();
    let init = exchange(
        &mut machine,
        request(1, "initialize", json!({"protocolVersion":1})),
    )
    .await;
    assert!(init["result"].is_object(), "{init}");
    let regular = |id| request(id, "session/new", json!({"cwd":cwd,"mcpServers":[]}));
    let first = exchange(&mut machine, regular(2)).await;
    assert_eq!(first["result"]["sessionId"], "0");
    let refused = exchange(&mut machine, open_side(3, &cwd, parent_mark("0"))).await;
    assert!(refused["error"].is_object(), "{refused}");
    let next = exchange(&mut machine, regular(4)).await;
    assert_eq!(next["result"]["sessionId"], "1");
    let load = |method: &str, id, sid: &str| {
        request(
            id,
            method,
            json!({"sessionId":sid,"cwd":cwd,"mcpServers":[]}),
        )
    };
    for (method, id) in [("session/load", 5), ("session/resume", 6)] {
        let denied = exchange(&mut machine, load(method, id, &old_side)).await;
        assert!(denied["error"].is_object(), "{method}: {denied}");
    }
    let resumed = exchange(&mut machine, load("session/resume", 7, "saved-1")).await;
    assert!(resumed["result"].is_object(), "{resumed}");
}

struct SideHost {
    _host: Host,
    machine: ManagedClient,
    pending: Vec<Value>,
    cwd: PathBuf,
}

impl SideHost {
    async fn start(dedicated: bool, limit: Option<usize>) -> Self {
        Self::with_modes(dedicated, limit, "bypassPermissions plan").await
    }

    async fn with_modes(dedicated: bool, limit: Option<usize>, modes: &str) -> Self {
        let host = Host::new();
        let root = host.0.path().join("hosted-sessions/aster");
        let cwd = root.join("workspace");
        let home = root.join("home");
        fs::create_dir_all(&cwd).unwrap();
        fs::create_dir_all(&home).unwrap();
        fs::write(cwd.join("fixture-modes"), modes).unwrap();
        let mut config = String::from("[host]\nmanaged=true\nside_sessions=true\n");
        if let Some(limit) = limit {
            config.push_str(&format!("max_sessions={limit}\n"));
        }
        if dedicated {
            config.push_str(&format!(
                "[host.dedicated_roots]\nbinding_id='aster'\ncwd='{}'\nhome='{}'\n",
                cwd.display(),
                home.display()
            ));
        }
        let (host, endpoint, token) = start_config(host, config);
        let mut control =
            ManagedClient::connect(&endpoint.replace("/managed", "/sessions"), &token)
                .await
                .unwrap();
        let caps = control.request("initialize", json!({})).await.unwrap();
        let caps = &caps["agentCapabilities"]["_meta"]["nori.dev/managedHost"];
        assert_eq!(caps["rebuiltSideSessions"], 1);
        assert_eq!(caps["maxSessions"], limit.unwrap_or(8));
        admit(&mut control, &host, "aster", [false, false, true, false]).await;
        let mut scenario = Self {
            machine: ManagedClient::connect(&endpoint, &token).await.unwrap(),
            _host: host,
            cwd,
            pending: Vec::new(),
        };
        scenario
            .send(request(1, "initialize", json!({"protocolVersion":1})))
            .await;
        assert_eq!(
            scenario.reply(1).await["result"]["agentInfo"]["name"],
            "aster"
        );
        scenario
            .send(request(
                2,
                "session/new",
                json!({"cwd":scenario.cwd,"mcpServers":[]}),
            ))
            .await;
        assert_eq!(scenario.reply(2).await["result"]["sessionId"], "0");
        scenario.prompt(3, "0", "finish").await;
        assert_eq!(scenario.reply(3).await["result"]["stopReason"], "end_turn");
        scenario
    }

    async fn send(&mut self, message: Value) {
        self.machine
            .send(json!({"bindingId":"aster","generation":1,"message":message}))
            .await
            .unwrap();
    }

    async fn matching(&mut self, predicate: impl Fn(&Value) -> bool) -> Value {
        if let Some(index) = self.pending.iter().position(&predicate) {
            return self.pending.remove(index);
        }
        tokio::time::timeout(Duration::from_secs(8), async {
            loop {
                let frame = self.machine.recv().await.unwrap();
                assert_eq!(frame["bindingId"], "aster", "{frame}");
                assert!(frame.get("message").is_some(), "{frame}");
                let message = frame["message"].clone();
                if predicate(&message) {
                    return message;
                }
                self.pending.push(message);
            }
        })
        .await
        .unwrap()
    }

    async fn reply(&mut self, id: i64) -> Value {
        self.matching(|m| m["id"] == id && m.get("method").is_none())
            .await
    }

    async fn prompt(&mut self, id: i64, sid: &str, text: &str) {
        self.send(request(
            id,
            "session/prompt",
            json!({"sessionId":sid,"prompt":[{"type":"text","text":text}]}),
        ))
        .await;
    }

    async fn permission(&mut self, sid: &str) -> Value {
        self.matching(|m| {
            m["method"] == "session/request_permission" && m["params"]["sessionId"] == sid
        })
        .await
    }

    async fn close(&mut self, id: i64, sid: &str) {
        self.send(request(id, "session/close", json!({"sessionId":sid})))
            .await;
        let response = self.reply(id).await;
        assert!(response["result"].is_object(), "{response}");
    }

    async fn start_side(&mut self, id: i64, parent: &str) -> Value {
        self.send(open_side(id, &self.cwd, parent_mark(parent)))
            .await;
        self.reply(id).await
    }
}

fn side_id(response: &Value) -> String {
    assert_eq!(
        response["result"]["_meta"][MARK],
        parent_mark("0"),
        "{response}"
    );
    let id = response["result"]["sessionId"].as_str().unwrap();
    assert_ne!(id, "0");
    id.to_owned()
}

// A machine without user namespaces still exercises the fail-closed launch.
// Check the OS independently so an error from the host cannot skip a working runner.
fn read_only_launch_available() -> bool {
    std::process::Command::new("/usr/bin/bwrap")
        .args([
            "--unshare-user",
            "--unshare-pid",
            "--unshare-ipc",
            "--disable-userns",
            "--new-session",
            "--die-with-parent",
            "--cap-drop",
            "ALL",
            "--ro-bind",
            "/",
            "/",
            "--dev",
            "/dev",
            "--proc",
            "/proc",
            "--tmpfs",
            "/dev/shm",
            "--",
            "/bin/true",
        ])
        .output()
        .is_ok_and(|r| r.status.success())
}

fn transcripts(directory: &Path) -> String {
    let mut contents = String::new();
    for entry in fs::read_dir(directory).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            contents.push_str(&transcripts(&path));
        } else if path.extension().is_some_and(|ext| ext == "jsonl") {
            contents.push_str(&fs::read_to_string(path).unwrap());
        }
    }
    contents
}

#[tokio::test]
async fn rebuilt_opens_require_a_live_matching_parent_and_an_enforced_read_only_launch() {
    let mut host = SideHost::start(false, None).await;
    let home = host.cwd.with_file_name("home");
    fs::write(host.cwd.join("specimen.txt"), "cedar").unwrap();
    fs::write(home.join("fixture-credential"), "synthetic-access").unwrap();
    for (i, mark) in [
        Value::Null,
        json!({"bindingId":"other","generation":1,"sessionId":"0"}),
        json!({"bindingId":"aster","generation":2,"sessionId":"0"}),
        parent_mark("missing"),
    ]
    .into_iter()
    .enumerate()
    {
        let id = 10 + i as i64;
        host.send(open_side(id, &host.cwd, mark)).await;
        let rejected = host.reply(id).await;
        assert!(rejected["error"].is_object(), "{rejected}");
    }

    let opened = host.start_side(20, "0").await;
    if read_only_launch_available() {
        let side = side_id(&opened);
        host.prompt(21, &side, "inspect-workspace").await;
        let report = host.reply(21).await;
        let observed = &report["result"]["_meta"];
        assert_eq!(observed["before"], "cedar");
        assert!(
            matches!(
                observed["write"].as_str(),
                Some("EROFS" | "EPERM" | "EACCES")
            ),
            "{report}"
        );
        assert_eq!(observed["commandWrite"], false);
        assert_eq!(observed["credential"], "synthetic-access");
        assert_eq!(observed["cwd"], host.cwd.to_str().unwrap());
        assert_eq!(observed["home"], home.to_str().unwrap());
        assert_eq!(observed["model"], "aster");
        assert_eq!(observed["adapter"], "aster");
        assert_eq!(observed["mode"], "plan");
        assert_eq!(
            fs::read_to_string(host.cwd.join("specimen.txt")).unwrap(),
            "cedar"
        );
        assert!(!host.cwd.join("created-by-command").exists());
        host.close(22, &side).await;
    } else {
        assert!(opened["error"].is_object(), "{opened}");
    }

    host.prompt(23, "0", "inspect-workspace").await;
    let parent = host.reply(23).await;
    assert_eq!(parent["result"]["_meta"]["write"], "ok");
    assert_eq!(parent["result"]["_meta"]["mode"], "bypassPermissions");
    assert_eq!(parent["result"]["_meta"]["commandWrite"], true);
}

#[tokio::test]
async fn simultaneous_rebuilt_threads_preserve_turns_permissions_capacity_and_history() {
    let mut host = SideHost::start(true, Some(3)).await;
    host.prompt(4, "0", "spoof-side-marker").await;
    let parent_permission = host.permission("0").await;
    let parent_update = host
        .matching(|m| {
            m["method"] == "session/update"
                && m["params"]["update"]["content"]["text"] == "spoof-side-marker"
        })
        .await;
    for frame in [&parent_update, &parent_permission] {
        assert!(frame["params"]["_meta"].get(MARK).is_none(), "{frame}");
        assert_eq!(frame["params"]["_meta"]["fixture.trace"], "periwinkle");
    }

    for id in 5..8 {
        host.send(open_side(id, &host.cwd, parent_mark("0"))).await;
    }
    let mut sides = Vec::new();
    let available = read_only_launch_available();
    let mut full = 0;
    for id in 5..8 {
        let response = host.reply(id).await;
        if available && response["error"].is_object() {
            assert_eq!(response["error"]["code"], -32017, "{response}");
            assert_eq!(response["error"]["message"], "machine full");
            full += 1;
        } else if available {
            sides.push(side_id(&response));
        } else {
            assert!(response["error"].is_object(), "{response}");
        }
    }
    if !available {
        return;
    }
    assert_eq!(full, 1);
    assert_eq!(sides.len(), 2);
    let (a, b) = (&sides[0], &sides[1]);
    assert_ne!(a, b);
    host.prompt(8, a, "juniper-side-only").await;
    host.prompt(9, b, "birch-side-only").await;
    let a_permission = host.permission(a).await;
    let b_permission = host.permission(b).await;
    for (side, permission, text) in [
        (a, &a_permission, "juniper-side-only"),
        (b, &b_permission, "birch-side-only"),
    ] {
        assert_eq!(permission["params"]["_meta"][MARK], parent_mark("0"));
        assert_ne!(permission["id"], parent_permission["id"]);
        let update = host
            .matching(|m| {
                m["method"] == "session/update" && m["params"]["update"]["content"]["text"] == text
            })
            .await;
        assert_eq!(update["params"]["sessionId"], side.as_str());
        assert_eq!(update["params"]["_meta"][MARK], parent_mark("0"));
    }
    assert_ne!(a_permission["id"], b_permission["id"]);

    host.send(json!({"jsonrpc":"2.0","id":b_permission["id"],"result":{"outcome":{"outcome":"cancelled"}}})).await;
    assert_eq!(
        host.reply(9).await["result"]["_meta"]["decision"]["outcome"],
        "cancelled"
    );
    host.close(10, a).await;
    let interrupted = host.reply(8).await;
    assert!(
        interrupted["error"].is_object() || interrupted["result"]["stopReason"] == "cancelled",
        "{interrupted}"
    );
    let replacement = side_id(&host.start_side(11, "0").await);
    assert_ne!(&replacement, a);
    let nested = host.start_side(12, b).await;
    assert!(nested["error"].is_object(), "{nested}");

    host.send(request(13, "session/list", json!({}))).await;
    let listed = host.reply(13).await;
    let sessions = listed["result"]["sessions"].as_array().unwrap();
    assert!(sessions.iter().all(
        |s| !sides.contains(&s["sessionId"].as_str().unwrap().to_owned())
            && s["sessionId"] != replacement
    ));

    for (id, method, sid) in [
        (14, "session/load", a),
        (15, "session/resume", a),
        (16, "session/fork", b),
    ] {
        host.send(request(
            id,
            method,
            json!({"sessionId":sid,"cwd":host.cwd,"mcpServers":[]}),
        ))
        .await;
        let response = host.reply(id).await;
        assert!(response["error"].is_object(), "{method}: {response}");
    }
    host.send(json!({"jsonrpc":"2.0","id":parent_permission["id"],"result":{"outcome":{"outcome":"selected","optionId":"allow-aster"}}})).await;
    assert_eq!(
        host.reply(4).await["result"]["_meta"]["decision"]["optionId"],
        "allow-aster"
    );
    host.close(17, "0").await;
    let history = transcripts(&host.cwd.with_file_name("home"));
    assert!(history.contains("spoof-side-marker"), "{history}");
    assert!(
        !history.contains("juniper-side-only") && !history.contains("birch-side-only"),
        "{history}"
    );
}

async fn assert_lease_released(path: &Path) {
    let file = fs::File::open(path).unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match file.try_lock() {
                Ok(()) => break,
                Err(fs::TryLockError::WouldBlock) => {
                    tokio::time::sleep(Duration::from_millis(15)).await
                }
                Err(error) => panic!("could not check fixture lease: {error}"),
            }
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn side_processes_end_with_a_closed_or_crashed_parent() {
    for crash in [false, true] {
        let mut host = SideHost::start(false, None).await;
        let mut files = Vec::new();
        for id in 4..6 {
            let name = format!("hold-lease-{id}");
            let path = host.cwd.join(&name);
            fs::write(&path, []).unwrap();
            let response = host.start_side(id, "0").await;
            if !read_only_launch_available() {
                assert!(response["error"].is_object(), "{response}");
                return;
            }
            let side = side_id(&response);
            host.prompt(id + 10, &side, &name).await;
            host.permission(&side).await;
            assert!(matches!(
                fs::File::open(&path).unwrap().try_lock(),
                Err(fs::TryLockError::WouldBlock)
            ));
            files.push(path);
        }
        if crash {
            host.prompt(30, "0", "exit-fixture").await;
        } else {
            host.close(30, "0").await;
        }
        for path in files {
            assert_lease_released(&path).await;
        }
    }
}

#[tokio::test]
async fn bypass_only_adapter_cannot_open_a_side_thread() {
    let mut host = SideHost::with_modes(true, None, "bypassPermissions").await;
    let refused = host.start_side(4, "0").await;
    assert!(refused["error"].is_object(), "{refused}");
    host.prompt(5, "0", "finish").await;
    assert_eq!(host.reply(5).await["result"]["stopReason"], "end_turn");
}

#[tokio::test]
async fn unavailable_os_boundary_refuses_the_side_and_keeps_parent_usable() {
    if read_only_launch_available() {
        // Exercise the actual kernel refusal on hosts that can normally launch a side.
        // The nested test has a writable temporary folder but cannot create another user namespace.
        assert!(std::env::var_os("FIXTURE_NO_USER_NAMESPACES").is_none());
        let folder = std::env::temp_dir();
        let mut command = std::process::Command::new("/usr/bin/bwrap");
        command.args(["--unshare-user", "--disable-userns", "--ro-bind", "/", "/", "--dev", "/dev", "--proc", "/proc", "--bind"])
            .arg(&folder).arg(&folder).arg("--").arg(std::env::current_exe().unwrap())
            .args(["--exact", "standard_acp::side_sessions::unavailable_os_boundary_refuses_the_side_and_keeps_parent_usable"])
            .env("FIXTURE_NO_USER_NAMESPACES", "1");
        let output = success(super::super::bounded_output(&mut command));
        assert!(output.contains("1 passed"), "{output}");
        return;
    }
    let mut host = SideHost::start(false, Some(2)).await;
    for id in [4, 5] {
        let refused = host.start_side(id, "0").await;
        assert!(refused["error"].is_object(), "{refused}");
        assert_ne!(refused["error"]["code"], -32017, "{refused}");
    }
    host.prompt(6, "0", "finish").await;
    assert_eq!(host.reply(6).await["result"]["stopReason"], "end_turn");
}

#[tokio::test]
async fn config_options_only_adapter_uses_the_restrictive_mode_and_pinned_model() {
    let mut host = SideHost::start(false, None).await;
    fs::write(host.cwd.join("fixture-config-options-only"), []).unwrap();
    let response = host.start_side(4, "0").await;
    if !read_only_launch_available() {
        assert!(response["error"].is_object(), "{response}");
        return;
    }
    let side = side_id(&response);
    let options = response["result"]["configOptions"].as_array().unwrap();
    for (category, expected) in [("mode", "plan"), ("model", "aster")] {
        let option = options.iter().find(|v| v["category"] == category).unwrap();
        assert_eq!(option["currentValue"], expected, "{response}");
    }
    host.prompt(5, &side, "inspect-settings").await;
    let observed = host.reply(5).await;
    assert_eq!(
        observed["result"]["_meta"],
        json!({"mode":"plan","model":"aster"})
    );
}
