//! The checked-in host wire schema and TypeScript types are generated from
//! the protocol types `nori host` serves.
//!
//! Regenerate both with `scripts/generate-host-wire-types.sh` from the
//! repository root. That script sets `NORI_WRITE_HOST_WIRE_SCHEMA` to have the
//! first test rewrite the schema instead of only comparing it.

use std::path::Path;
use std::process::Command;

use pretty_assertions::assert_eq;

fn repository() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .unwrap_or_else(|| panic!("the cli crate lives two levels below the repository"))
}

#[test]
fn the_checked_in_host_wire_schema_matches_the_protocol_types() {
    let generated = format!(
        "{}\n",
        serde_json::to_string_pretty(&nori_protocol::host_wire_schema())
            .unwrap_or_else(|error| panic!("schema serializes: {error}"))
    );
    let path = repository().join("nori-rs/nori-protocol/schema/host-wire.schema.json");
    if std::env::var_os("NORI_WRITE_HOST_WIRE_SCHEMA").is_some() {
        std::fs::write(&path, &generated)
            .unwrap_or_else(|error| panic!("write {}: {error}", path.display()));
    }
    let checked_in = std::fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!(
            "read {}: {error}; run scripts/generate-host-wire-types.sh",
            path.display()
        )
    });
    assert_eq!(
        checked_in, generated,
        "the host wire schema is out of date; run scripts/generate-host-wire-types.sh"
    );
}

/// Needs Node.js and npm; machines without them skip the comparison, except
/// in CI.
#[test]
fn the_checked_in_host_wire_typescript_matches_the_schema() {
    if which::which("npm").is_err() || which::which("node").is_err() {
        assert!(
            std::env::var_os("CI").is_none(),
            "CI must check the TypeScript types; install node and npm"
        );
        eprintln!("skipping: the TypeScript check needs node and npm on PATH");
        return;
    }
    let output = Command::new("bash")
        .arg(repository().join("scripts/generate-host-wire-types.sh"))
        .arg("--check")
        .output()
        .unwrap_or_else(|error| panic!("run generate-host-wire-types.sh: {error}"));
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
