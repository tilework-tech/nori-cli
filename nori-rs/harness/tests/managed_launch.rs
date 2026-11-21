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
    if sandbox_available() {
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
            launches.stop_side("sample", 2, id).await.unwrap();
        }
        assert_eq!(fs::read_to_string(cwd.join("input")).unwrap(), "original");
        assert!(!cwd.join("creation").exists());
        assert!(!home.join("state").exists());
        // Outstanding transport pipes can outlive the host owner on cancellation.
        let _parent = launches.take_connection("sample", 2).unwrap();
        let outstanding = launches.spawn_side("sample", 2, "outstanding").unwrap();
        drop(launches);
        let mut result = String::new();
        let read = tokio::time::timeout(
            Duration::from_secs(2),
            BufReader::new(outstanding.stdout).read_line(&mut result),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(read, 0);
    } else {
        launches.shutdown().await.unwrap();
    }
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn release_one_drains_both_side_process_trees_before_reusing_their_slots() {
    if !sandbox_available() {
        return;
    }
    let root = tempfile::tempdir().unwrap();
    let launches = ManagedLaunches::from_config(
        &nori_config::HostConfig {
            managed: true,
            side_sessions: true,
            bindings_root: Some(root.path().into()),
            max_sessions: 5,
            ..Default::default()
        },
        root.path(),
    )
    .unwrap();
    let script = r#"
        set -eu
        read -r side
        exec 8<"$side.adapter"
        flock -x 8
        setsid sh -ec 'exec 9<"$1.client"; flock -x 9; echo ready; exec sleep 120' sh "$side" 8<&- &
        wait
    "#;
    for binding in ["closing", "surviving"] {
        let home = root.path().join(binding).join("home");
        let cwd = root.path().join(binding).join("work");
        fs::create_dir_all(&home).unwrap();
        fs::create_dir_all(&cwd).unwrap();
        launches.ensure(serde_json::from_value(json!({
            "commandId":binding, "conversationId":uuid::Uuid::new_v4(), "bindingId":binding, "generation":1,
            "spec":{"agent":"fixture","projectId":"project","runId":"run","cwd":cwd,"home":home,
                "adapter":{"path":"/bin/sh","version":"fixture-v1","args":["-c",script]},
                "additions":{"promptEcho":false,"statusEvents":false,"recording":false,"clientMcp":false}}
        })).unwrap()).unwrap();
    }
    let mut leases = Vec::new();
    for (binding, side) in [("closing", "a"), ("closing", "b"), ("surviving", "witness")] {
        let cwd = root.path().join(binding).join("work");
        let files = ["adapter", "client"].map(|process| {
            let path = cwd.join(format!("{side}.{process}"));
            fs::write(&path, []).unwrap();
            fs::File::open(path).unwrap()
        });
        let mut pipe = launches.spawn_side(binding, 1, side).unwrap();
        pipe.stdin
            .write_all(format!("{side}\n").as_bytes())
            .await
            .unwrap();
        let mut output = BufReader::new(pipe.stdout).lines();
        let ready = tokio::time::timeout(Duration::from_secs(5), output.next_line())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(ready.as_deref(), Some("ready"));
        for file in &files {
            assert!(matches!(file.try_lock(), Err(fs::TryLockError::WouldBlock)));
        }
        leases.push((binding, files));
    }
    assert!(matches!(
        launches
            .spawn_side("surviving", 1, "full")
            .err()
            .unwrap()
            .downcast_ref(),
        Some(nori_harness::managed_launch::LaunchRefusal::AtCapacity)
    ));

    launches.release_one("closing", 1).await.unwrap();
    for (binding, files) in leases {
        for file in files {
            if binding == "closing" {
                file.try_lock()
                    .unwrap_or_else(|error| panic!("side process lease at release: {error}"));
            } else {
                assert!(matches!(file.try_lock(), Err(fs::TryLockError::WouldBlock)));
            }
        }
    }
    let _replacements =
        ["next-a", "next-b"].map(|id| launches.spawn_side("surviving", 1, id).unwrap());
    launches.shutdown().await.unwrap();
}

