use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use tokio::io::AsyncBufReadExt;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::io::BufReader;
use tokio::io::Lines;
use tokio::process::Child;
use tokio::process::ChildStdin;
use tokio::process::ChildStdout;

use super::FIXTURE;
use super::Host;

fn stage_wrapper(runtime: &Path, script: &str) -> PathBuf {
    let path = runtime.join("fixture wrapper");
    fs::write(&path, script).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
    path
}

/// A foreground host on stdin and stdout, as a machine bridge would run it.
struct StdioHost {
    host: Host,
    child: Child,
    input: ChildStdin,
    output: Lines<BufReader<ChildStdout>>,
    cwd: PathBuf,
    home: PathBuf,
}

impl StdioHost {
    fn start(spec_env: Value, process_env: &[(&str, &str)], extra: &[&str]) -> Self {
        Self::start_with_launch(spec_env, process_env, extra, |_, _| {})
    }

    fn start_with_launch(
        spec_env: Value,
        process_env: &[(&str, &str)],
        extra: &[&str],
        configure: impl FnOnce(&mut Value, &std::path::Path),
    ) -> Self {
        Self::start_in(Host::new(), spec_env, process_env, extra, configure)
    }

    fn start_in(
        host: Host,
        spec_env: Value,
        process_env: &[(&str, &str)],
        extra: &[&str],
        configure: impl FnOnce(&mut Value, &std::path::Path),
    ) -> Self {
        let cwd = host.0.path().join("workspace");
        let home = host.0.path().join("home");
        fs::create_dir_all(&cwd).unwrap();
        fs::create_dir_all(&home).unwrap();
        fs::write(cwd.join("fixture-modes"), "bypassPermissions plan").unwrap();
        fs::write(
            host.0.path().join("config.toml"),
            format!(
                "[host]\nmanaged=true\nside_sessions=true\n[host.dedicated_roots]\nbinding_id='aster'\ncwd='{}'\nhome='{}'\n",
                cwd.display(),
                home.display()
            ),
        )
        .unwrap();
        let launch = host.0.path().join("launch.json");
        let mut spec = json!({
            "projectId":"fixture-project", "runId":"run-aster", "agent":"aster", "model":"aster",
            "cwd":cwd, "home":home, "env":spec_env,
            "adapter":{"path":which::which("python3").unwrap(),"version":"fixture-1","args":[FIXTURE,"aster"]},
            "additions":{"promptEcho":false,"statusEvents":false,"recording":false,"clientMcp":false}
        });
        configure(&mut spec, &home);
        fs::write(&launch, spec.to_string()).unwrap();
        let mut command =
            tokio::process::Command::from(host.command(&["host", "--stdio", "--launch"]));
        let mut child = command
            .arg(&launch)
            .args(extra)
            .env_clear()
            .env("NORI_HOME", host.0.path())
            .env("HOME", &home)
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .envs(process_env.iter().copied())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        Self {
            input: child.stdin.take().unwrap(),
            output: BufReader::new(child.stdout.take().unwrap()).lines(),
            host,
            child,
            cwd: cwd.canonicalize().unwrap(),
            home: home.canonicalize().unwrap(),
        }
    }

    async fn send(&mut self, message: Value) {
        self.input
            .write_all(format!("{message}\n").as_bytes())
            .await
            .unwrap();
    }

    async fn receive(&mut self, matches: impl Fn(&Value) -> bool) -> Value {
        tokio::time::timeout(Duration::from_secs(20), async {
            loop {
                let line = self.output.next_line().await.unwrap().unwrap();
                let message: Value = serde_json::from_str(&line).unwrap();
                assert_eq!(message["jsonrpc"], "2.0", "{message}");
                assert!(message.get("bindingId").is_none(), "{message}");
                if matches(&message) {
                    return message;
                }
            }
        })
        .await
        .unwrap()
    }

