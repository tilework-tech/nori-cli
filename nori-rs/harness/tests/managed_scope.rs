//! Scope validation at the harness boundary, with a real stdio peer.
#![cfg(unix)]

use std::path::Path;
use std::process::Stdio;

use anyhow::Context;
use nori_harness::backend::managed_additions::ManagedSessionAdditions;
use nori_harness::managed_acp;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;

#[tokio::test]
async fn refused_operations_leave_the_admitted_workspace_and_peer_usable() -> anyhow::Result<()> {
    let workspace = tempfile::tempdir()?;
    let other = tempfile::tempdir()?;
    let cwd = workspace.path().canonicalize()?;
    let mut peer = tokio::process::Command::new("python3")
        .args([
            "-u",
            "-c",
            r#"
import json, sys
number = 0
for line in sys.stdin:
    req = json.loads(line)
    if req["method"] == "initialize":
        result = {"protocolVersion": 1, "agentCapabilities": {}}
    elif req["method"] == "session/new":
        number += 1
        result = {"sessionId": str(number)}
    else:
        result = {"_meta": {"received": req["method"]}}
    print(json.dumps({"jsonrpc": "2.0", "id": req["id"], "result": result}), flush=True)
"#,
        ])
        .current_dir(&cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;
    let mut lane = managed_acp::connect(
        peer.stdout.take().context("fixture stdout")?,
        peer.stdin.take().context("fixture stdin")?,
        cwd.clone(),
        ManagedSessionAdditions::new(
            workspace.path().into(),
            cwd.clone(),
            "fixture".into(),
            false,
            false,
        ),
        true,
        false,
    );
    let new = |path: &Path| json!({"cwd":path, "mcpServers":[]});
    for (method, params, expected) in [
        (
            "initialize",
            json!({"protocolVersion":1, "clientCapabilities":{}}),
            Value::Null,
        ),
        ("session/new", new(other.path()), json!(-32602)),
        (
            "session/new",
            json!({"cwd":cwd,"mcpServers":[],"additionalDirectories":[other.path()]}),
            json!(-32602),
        ),
        ("session/new", new(&cwd), Value::Null),
        (
            "session/prompt",
            json!({"sessionId":"agent-owned","prompt":[]}),
            Value::Null,
        ),
        (
            "session/prompt",
            json!({"sessionId":"1","_meta":"invalid","prompt":[]}),
            json!(-32602),
        ),
        ("_nori/host/stop", json!({}), json!(-32601)),
        ("session/close", json!({"sessionId":"1"}), Value::Null),
    ] {
        lane.input
            .send(json!({"jsonrpc":"2.0","id":"same","method":method,"params":params}))
            .await?;
        let reply = tokio::time::timeout(std::time::Duration::from_secs(2), lane.output.recv())
            .await?
            .context("peer disconnected")?;
        assert_eq!(reply["id"], "same");
        assert_eq!(&reply["error"]["code"], &expected, "{reply}");
        if method == "session/new" && expected.is_null() {
            assert_eq!(reply["result"]["sessionId"], "1");
        }
        if method == "session/prompt" && expected.is_null() {
            assert_eq!(
                reply["result"],
                json!({"_meta":{"received":"session/prompt"}})
            );
        }
    }
    Ok(())
}
