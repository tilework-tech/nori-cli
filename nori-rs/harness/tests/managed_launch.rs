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