    /// Send one ACP request and return its response on the bare stdio stream.
    async fn call(&mut self, id: i64, method: &str, params: Value) -> Value {
        self.send(json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))
            .await;
        self.receive(|message| message["id"] == id && message.get("method").is_none())
            .await
    }

    async fn open_parent(&mut self) {
        let initialized = self
            .call(1, "initialize", json!({"protocolVersion":1}))
            .await;
        assert_eq!(initialized["result"]["agentInfo"]["name"], "aster");
        let opened = self
            .call(2, "session/new", json!({"cwd":self.cwd,"mcpServers":[]}))
            .await;
        assert_eq!(opened["result"]["sessionId"], "0");
    }

    async fn prompt(&mut self, id: i64, sid: &str, text: &str) -> Value {
        self.call(
            id,
            "session/prompt",
            json!({"sessionId":sid,"prompt":[{"type":"text","text":text}]}),
        )
        .await
    }

    /// Close stdin and wait for the host to leave.
    async fn finish(mut self) -> (std::process::ExitStatus, Host) {
        drop(self.input);
        let status = tokio::time::timeout(Duration::from_secs(35), self.child.wait())
            .await
            .unwrap()
            .unwrap();
        (status, self.host)
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stdio_admission_digest_tracks_retained_inherited_inputs_only() {
    let mut files = Host::new();
    let mut receipts = Vec::new();
    for (removed, retained) in [("cedar", "ivy"), ("birch", "ivy"), ("birch", "moss")] {
        // Keep both root paths and PATH fixed. The CLI adds a temporary helper directory to its own PATH.
        let mut host = StdioHost::start_in(
            files,
            json!({"PATH":"/usr/bin:/bin"}),
            &[("FIXTURE_REMOVED", removed), ("FIXTURE_RETAINED", retained)],
            &["--inherit-env"],
            |spec, home| {
                let runtime = home.join("prepared");
                fs::create_dir_all(&runtime).unwrap();
                let wrapper = stage_wrapper(
                    &runtime,
                    r#"#!/bin/sh
printf '%s %s' "${FIXTURE_REMOVED-unset}" "$FIXTURE_RETAINED" > child-environment
exec "$@"
"#,
                );
                spec["runtimeDir"] = json!(runtime);
                // CoreFoundation supplies a process environment variable even after env_clear on macOS.
                spec["envRemove"] = json!(["FIXTURE_REMOVED", "__CF_USER_TEXT_ENCODING"]);
                spec["adapter"]["wrapper"] = json!({"path":wrapper,"version":"fixture"});
            },
        );
        host.open_parent().await;
        assert_eq!(
            fs::read_to_string(host.cwd.join("child-environment")).unwrap(),
            format!("unset {retained}")
        );
        let mut control = tokio::task::block_in_place(|| host.host.client());
        let status = control
            .request(
                "_nori/session/status",
                json!({"bindingId":"aster","generation":1}),
            )
            .await
            .unwrap();
        let digest = status["specDigest"].as_str().unwrap();
        assert_eq!(digest.len(), 64);
        receipts.push(digest.to_owned());

        let mut spec: Value =
            serde_json::from_slice(&fs::read(host.host.0.path().join("launch.json")).unwrap())
                .unwrap();
        spec["env"]["FIXTURE_RETAINED"] = json!(retained);
        let mut retry = json!({"bindingId":"aster","generation":1,"commandId":"retry",
            "conversationId":status["conversationId"],"spec":spec});
        assert_eq!(
            control
                .request("_nori/session/ensure", retry.clone())
                .await
                .unwrap(),
            status
        );
        retry["spec"]["env"]["FIXTURE_RETAINED"] = json!("changed");
        assert!(
            control
                .request("_nori/session/ensure", retry)
                .await
                .unwrap_err()
                .to_string()
                .contains("conflict")
        );
        let (exit, original_files) = host.finish().await;
        assert!(exit.success());
        files = original_files;
    }
    assert_eq!(receipts[0], receipts[1], "removed input changed the digest");
    assert_ne!(receipts[1], receipts[2], "retained input was omitted");
}

#[tokio::test]
async fn plain_env_and_wrapped_managed_launches_select_two_independent_acp_servers() {
    let python = which::which("python3").unwrap();
    let rust_peer = std::env::var_os("MOCK_ACP_AGENT_BIN")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_BIN_EXE_nori")).with_file_name("mock_acp_agent")
        });
    for (name, program, args) in [
        ("aster", python, vec![FIXTURE, "aster"]),
        ("mock-agent", rust_peer, Vec::new()),
    ] {
        for mode in ["plain", "env", "wrapped"] {
            let mut host = StdioHost::start_with_launch(
                json!({}),
                &[("FIXTURE_AMBIENT", "synthetic")],
                &["--inherit-env"],
                |spec, home| {
                    let alias = home.join("selected inner executable");
                    std::os::unix::fs::symlink(&program, &alias).unwrap();
                    let fallback = if mode == "plain" {
                        &program
                    } else {
                        spec["env"]["FIXTURE_INNER"] = json!(alias);
                        spec["envRemove"] = json!(["FIXTURE_AMBIENT"]);
                        &home.join("unselected default")
                    };
                    // This synthetic server acts like an adapter that selects its inner CLI from env.
                    let mut adapter_args = vec![
                        "-c",
                        r#"printf '%s' "${FIXTURE_AMBIENT-unset}" > observed-ambient
program=$1; shift; exec "${FIXTURE_INNER:-$program}" "$@""#,
                        "fixture-server",
                        fallback.to_str().unwrap(),
                    ];
                    adapter_args.extend_from_slice(&args);
                    spec["adapter"] =
                        json!({"path":"/bin/sh","version":"fixture-1","args":adapter_args});
                    if mode == "wrapped" {
                        let runtime = home.join("prepared");
                        fs::create_dir(&runtime).unwrap();
                        fs::write(runtime.join("marker"), "wrapped").unwrap();
                        let wrapper = stage_wrapper(
                            &runtime,
                            r#"#!/bin/sh
test "$1" = -- || exit 1
shift
/bin/cat "$NORI_AGENT_RUNTIME_DIR/marker" > wrapper-invoked || exit 1
exec "$@"
"#,
                        );
                        spec["runtimeDir"] = json!(runtime);
                        spec["adapter"]["wrapper"] =
                            json!({"path":wrapper,"version":"fixture-1","args":["--"]});
                    }
                },
            );
            let init = host
                .call(1, "initialize", json!({"protocolVersion":1}))
                .await;
            assert_eq!(init["result"]["agentInfo"]["name"], name, "{mode}");
            // Ordinary ACP messages can open a session on the existing server only.
            let other = json!({"path":"/bin/sh", "version":"fixture", "args":["-c","echo changed > unadmitted"]});
            let opened = host
                .call(
                    2,
                    "session/new",
                    json!({
                        "cwd":host.cwd,"mcpServers":[], "adapter":other, "wrapper":other
                    }),
                )
                .await;
            let sid = opened["result"]["sessionId"].as_str().unwrap();
            assert_eq!(
                host.prompt(3, sid, "finish").await["result"]["stopReason"],
                "end_turn",
                "{name} {mode}"
            );
            assert_eq!(
                fs::read_to_string(host.cwd.join("observed-ambient")).unwrap(),
                if mode == "plain" {
                    "synthetic"
                } else {
                    "unset"
                }
            );
            assert_eq!(host.cwd.join("wrapper-invoked").exists(), mode == "wrapped");
            if mode == "wrapped" {
                assert_eq!(
                    fs::read_to_string(host.cwd.join("wrapper-invoked")).unwrap(),
                    "wrapped"
                );
            }
            assert!(!host.cwd.join("unadmitted").exists());
            assert!(host.finish().await.0.success());
        }
    }
}