#[cfg(target_os = "linux")]
fn sandbox_available() -> bool {
    std::process::Command::new("bwrap")
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
        .is_ok_and(|result| result.status.success())
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn codex_sides_keep_binding_inputs_and_use_independent_runtime_homes() {
    if !sandbox_available() {
        return;
    }
    let root = tempfile::tempdir().unwrap();
    let binding = root.path().join("sample");
    let home = binding.join("home");
    let cwd = home.join("work");
    let provider = home.join(".codex");
    fs::create_dir_all(&cwd).unwrap();
    fs::create_dir(&provider).unwrap();
    fs::write(provider.join("config.toml"), "model = 'fixture'").unwrap();
    fs::write(provider.join("installation_id"), "parent").unwrap();
    let launches = ManagedLaunches::from_config(
        &nori_config::HostConfig {
            managed: true,
            side_sessions: true,
            bindings_root: Some(root.path().into()),
            max_sessions: 3,
            ..Default::default()
        },
        root.path(),
    )
    .unwrap();
    let script = r#"
        read -r marker || exit 0
        set -eu
        test ! -e "$CODEX_HOME/auth.json"
        test "$(cat "$CODEX_HOME/config.toml")" = "model = 'fixture'"
        test "$(cat "$HOME/.codex/installation_id")" = parent
        test ! -e "$CODEX_HOME/installation_id"
        test ! -e "$CODEX_SQLITE_HOME/state.sqlite"
        if touch "$PWD/new" || touch "$HOME/new" || echo changed >> "$CODEX_HOME/config.toml"; then exit 2; fi
        echo "$marker" > "$CODEX_HOME/installation_id"
        echo "$marker" > "$CODEX_SQLITE_HOME/state.sqlite"
        echo ready
        read -r done
        cat "$CODEX_HOME/installation_id" "$CODEX_SQLITE_HOME/state.sqlite"
    "#;
    launches.ensure(serde_json::from_value(json!({
        "commandId":"start", "conversationId":uuid::Uuid::new_v4(), "bindingId":"sample", "generation":1,
        "spec":{"agent":"codex","projectId":"project","runId":"run","cwd":cwd,"home":home,
            "env":{"CODEX_SQLITE_HOME":cwd},
            "adapter":{"path":"/bin/sh","version":"2.7.0","args":["-c",script]},
            "additions":{"promptEcho":false,"statusEvents":false,"recording":false,"clientMcp":false}}
    })).unwrap()).unwrap();
    let mut siblings = Vec::new();
    for id in ["lilac", "moss"] {
        let child = launches.spawn_side("sample", 1, id).unwrap();
        let mut reader = BufReader::new(child.stdout).lines();
        let mut stdin = child.stdin;
        stdin.write_all(format!("{id}\n").as_bytes()).await.unwrap();
        assert_eq!(reader.next_line().await.unwrap().as_deref(), Some("ready"));
        siblings.push((id, stdin, reader));
    }
    for (id, mut stdin, mut reader) in siblings {
        stdin.write_all(b"done\n").await.unwrap();
        assert_eq!(reader.next_line().await.unwrap().as_deref(), Some(id));
        assert_eq!(reader.next_line().await.unwrap().as_deref(), Some(id));
    }
    assert_eq!(
        fs::read_to_string(provider.join("installation_id")).unwrap(),
        "parent"
    );
    assert_eq!(
        fs::read_to_string(provider.join("config.toml")).unwrap(),
        "model = 'fixture'"
    );
    assert!(!provider.join("state.sqlite").exists());
    assert!(fs::read_dir(&cwd).unwrap().next().is_none());
    launches.shutdown().await.unwrap();
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn codex_sides_start_beside_provider_runtime_entries_and_without_mcp_servers() {
    if !sandbox_available() {
        return;
    }
    let root = tempfile::tempdir().unwrap();
    let binding = root.path().join("sample");
    let home = binding.join("home");
    let cwd = home.join("work");
    let provider = home.join(".codex");
    fs::create_dir_all(&cwd).unwrap();
    fs::create_dir(&provider).unwrap();
    let config = "model = 'fixture'\n[mcp_servers.fixture]\nurl = 'https://mcp.invalid'\n";
    fs::write(provider.join("config.toml"), config).unwrap();
    for file in ["session_index.jsonl.lock", ".nori-managed"] {
        fs::write(provider.join(file), "parent").unwrap();
    }
    for folder in ["cache", "plugins"] {
        fs::create_dir(provider.join(folder)).unwrap();
    }
    let launches = ManagedLaunches::from_config(
        &nori_config::HostConfig {
            managed: true,
            side_sessions: true,
            bindings_root: Some(root.path().into()),
            ..Default::default()
        },
        root.path(),
    )
    .unwrap();
    let script = r#"
        read -r marker || exit 0
        set -eu
        grep -q fixture "$CODEX_HOME/config.toml"
        if grep -q mcp "$CODEX_HOME/config.toml"; then exit 2; fi
        for entry in session_index.jsonl.lock .nori-managed cache plugins; do
            test ! -e "$CODEX_HOME/$entry"
        done
        echo ready
        read -r done
    "#;
    launches.ensure(serde_json::from_value(json!({
        "commandId":"start", "conversationId":uuid::Uuid::new_v4(), "bindingId":"sample", "generation":1,
        "spec":{"agent":"codex","projectId":"project","runId":"run","cwd":cwd,"home":home,
            "adapter":{"path":"/bin/sh","version":"2.7.0","args":["-c",script]},
            "additions":{"promptEcho":false,"statusEvents":false,"recording":false,"clientMcp":false}}
    })).unwrap()).unwrap();
    let mut side = launches.spawn_side("sample", 1, "lilac").unwrap();
    side.stdin.write_all(b"lilac\n").await.unwrap();
    let mut output = BufReader::new(side.stdout).lines();
    assert_eq!(output.next_line().await.unwrap().as_deref(), Some("ready"));
    assert_eq!(
        fs::read_to_string(provider.join("config.toml")).unwrap(),
        config
    );
    launches.shutdown().await.unwrap();
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn a_codex_side_without_the_boundary_runs_on_the_binding_home_and_stops_with_it() {
    const NAME: &str =
        "a_codex_side_without_the_boundary_runs_on_the_binding_home_and_stops_with_it";
    if sandbox_available() {
        // Rerun where another user namespace cannot be created, as on a machine without the boundary.
        assert!(std::env::var_os("FIXTURE_NO_USER_NAMESPACES").is_none());
        let folder = std::env::temp_dir();
        let output = std::process::Command::new("bwrap")
            .args([
                "--unshare-user",
                "--disable-userns",
                "--ro-bind",
                "/",
                "/",
                "--dev",
                "/dev",
                "--proc",
                "/proc",
                "--bind",
            ])
            .arg(&folder)
            .arg(&folder)
            .arg("--")
            .arg(std::env::current_exe().unwrap())
            .args(["--exact", NAME])
            .env("FIXTURE_NO_USER_NAMESPACES", "1")
            .output()
            .unwrap();
        let output = String::from_utf8_lossy(&output.stdout).into_owned();
        assert!(output.contains("1 passed"), "{output}");
        return;
    }
    let root = tempfile::tempdir().unwrap();
    let binding = root.path().join("sample");
    let home = binding.join("home");
    let cwd = home.join("work");
    let provider = home.join(".codex");
    fs::create_dir_all(&cwd).unwrap();
    fs::create_dir(&provider).unwrap();
    fs::write(provider.join("session_index.jsonl.lock"), "parent").unwrap();
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
    let script = r#"
        read -r marker || exit 0
        echo "$CODEX_HOME"
        read -r done
    "#;
    launches.ensure(serde_json::from_value(json!({
        "commandId":"start", "conversationId":uuid::Uuid::new_v4(), "bindingId":"sample", "generation":1,
        "spec":{"agent":"codex","projectId":"project","runId":"run","cwd":cwd,"home":home,
            "adapter":{"path":"/bin/sh","version":"2.7.0","args":["-c",script]},
            "additions":{"promptEcho":false,"statusEvents":false,"recording":false,"clientMcp":false}}
    })).unwrap()).unwrap();
    let mut side = launches.spawn_side("sample", 1, "lilac").unwrap();
    side.stdin.write_all(b"lilac\n").await.unwrap();
    let mut output = BufReader::new(side.stdout).lines();
    assert_eq!(
        output.next_line().await.unwrap(),
        Some(provider.canonicalize().unwrap().display().to_string())
    );
    assert!(matches!(
        launches
            .spawn_side("sample", 1, "full")
            .err()
            .unwrap()
            .downcast_ref(),
        Some(nori_harness::managed_launch::LaunchRefusal::AtCapacity)
    ));
    launches.stop_side("sample", 1, "lilac").await.unwrap();
    assert_eq!(output.next_line().await.unwrap(), None);
    let _replacement = launches.spawn_side("sample", 1, "moss").unwrap();
    launches.shutdown().await.unwrap();
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn codex_sides_start_from_a_machine_environment_without_its_other_variables() {
    if !sandbox_available() {
        return;
    }
    let root = tempfile::tempdir().unwrap();
    let binding = root.path().join("sample");
    let home = binding.join("home");
    let cwd = home.join("work");
    fs::create_dir_all(&cwd).unwrap();
    fs::create_dir(home.join(".codex")).unwrap();
    let launches = ManagedLaunches::from_config(
        &nori_config::HostConfig {
            managed: true,
            side_sessions: true,
            bindings_root: Some(root.path().into()),
            ..Default::default()
        },
        root.path(),
    )
    .unwrap();
    // The parent keeps every variable. Its side keeps only the recognised ones.
    let script = r#"
        read -r role || exit 0
        echo "$role ${TZ:-} ${SERVICE_TOKEN:-absent} ${OPENAI_API_KEY:-absent}"
        read -r done
    "#;
    launches.ensure(serde_json::from_value(json!({
        "commandId":"start", "conversationId":uuid::Uuid::new_v4(), "bindingId":"sample", "generation":1,
        "spec":{"agent":"codex","projectId":"project","runId":"run","cwd":cwd,"home":home,
            "env":{"PATH":"/usr/bin:/bin","TZ":"UTC","SERVICE_TOKEN":"synthetic","OPENAI_API_KEY":"synthetic"},
            "adapter":{"path":"/bin/sh","version":"2.7.0","args":["-c",script]},
            "additions":{"promptEcho":false,"statusEvents":false,"recording":false,"clientMcp":false}}
    })).unwrap()).unwrap();
    let mut observed = Vec::new();
    for (role, mut pipe) in [
        ("side", launches.spawn_side("sample", 1, "lilac").unwrap()),
        ("parent", launches.take_connection("sample", 1).unwrap()),
    ] {
        pipe.stdin
            .write_all(format!("{role}\n").as_bytes())
            .await
            .unwrap();
        let mut output = BufReader::new(pipe.stdout).lines();
        observed.push(output.next_line().await.unwrap().unwrap());
    }
    assert_eq!(
        observed,
        ["side UTC absent absent", "parent UTC synthetic synthetic"]
    );
    launches.shutdown().await.unwrap();
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn codex_refuses_unverified_credential_scopes_before_spawning() {
    let root = tempfile::tempdir().unwrap();
    let binding = root.path().join("sample");
    let home = binding.join("home");
    let cwd = binding.join("workspace");
    let provider = home.join(".codex");
    fs::create_dir_all(&cwd).unwrap();
    fs::create_dir_all(&provider).unwrap();
    let subscription = json!(["sessions:codex-subscription-file:v1"]);
    for (file, content, refs, env, reason) in [
        ("config.toml", "", subscription, json!({}), "auth.json"),
        (
            "auth.json",
            "synthetic",
            json!(["sessions:codex-subscription-file:v1", "second-source"]),
            json!({}),
            "credential references",
        ),
        ("auth.json", "synthetic", json!([]), json!({}), "auth.json"),
        (
            ".credentials.json",
            "synthetic",
            json!([]),
            json!({}),
            ".credentials.json",
        ),
        (
            "config.toml",
            "",
            json!(["synthetic-ref"]),
            json!({}),
            "credential references",
        ),
        (
            "config.toml",
            "cli_auth_credentials_store = 'keyring'",
            json!([]),
            json!({}),
            "cli_auth_credentials_store",
        ),
        (
            "config.toml",
            "cli_auth_credentials_store = 'auto'",
            json!([]),
            json!({}),
            "cli_auth_credentials_store",
        ),
    ] {
        fs::write(provider.join(file), content).unwrap();
        let launches = ManagedLaunches::from_config(
            &nori_config::HostConfig {
                managed: true,
                side_sessions: true,
                side_confinement: nori_config::SideConfinement::Required,
                bindings_root: Some(root.path().into()),
                ..Default::default()
            },
            root.path(),
        )
        .unwrap();
        launches.ensure(serde_json::from_value(json!({
            "commandId":"start", "conversationId":uuid::Uuid::new_v4(), "bindingId":"sample", "generation":1,
            "spec":{"agent":"codex","projectId":"project","runId":"run","cwd":cwd,"home":home,"credentialRefs":refs,"env":env,
                "adapter":{"path":"/bin/cat","version":"2.7.0","args":[]},
                "additions":{"promptEcho":false,"statusEvents":false,"recording":false,"clientMcp":false}}
        })).unwrap()).unwrap();
        let refusal = launches
            .spawn_side("sample", 1, "side")
            .err()
            .expect("unsupported launch must be refused before spawn");
        assert!(
            refusal.to_string().contains(reason),
            "expected {reason:?} in {refusal}"
        );
        launches.shutdown().await.unwrap();
        fs::remove_file(provider.join(file)).unwrap();
    }
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn ordinary_read_only_codex_keeps_its_credential_home_with_either_flag_value() {
    if !sandbox_available() {
        return;
    }
    let root = tempfile::tempdir().unwrap();
    let binding = root.path().join("sample");
    let home = binding.join("home");
    let cwd = binding.join("work");
    fs::create_dir_all(home.join(".codex")).unwrap();
    fs::create_dir(&cwd).unwrap();
    fs::write(home.join(".codex/auth.json"), "scope-fixture").unwrap();
    for enabled in [false, true] {
        let launches = ManagedLaunches::from_config(
            &nori_config::HostConfig {
                managed: true,
                side_sessions: enabled,
                bindings_root: Some(root.path().into()),
                ..Default::default()
            },
            root.path(),
        )
        .unwrap();
        launches.ensure(serde_json::from_value(json!({
            "bindingId":"sample","generation":1,"conversationId":uuid::Uuid::new_v4(),"commandId":"regular",
            "spec":{"agent":"codex","projectId":"p","runId":"r","cwd":cwd,"home":home,"workspaceReadOnly":true,
                "credentialRefs":["synthetic-file"],"adapter":{"path":"/bin/sh","version":"2.7.0",
                "args":["-c",r#"test "$CODEX_HOME" = "$HOME/.codex" && ! touch "$PWD/probe" && cat "$CODEX_HOME/auth.json""#]},
                "additions":{"clientMcp":false,"recording":false,"statusEvents":false,"promptEcho":false}}
        })).unwrap()).unwrap();
        let child = launches.take_connection("sample", 1).unwrap();
        let mut result = String::new();
        BufReader::new(child.stdout)
            .read_line(&mut result)
            .await
            .unwrap();
        assert_eq!(result, "scope-fixture");
        launches.shutdown().await.unwrap();
    }
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn subscription_sides_follow_the_admitted_files_atomic_replacement() {
    if !sandbox_available() {
        return;
    }
    let root = tempfile::tempdir().unwrap();
    let binding = root.path().join("sample");
    let home = binding.join("home");
    let cwd = binding.join("work");
    let provider = home.join(".codex");
    fs::create_dir_all(&provider).unwrap();
    fs::create_dir(&cwd).unwrap();
    let auth = provider.join("auth.json");
    fs::write(&auth, "subscription-generation-one\n").unwrap();
    let config = r#"
        model_provider = "openai"
        cli_auth_credentials_store = "file"
        forced_chatgpt_workspace_id = ["fixture-workspace"]
        [projects."/synthetic/work"]
        trust_level = "trusted"
        [notice.model_migrations]
        fixture = "replacement"
    "#;
    fs::write(provider.join("config.toml"), config).unwrap();
    fs::write(provider.join("auth.json.456.tmp"), "unfinished").unwrap();
    let launches = ManagedLaunches::from_config(
        &nori_config::HostConfig {
            managed: true,
            side_sessions: true,
            bindings_root: Some(root.path().into()),
            max_sessions: 3,
            ..Default::default()
        },
        root.path(),
    )
    .unwrap();
    let script = r#"
        set -eu
        read -r marker
        if echo changed >> "$CODEX_HOME/auth.json" || touch "$HOME/created" || touch "$PWD/created"; then exit 2; fi
        test ! -e "$CODEX_HOME/installation_id"
        echo "$marker" > "$CODEX_HOME/installation_id"
        cat "$CODEX_HOME/auth.json"
        read -r again
        cat "$CODEX_HOME/auth.json" "$CODEX_HOME/installation_id"
    "#;
    launches.ensure(serde_json::from_value(json!({
        "commandId":"start", "conversationId":uuid::Uuid::new_v4(), "bindingId":"sample", "generation":1,
        "spec":{"agent":"codex","projectId":"project","runId":"run","cwd":cwd,"home":home,
            "credentialRefs":["sessions:codex-subscription-file:v1"],
            "adapter":{"path":"/bin/sh","version":"2.7.0","args":["-c",script]},
            "additions":{"promptEcho":false,"statusEvents":false,"recording":false,"clientMcp":false}}
    })).unwrap()).unwrap();
    let original = provider.join("auth.json.789.tmp");
    fs::rename(&auth, &original).unwrap();
    std::os::unix::fs::symlink(&original, &auth).unwrap();
    let refused = launches
        .spawn_side("sample", 1, "redirected")
        .err()
        .unwrap();
    assert!(refused.to_string().contains("auth.json input type"));
    fs::rename(&original, &auth).unwrap();
    let mut sides = Vec::new();
    for id in ["lilac", "moss"] {
        let mut child = launches.spawn_side("sample", 1, id).unwrap();
        let mut output = BufReader::new(child.stdout).lines();
        child
            .stdin
            .write_all(format!("{id}\n").as_bytes())
            .await
            .unwrap();
        assert_eq!(
            output.next_line().await.unwrap().as_deref(),
            Some("subscription-generation-one")
        );
        sides.push((id, child.stdin, output));
    }
    let replacement = provider.join("auth.json.123.tmp");
    fs::write(&replacement, "subscription-generation-two\n").unwrap();
    fs::rename(replacement, &auth).unwrap();
    for (id, mut input, mut output) in sides {
        input.write_all(b"again\n").await.unwrap();
        assert_eq!(
            output.next_line().await.unwrap().as_deref(),
            Some("subscription-generation-two")
        );
        assert_eq!(output.next_line().await.unwrap().as_deref(), Some(id));
        launches.stop_side("sample", 1, id).await.unwrap();
    }
    launches.shutdown().await.unwrap();
    assert!(!provider.join("installation_id").exists());
    assert!(!home.join("created").exists());
    assert!(!cwd.join("created").exists());
    assert_eq!(
        fs::read_to_string(provider.join("config.toml")).unwrap(),
        config
    );
    assert_eq!(
        fs::read_to_string(auth).unwrap(),
        "subscription-generation-two\n"
    );
}

#[tokio::test]
async fn rebuilt_side_refuses_a_redirected_prepared_directory() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("sample/home");
    let cwd = root.path().join("sample/work");
    let runtime = home.join("prepared");
    fs::create_dir_all(&runtime).unwrap();
    fs::create_dir(&cwd).unwrap();
    let launches = ManagedLaunches::from_config(
        &nori_config::HostConfig {
            managed: true,
            side_sessions: true,
            bindings_root: Some(root.path().into()),
            ..Default::default()
        },
        root.path(),
    )
    .unwrap();
    let request = serde_json::from_value(json!({
        "commandId":"start","conversationId":uuid::Uuid::new_v4(),"bindingId":"sample","generation":1,
        "spec":{"agent":"fixture","projectId":"project","runId":"run","cwd":cwd,"home":home,
            "runtimeDir":runtime,"adapter":{"path":"/bin/sh","version":"fixture","args":["-c","read -r line"]},
            "additions":{"promptEcho":false,"statusEvents":false,"recording":false,"clientMcp":false}}
    })).unwrap();
    launches.ensure(request).unwrap();
    let moved = home.join("elsewhere");
    fs::rename(&runtime, &moved).unwrap();
    std::os::unix::fs::symlink(moved, runtime).unwrap();
    assert!(launches.spawn_side("sample", 1, "side").is_err());
    launches.shutdown().await.unwrap();
}
