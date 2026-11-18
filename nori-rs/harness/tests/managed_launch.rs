#![cfg(unix)]
#![allow(clippy::unwrap_used)]

use std::fs;
use std::time::Duration;

use nori_harness::managed_launch::ManagedLaunches;
use serde_json::json;
use tokio::io::AsyncBufReadExt;
use tokio::io::AsyncWriteExt;
use tokio::io::BufReader;

#[tokio::test]
async fn handoff_uses_the_admitted_process_and_leaves_initialize_to_the_client() {
    let root = tempfile::tempdir().unwrap();
    let binding = root.path().join("sample");
    let cwd = binding.join("work");
    let home = binding.join("home");
    fs::create_dir_all(&cwd).unwrap();
    fs::create_dir_all(&home).unwrap();
    let alias = binding.join("work-link");
    std::os::unix::fs::symlink(&cwd, &alias).unwrap();
    let launches = ManagedLaunches::new(root.path(), 1).unwrap();
    let request = serde_json::from_value(json!({
        "commandId":"one","conversationId":uuid::Uuid::new_v4(),"bindingId":"sample","generation":2,
        "spec":{"agent":"fixture","projectId":"project","runId":"run","cwd":alias,"home":home,
        "adapter":{"path":"/bin/sh","version":"fixture-v1","args":["-c",
            "echo start >> starts; read -r first; echo \"$first\"; read -r next"]},
        "additions":{"promptEcho":false,"statusEvents":false,"recording":false,"clientMcp":false}}
    }))
    .unwrap();
    launches.ensure(request).unwrap();
    assert!(launches.take_connection("sample", 1).is_err());
    let mut pipe = launches.take_connection("sample", 2).unwrap();
    assert_eq!(pipe.cwd, cwd.canonicalize().unwrap());
    assert_eq!(pipe.home, home.canonicalize().unwrap());
    assert!(launches.take_connection("sample", 2).is_err());
    let initialize =
        r#"{"jsonrpc":"2.0","id":91,"method":"initialize","params":{"protocolVersion":1}}"#;
    pipe.stdin
        .write_all(format!("{initialize}\n").as_bytes())
        .await
        .unwrap();
    let mut line = String::new();
    tokio::time::timeout(
        Duration::from_secs(5),
        BufReader::new(pipe.stdout).read_line(&mut line),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(line.trim_end(), initialize);
    launches.shutdown().await.unwrap();
    assert_eq!(fs::read_to_string(cwd.join("starts")).unwrap(), "start\n");
}

#[test]
fn dedicated_binding_defaults_to_eight_process_slots() {
    let root = tempfile::tempdir().unwrap();
    let cwd = root.path().join("workspace");
    let home = root.path().join("home");
    fs::create_dir(&cwd).unwrap();
    fs::create_dir(&home).unwrap();
    let config = nori_config::HostConfig {
        managed: true,
        dedicated_roots: Some(nori_config::DedicatedRoots {
            binding_id: "sample".into(),
            cwd,
            home,
        }),
        ..Default::default()
    };
    let launches = ManagedLaunches::from_config(&config, root.path()).unwrap();
    let caps = serde_json::to_value(launches.capabilities()).unwrap();
    assert_eq!(caps["maxSessions"], 8);
    assert!(caps.get("rebuiltSideSessions").is_none());
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn read_only_launch_pins_environment_and_isolates_scratch() {
    let root = tempfile::tempdir().unwrap();
    let binding = root.path().join("sample");
    let home = binding.join("home");
    let cwd = home.join("work");
    fs::create_dir_all(&cwd).unwrap();
    fs::write(cwd.join("input"), "original").unwrap();
    let launches = ManagedLaunches::from_config(
        &nori_config::HostConfig {
            managed: true,
            side_sessions: true,
            bindings_root: Some(root.path().into()),
            max_sessions: 2,
            ..Default::default()
        },
        root.path(),
    )
    .unwrap();
    let request = serde_json::from_value(json!({
        "commandId":"start", "conversationId":uuid::Uuid::new_v4(), "bindingId":"sample", "generation":2,
        "spec":{"agent":"fixture","projectId":"project","runId":"run","cwd":cwd,"home":home,
            "env":{"COLOR":"red","TMPDIR":cwd},
            "adapter":{"path":"/bin/sh","version":"fixture-v1","args":["-c",r#"
                read -r input || exit 0
                test "$COLOR" = red && test "$CODEX_HOME" = "$HOME/.codex" || exit 1
                if touch "$PWD/creation" || chmod 000 "$PWD/input" || touch "$HOME/state"; then exit 2; fi
                test ! -e "$TMPDIR/marker" && touch "$TMPDIR/marker" && echo private
            "#]},
            "additions":{"promptEcho":false,"statusEvents":false,"recording":false,"clientMcp":false}
        }
    })).unwrap();
    launches.ensure(request).unwrap();

    // Check this host's sandbox separately; a launch returning an empty stream
    // on a supported host is a failure. In a restricted CI container the
    // real-host protocol test exercises the refusal path.
    let supported = std::process::Command::new("/usr/bin/bwrap")
        .args([
            "--unshare-user",
            "--unshare-pid",
            "--unshare-ipc",
            "--disable-userns",
            "--ro-bind",
            "/",
            "/",
            "--proc",
            "/proc",
            "--dev",
            "/dev",
            "--tmpfs",
            "/dev/shm",
            "--",
            "/bin/true",
        ])
        .output()
        .is_ok_and(|result| result.status.success());
    if supported {
        for id in ["first", "second"] {
            let mut child = launches.spawn_side("sample", 2, id).unwrap();
            child.stdin.write_all(b"probe\n").await.unwrap();
            let mut result = String::new();
            tokio::time::timeout(
                Duration::from_secs(5),
                BufReader::new(child.stdout).read_line(&mut result),
            )
            .await
            .unwrap()
            .unwrap();
            assert_eq!(result, "private\n");
            launches.stop_side("sample", 2, id).await;
        }
        assert_eq!(fs::read_to_string(cwd.join("input")).unwrap(), "original");
        assert!(!cwd.join("creation").exists());
        assert!(!home.join("state").exists());
    }
    launches.shutdown().await.unwrap();
}