#[tokio::test]
async fn wrapped_launch_preserves_child_inputs_turns_cancellation_and_cleanup() {
    let python = which::which("python3").unwrap();
    let mut host = StdioHost::start_with_launch(
        json!({"SAMPLE_LABEL":"orchid","Sample_Host_Only":"explicit"}),
        &[
            ("SAMPLE_HOST_ONLY", "hidden"),
            ("sample_host_only", "lowercase kept"),
            ("Sample_Host_Only", "inherited"),
            ("SAMPLE_INHERITED", "kept"),
        ],
        &["--inherit-env"],
        |spec, home| {
            let runtime = home.join("prepared");
            fs::create_dir(&runtime).unwrap();
            fs::write(runtime.join("marker"), "prepared orchid").unwrap();
            let wrapper = stage_wrapper(
                &runtime,
                r#"#!/usr/bin/env python3
import json, os, sys
from pathlib import Path
prefix, empty, separator, server, *args = sys.argv[1:]
observed = [prefix, empty, separator, server, args, os.environ["SAMPLE_LABEL"],
            os.getenv("SAMPLE_HOST_ONLY"), os.environ["sample_host_only"], os.environ["Sample_Host_Only"],
            os.environ["SAMPLE_INHERITED"], os.environ["HOME"],
            Path(os.environ["NORI_AGENT_RUNTIME_DIR"], "marker").read_text()]
Path("child-inputs.json").write_text(json.dumps(observed))
os.execv(server, [server, *args])
"#,
            );
            spec["runtimeDir"] = json!(runtime);
            spec["envRemove"] = json!(["SAMPLE_HOST_ONLY"]);
            spec["adapter"]["args"] = json!([
                FIXTURE,
                "aster",
                "workspace with spaces",
                "--",
                "*.fixture",
                "",
                "雪と杉"
            ]);
            spec["adapter"]["wrapper"] =
                json!({"path":wrapper,"version":"fixture-1","args":["two words","","--"]});
        },
    );
    host.open_parent().await;
    let observed: Value =
        serde_json::from_slice(&fs::read(host.cwd.join("child-inputs.json")).unwrap()).unwrap();
    assert_eq!(
        observed,
        json!([
            "two words",
            "",
            "--",
            python,
            [
                FIXTURE,
                "aster",
                "workspace with spaces",
                "--",
                "*.fixture",
                "",
                "雪と杉"
            ],
            "orchid",
            null,
            "lowercase kept",
            "explicit",
            "kept",
            host.home,
            "prepared orchid"
        ])
    );

    let delivered = host.prompt(8, "0", "inspect-argv").await;
    assert_eq!(
        delivered["result"]["_meta"]["argv"],
        json!([
            "aster",
            "workspace with spaces",
            "--",
            "*.fixture",
            "",
            "雪と杉"
        ])
    );

    let lease = fs::File::create(host.cwd.join("hold-lease-server")).unwrap();
    let descendant = fs::File::create(host.cwd.join("hold-lease-server-client")).unwrap();
    let completed = host.prompt(3, "0", "hold-lease-server").await;
    assert_eq!(completed["result"]["stopReason"], "end_turn");
    assert!(matches!(
        lease.try_lock(),
        Err(fs::TryLockError::WouldBlock)
    ));
    assert!(matches!(
        descendant.try_lock(),
        Err(fs::TryLockError::WouldBlock)
    ));
    host.send(json!({"jsonrpc":"2.0","id":4,"method":"session/prompt",
        "params":{"sessionId":"0","prompt":[{"type":"text","text":"await approval"}]}}))
        .await;
    host.receive(|message| message["method"] == "session/request_permission")
        .await;
    host.send(json!({"jsonrpc":"2.0","method":"session/cancel","params":{"sessionId":"0"}}))
        .await;
    let cancelled = host
        .receive(|message| message["id"] == 4 && message.get("method").is_none())
        .await;
    assert_eq!(cancelled["result"]["stopReason"], "cancelled");
    assert!(host.finish().await.0.success());
    lease.try_lock().expect("server survived the host");
    descendant
        .try_lock()
        .expect("server descendant survived the host");
}

