// Test to verify that HTTP backend dependencies are NOT included in nori binary.
// This test ensures we've successfully removed the legacy HTTP backend code.

use std::process::Command;

#[test]
fn nori_does_not_depend_on_http_backend_crates() {
    // Run cargo tree to get the dependency graph for nori-cli
    let output = Command::new("cargo")
        .args(["tree", "-p", "nori-cli"])
        .output()
        .expect("Failed to execute cargo tree");

    assert!(
        output.status.success(),
        "cargo tree command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let dep_tree = String::from_utf8_lossy(&output.stdout);

    // Verify codex-api is NOT in the dependency tree
    assert!(
        !dep_tree.contains("codex-api"),
        "nori-cli should NOT depend on codex-api (HTTP backend crate). Found in dependency tree:\n{}",
        dep_tree
    );

    // Verify codex-client is NOT in the dependency tree
    assert!(
        !dep_tree.contains("codex-client"),
        "nori-cli should NOT depend on codex-client (HTTP transport crate). Found in dependency tree:\n{}",
        dep_tree
    );

    // Verify eventsource-stream is NOT in the dependency tree
    // (This is only used for HTTP SSE parsing)
    assert!(
        !dep_tree.contains("eventsource-stream"),
        "nori-cli should NOT depend on eventsource-stream (HTTP SSE parser). Found in dependency tree:\n{}",
        dep_tree
    );
}

#[test]
fn nori_still_depends_on_acp_backend() {
    // Run cargo tree to get the dependency graph for nori-cli
    let output = Command::new("cargo")
        .args(["tree", "-p", "nori-cli"])
        .output()
        .expect("Failed to execute cargo tree");

    assert!(
        output.status.success(),
        "cargo tree command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let dep_tree = String::from_utf8_lossy(&output.stdout);

    // Verify codex-acp IS in the dependency tree
    assert!(
        dep_tree.contains("codex-acp"),
        "nori-cli MUST depend on codex-acp (ACP backend crate). Not found in dependency tree:\n{}",
        dep_tree
    );

    // Verify agent-client-protocol IS in the dependency tree
    assert!(
        dep_tree.contains("agent-client-protocol"),
        "nori-cli MUST depend on agent-client-protocol (ACP spec crate). Not found in dependency tree:\n{}",
        dep_tree
    );
}