#[tokio::test]
async fn stdio_host_serves_the_pinned_adapter_and_a_side_session_as_plain_acp_until_stdin_closes() {
    let mut host = StdioHost::start(json!({"FIXTURE_MODEL":"aster"}), &[], &[]);
    host.open_parent().await;
    let finished = host.prompt(3, "0", "finish").await;
    assert_eq!(finished["result"]["stopReason"], "end_turn");

    let mark = json!({"bindingId":"aster","generation":1,"sessionId":"0"});
    let side = host
        .call(
            4,
            "session/new",
            json!({"cwd":host.cwd,"mcpServers":[],"_meta":{"nori.dev/sideSession":mark}}),
        )
        .await;
    assert_eq!(
        side["result"]["_meta"]["nori.dev/sideSession"], mark,
        "{side}"
    );
    #[cfg(not(target_os = "linux"))]
    assert_eq!(side["result"]["_meta"]["nori.dev/sideConfinement"], "none");
    let side = side["result"]["sessionId"].as_str().unwrap().to_owned();
    assert_ne!(side, "0");
    let settings = host.prompt(5, &side, "inspect-settings").await;
    assert_eq!(
        settings["result"]["_meta"],
        json!({"mode":"plan","model":"aster"})
    );
    let parent = host.prompt(6, "0", "finish").await;
    assert_eq!(parent["result"]["stopReason"], "end_turn");

    let (status, files) = host.finish().await;
    assert!(status.success(), "{status}");
    assert!(!files.0.path().join("host/state.json").exists());
}

#[tokio::test]
async fn stdio_host_exits_and_ends_its_adapter_when_terminated_with_stdin_still_open() {
    let mut host = StdioHost::start(json!({"FIXTURE_MODEL":"aster"}), &[], &[]);
    fs::write(host.cwd.join("specimen.txt"), "cedar").unwrap();
    fs::write(host.home.join("fixture-credential"), "synthetic-access").unwrap();
    host.open_parent().await;
    // A machine bridge stops its child with a signal and keeps the pipes.
    let adapter = fs::File::create(host.cwd.join("hold-lease-adapter")).unwrap();
    fs::File::create(host.cwd.join("hold-lease-adapter-client")).unwrap();
    let held = host.prompt(3, "0", "hold-lease-adapter").await;
    assert_eq!(held["result"]["stopReason"], "end_turn", "{held}");
    assert!(matches!(
        adapter.try_lock(),
        Err(fs::TryLockError::WouldBlock)
    ));

    let pid = i32::try_from(host.child.id().unwrap()).unwrap();
    // SAFETY: signals the child this test started and still owns.
    assert_eq!(unsafe { libc::kill(pid, libc::SIGTERM) }, 0);
    let status = tokio::time::timeout(Duration::from_secs(20), host.child.wait())
        .await
        .expect("the host is still running after the signal")
        .unwrap();
    assert!(!status.success());
    adapter.try_lock().expect("the adapter outlived its host");
    drop(host.input);
}

#[tokio::test]
async fn stdio_host_can_give_the_adapter_its_own_environment_but_not_its_home() {
    let mut host = StdioHost::start(
        json!({}),
        &[
            ("FIXTURE_MODEL", "aster"),
            ("HOME", "/nonexistent-caller-home"),
            ("NORI_AGENT_RUNTIME_DIR", "/nonexistent-caller-runtime"),
        ],
        &["--inherit-env"],
    );
    fs::write(host.cwd.join("specimen.txt"), "cedar").unwrap();
    fs::write(host.home.join("fixture-credential"), "synthetic-access").unwrap();
    host.open_parent().await;
    let report = host.prompt(3, "0", "inspect-workspace").await;
    let observed = &report["result"]["_meta"];
    assert_eq!(observed["model"], "aster", "{report}");
    assert_eq!(observed["home"], host.home.to_str().unwrap());
    assert_eq!(observed["cwd"], host.cwd.to_str().unwrap());
    assert!(host.finish().await.0.success());
}

#[tokio::test]
async fn stdio_host_exits_with_an_error_and_a_clean_stdout_when_the_launch_is_refused() {
    let mut host = StdioHost::start(json!({"HOME":"/reserved"}), &[], &[]);
    let status = tokio::time::timeout(Duration::from_secs(35), host.child.wait())
        .await
        .unwrap()
        .unwrap();
    assert!(!status.success());
    assert_eq!(host.output.next_line().await.unwrap(), None);
    let mut reason = String::new();
    host.child
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut reason)
        .await
        .unwrap();
    assert!(reason.contains("environment key"), "{reason}");
}
